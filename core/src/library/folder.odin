// Listing a folder of photographs as `PhotoFolder` does
// (`PhotoBrowser.swift`; folder.rs): which files are photographs, their
// order, the sidecars beside them and each RAW's camera JPEG.
package library

import "core:slice"
import "core:strings"

// RAW files (`EditorModel.rawExtensions`).
RAW_EXTENSIONS := [30]string {
	"dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "ptx", "srw", "x3f",
	"iiq", "3fr", "ari", "bay", "crw", "dcr", "drf", "erf", "fff", "k25", "kdc", "mef", "mos", "mrw", "rwl",
}
// Other photographs Negs develops.
IMAGE_EXTENSIONS := [5]string{"jpg", "jpeg", "png", "tif", "tiff"}
// The camera's own picture beside a RAW of the same name.
CAMERA_PICTURE_EXTENSIONS := [5]string{"jpg", "jpeg", "heic", "heif", "hif"}

is_photo :: proc(path: string) -> bool {
	ext := extension_lower(path)
	return slice.contains(IMAGE_EXTENSIONS[:], ext) || slice.contains(RAW_EXTENSIONS[:], ext)
}

is_raw :: proc(path: string) -> bool {
	return slice.contains(RAW_EXTENSIONS[:], extension_lower(path))
}

is_camera_picture :: proc(path: string) -> bool {
	return slice.contains(CAMERA_PICTURE_EXTENSIONS[:], extension_lower(path))
}

// A file as listed: its size and date (nanoseconds since 1970).
Photo_File :: struct {
	path:         string,
	name:         string,
	has_modified: bool,
	modified:     i64,
	has_size:     bool,
	size:         u64,
}

photo_file_clone :: proc(f: Photo_File, allocator := context.allocator) -> Photo_File {
	out := f
	out.path = strings.clone(f.path, allocator)
	out.name = file_name(out.path)
	return out
}

photo_file_destroy :: proc(f: Photo_File, allocator := context.allocator) {
	delete(f.path, allocator)
}

// One folder listing, in the temporary allocator: its photographs in name
// order, the sidecars beside them (by the photograph's file name), and,
// when pairing, each RAW's camera picture.
Listing :: struct {
	photos:     [dynamic]Photo_File,
	sidecars:   map[string]Photo_File,
	// RAW → its camera JPEG (or HEIF).
	companions: map[string]string,
}

// Lists `directory` (`PhotoFolder.listing`). Hidden files are skipped, and
// so is anything that isn't a regular file.
listing :: proc(directory: string, pairing: bool) -> (Listing, Io_Error) {
	ta := context.temp_allocator
	entries, err := fs_read_dir(directory, ta)
	if err != 0 {
		return {}, err
	}
	out := Listing {
		photos     = make([dynamic]Photo_File, 0, len(entries), ta),
		sidecars   = make(map[string]Photo_File, ta),
		companions = make(map[string]string, ta),
	}
	heifs := make([dynamic]string, 0, 4, ta)
	for entry in entries {
		path := join(directory, entry.name, ta)
		ext := extension_lower(path)
		is_sidecar := ext == SIDECAR_EXTENSION
		photo := slice.contains(IMAGE_EXTENSIONS[:], ext) || slice.contains(RAW_EXTENSIONS[:], ext)
		heif := pairing && !photo && slice.contains(CAMERA_PICTURE_EXTENSIONS[:], ext)
		if !(is_sidecar || heif || photo) {
			continue
		}
		if !entry.ok || !entry.stat.regular || entry.hidden {
			continue
		}
		if heif {
			append(&heifs, path)
			continue
		}
		file := Photo_File {
			path         = path,
			name         = file_name(path),
			has_modified = true,
			modified     = entry.stat.modified,
			has_size     = true,
			size         = u64(entry.stat.size),
		}
		if is_sidecar {
			stem, _, _ := split_extension(file.name)
			out.sidecars[stem] = file
		} else {
			append(&out.photos, file)
		}
	}
	slice.stable_sort_by(out.photos[:], proc(a, b: Photo_File) -> bool {return standard_compare(a.name, b.name) < 0})
	if pairing {
		pair(&out, heifs[:])
	}
	return out, 0
}

// Takes each RAW's camera picture out of the photographs: the first in
// name order with the RAW's name, unless it has a sidecar of its own.
@(private = "file")
pair :: proc(l: ^Listing, heifs: []string) {
	ta := context.temp_allocator
	raws := make(map[string]string, ta)
	for photo in l.photos {
		if is_raw(photo.path) {
			stem := stem_lower(photo.path)
			if stem not_in raws {
				raws[stem] = photo.path
			}
		}
	}
	if len(raws) == 0 {
		return
	}
	slice.stable_sort_by(heifs, proc(a, b: string) -> bool {return standard_compare(file_name(a), file_name(b)) < 0})
	pictures := make([dynamic]string, 0, len(l.photos) + len(heifs), ta)
	for p in l.photos {
		if is_camera_picture(p.path) {
			append(&pictures, p.path)
		}
	}
	append(&pictures, ..heifs)
	taken := make(map[string]bool, ta)
	for picture in pictures {
		raw, found := raws[stem_lower(picture)]
		if !found {
			continue
		}
		if raw in l.companions || file_name(picture) in l.sidecars {
			continue
		}
		l.companions[raw] = picture
		taken[picture] = true
	}
	if len(taken) > 0 {
		kept := 0
		for p in l.photos {
			if p.path not_in taken {
				l.photos[kept] = p
				kept += 1
			}
		}
		resize(&l.photos, kept)
	}
}

// The RAW a camera picture goes with, when pairing takes it: what opening
// it opens instead (`PhotoFolder.pairedRAW`). Temporary.
paired_raw :: proc(picture: string) -> (string, bool) {
	if !is_camera_picture(picture) {
		return "", false
	}
	dir, has_dir := parent(picture)
	if !has_dir {
		return "", false
	}
	found, err := listing(dir, true)
	if err != 0 {
		return "", false
	}
	name := file_name(picture)
	if name == "" {
		return "", false
	}
	for raw, p in found.companions {
		if file_name(p) == name {
			return raw, true
		}
	}
	return "", false
}
