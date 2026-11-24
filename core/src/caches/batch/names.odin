// The names a batch gives: `{name}`, numbered names, and names unique
// among the batch's own.
package batch

import "core:strconv"
import "core:strings"
import facts "../../rules/facts"
import files "../files"

// `batch_name`: the photograph's file name without its extension, a
// duplicate's number after it (`IMG_0412 (2)` for the title
// `IMG_0412.CR3 (2)`).
batch_name :: proc(photo: string, title: string, allocator := context.allocator, style := files.STYLE) -> string {
	file := ""
	if name, ok := files.file_name(photo, style); ok {
		file = files.display(name, context.temp_allocator)
	}
	suffix := title[len(file):] if strings.has_prefix(title, file) else ""
	stem := ""
	if s, ok := files.file_stem(photo, style); ok {
		stem = files.display(s, context.temp_allocator)
	}
	return strings.concatenate({stem, suffix}, allocator)
}

// The `n`th name `numbered` tries for `file_name` (n >= 2):
// `{stem} {n}.{extension}`, the dot there even without an extension.
numbered_name :: proc(file_name: string, n: u64, allocator := context.allocator, style := files.STYLE) -> string {
	stem, _ := files.file_stem(file_name, style)
	ext, _ := files.extension(file_name, style)
	buf: [24]u8
	digits := strconv.write_uint(buf[:], n, 10)
	return strings.concatenate(
		{files.display(stem, context.temp_allocator), " ", digits, ".", files.display(ext, context.temp_allocator)},
		allocator,
	)
}

// `numbered`: `file_name`, then `name 2.ext`, `name 3.ext`… until `taken`
// says no.
numbered :: proc(
	file_name: string,
	taken: proc(name: string, data: rawptr) -> bool,
	data: rawptr,
	allocator := context.allocator,
	style := files.STYLE,
) -> string {
	if !taken(file_name, data) {
		return strings.clone(file_name, allocator)
	}
	n: u64 = 2
	for {
		name := numbered_name(file_name, n, allocator, style)
		if !taken(name, data) {
			return name
		}
		delete(name, allocator)
		n += 1
	}
}

// `unique`: the batch's own names made unique among themselves, in order,
// ignoring case as Windows does (str::to_lowercase).
unique :: proc(names: []string, allocator := context.allocator, style := files.STYLE) -> []string {
	used := make(map[string]struct{}, len(names), context.temp_allocator)
	out := make([]string, len(names), allocator)
	taken :: proc(name: string, data: rawptr) -> bool {
		used := (^map[string]struct{})(data)
		return facts.lower(name, context.temp_allocator) in used^
	}
	for name, i in names {
		u := numbered(name, taken, &used, allocator, style)
		used[facts.lower(u, context.temp_allocator)] = {}
		out[i] = u
	}
	return out
}
