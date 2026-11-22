// A RAW file's embedded JPEG previews found from its header, without
// reading the file whole or decoding anything (windows/src/embedded_preview.rs,
// frozen in core/diff/oracle). The hosts decode the smallest one big enough
// and know every photograph's shape from the largest one's size.
//
// TIFF-based RAWs (CR2, NEF, ARW, DNG, ORF, RW2, PEF and the like): the
// JPEGs named by JPEGInterchangeFormat, by a single strip with JPEG
// compression, or by RW2's JpgFromRaw, in IFD0, the IFDs chained after it
// and their SubIFDs. Fuji RAF: the JPEG its header points at. Anything else
// (CR3, a lossless-JPEG raw strip, a tiled preview) finds nothing.
package embedded

// One embedded JPEG: where it is and its size as stored (negs_jpeg_t).
Jpeg :: struct {
	offset: u64,
	length: u64,
	width:  u32,
	height: u32,
}

// What the header says: the previews (in the order the IFDs name them,
// each offset once), and the EXIF orientation (1 to 8) if the file's own
// IFDs give one.
Previews :: struct {
	jpegs:           [dynamic]Jpeg,
	orientation:     u16,
	has_orientation: bool,
}

previews_destroy :: proc(p: ^Previews) {
	delete(p.jpegs)
	p^ = {}
}

// Previews further than this from the largest one's shape are left out: a
// camera's small thumbnail is often 4:3 with black bands round a 3:2
// picture.
SHAPE_TOLERANCE: f64 = 0.015

@(private)
long_side :: proc "contextless" (j: Jpeg) -> u32 {
	return max(j.width, j.height)
}

@(private)
ratio_of :: proc "contextless" (j: Jpeg) -> f64 {
	return f64(j.width) / f64(max(j.height, 1))
}

// The largest preview by pixels, then length; of equals, the last (Rust's
// max_by_key).
largest :: proc "contextless" (jpegs: []Jpeg) -> (Jpeg, bool) {
	if len(jpegs) == 0 {
		return {}, false
	}
	best := jpegs[0]
	best_area := u64(best.width) * u64(best.height)
	for j in jpegs[1:] {
		area := u64(j.width) * u64(j.height)
		if area > best_area || (area == best_area && j.length >= best.length) {
			best, best_area = j, area
		}
	}
	return best, true
}

// The smallest preview at least `pixels` on its long side and of the
// largest one's shape (of equals, the first), else the largest.
for_size :: proc "contextless" (jpegs: []Jpeg, pixels: u32) -> (Jpeg, bool) {
	big, ok := largest(jpegs)
	if !ok {
		return {}, false
	}
	shape := ratio_of(big)
	found := false
	best: Jpeg
	for j in jpegs {
		if !(abs((ratio_of(j) - shape) / shape) <= SHAPE_TOLERANCE) || long_side(j) < pixels {
			continue
		}
		if !found || long_side(j) < long_side(best) || (long_side(j) == long_side(best) && j.length < best.length) {
			best, found = j, true
		}
	}
	return (best if found else big), true
}

// Width ÷ height as shown upright: the largest preview's, turned by the
// orientation.
ratio :: proc "contextless" (jpegs: []Jpeg, orientation: u16, has_orientation: bool) -> (f64, bool) {
	j, ok := largest(jpegs)
	if !ok {
		return 0, false
	}
	if has_orientation && orientation >= 5 {
		return f64(j.height) / f64(max(j.width, 1)), true
	}
	return ratio_of(j), true
}
