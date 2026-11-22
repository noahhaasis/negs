// The embedded previews' C ABI (negs_core.h, "Phase 3c"): a header's JPEGs
// in one owned block (negs_previews_t, freed by negs_previews_free), the
// bytes of one in a negs_buf_t, and the choice between them.
package negs_core

import "core:mem"
import "../rules/embedded"

Previews_C :: struct {
	jpegs:           [^]embedded.Jpeg,
	count:           uint,
	orientation:     u16,
	has_orientation: u8,
	reserved:        [5]u8,
}

@(private = "file")
previews_out :: proc(p: ^embedded.Previews, out: ^Previews_C) {
	out^ = {}
	n := len(p.jpegs)
	if n > 0 {
		block, err := mem.alloc_bytes_non_zeroed(n * size_of(embedded.Jpeg), align_of(embedded.Jpeg))
		if err == nil {
			jpegs := ([^]embedded.Jpeg)(raw_data(block))
			copy(jpegs[:n], p.jpegs[:])
			out.jpegs, out.count = jpegs, uint(n)
		}
	}
	out.orientation, out.has_orientation = p.orientation, u8(p.has_orientation)
}

@(export, link_name = "negs_embedded_find_bytes")
embedded_find_bytes :: proc "c" (data: [^]u8, n: uint, out: ^Previews_C) -> u8 {
	context = core_context()
	if out == nil {
		return 0
	}
	out^ = {}
	src := embedded.source_bytes(data[:n] if data != nil else nil)
	p, ok := embedded.find_in(&src)
	defer embedded.previews_destroy(&p)
	if !ok {
		return 0
	}
	previews_out(&p, out)
	return 1
}

@(export, link_name = "negs_embedded_find")
embedded_find :: proc "c" (path: [^]u8, path_len: uint, out: ^Previews_C) -> u8 {
	context = core_context()
	if out == nil {
		return 0
	}
	out^ = {}
	p, ok := embedded.find(host_string(path, path_len))
	defer embedded.previews_destroy(&p)
	if !ok {
		return 0
	}
	previews_out(&p, out)
	return 1
}

@(export, link_name = "negs_previews_free")
previews_free :: proc "c" (p: ^Previews_C) {
	context = core_context()
	if p == nil {
		return
	}
	if p.jpegs != nil {
		mem.free(p.jpegs)
	}
	p^ = {}
}

@(export, link_name = "negs_embedded_bytes")
embedded_bytes :: proc "c" (path: [^]u8, path_len: uint, offset, length: u64, out: ^Buf) -> u8 {
	context = core_context()
	if out == nil {
		return 0
	}
	out^ = {}
	bytes, ok := embedded.read_file(host_string(path, path_len), offset, length)
	if !ok {
		return 0
	}
	if len(bytes) > 0 {
		out^ = Buf{raw_data(bytes), uint(len(bytes))}
	}
	return 1
}

@(private = "file")
jpegs_of :: proc "contextless" (jpegs: [^]embedded.Jpeg, count: uint) -> []embedded.Jpeg {
	return jpegs[:count] if jpegs != nil else nil
}

@(export, link_name = "negs_previews_largest")
previews_largest :: proc "c" (jpegs: [^]embedded.Jpeg, count: uint, out: ^embedded.Jpeg) -> u8 {
	j, ok := embedded.largest(jpegs_of(jpegs, count))
	if ok && out != nil {
		out^ = j
	}
	return u8(ok)
}

@(export, link_name = "negs_previews_for_size")
previews_for_size :: proc "c" (jpegs: [^]embedded.Jpeg, count: uint, pixels: u32, out: ^embedded.Jpeg) -> u8 {
	j, ok := embedded.for_size(jpegs_of(jpegs, count), pixels)
	if ok && out != nil {
		out^ = j
	}
	return u8(ok)
}

@(export, link_name = "negs_previews_ratio")
previews_ratio :: proc "c" (jpegs: [^]embedded.Jpeg, count: uint, orientation: u16, has_orientation: u8, out: ^f64) -> u8 {
	r, ok := embedded.ratio(jpegs_of(jpegs, count), orientation, has_orientation != 0)
	if ok && out != nil {
		out^ = r
	}
	return u8(ok)
}
