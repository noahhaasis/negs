// The C ABI's basics (core/include/negs_core.h): the version, owned byte
// blocks and the layout table the C layout check reads.
package negs_core

import "base:runtime"
import "core:mem"

ABI_VERSION :: 1
VERSION: cstring : "0.1.0 (odin dev-2026-11a)"

// negs_buf_t: an owned block, freed with negs_buf_free.
Buf :: struct {
	data: [^]u8,
	len:  uint,
}

@(export, link_name = "negs_core_abi_version")
abi_version :: proc "c" () -> u32 {
	return ABI_VERSION
}

@(export, link_name = "negs_core_version")
version :: proc "c" () -> cstring {
	return VERSION
}

// A block of `n` bytes in the core's allocator, for results the host owns.
buf_make :: proc(n: int) -> Buf {
	if n <= 0 {
		return {}
	}
	bytes, err := mem.alloc_bytes_non_zeroed(n)
	if err != nil {
		return {}
	}
	return Buf{raw_data(bytes), uint(n)}
}

// The block's bytes, copied out of `s`.
buf_from :: proc(s: []u8) -> Buf {
	b := buf_make(len(s))
	if b.data != nil {
		runtime.mem_copy_non_overlapping(b.data, raw_data(s), len(s))
	}
	return b
}

@(export, link_name = "negs_buf_copy")
buf_copy :: proc "c" (data: [^]u8, n: uint) -> Buf {
	context = core_context()
	if data == nil || n == 0 {
		return {}
	}
	return buf_from(data[:n])
}

@(export, link_name = "negs_buf_free")
buf_free :: proc "c" (b: Buf) {
	context = core_context()
	if b.data != nil {
		mem.free(b.data)
	}
}

// The host's string, borrowed for the call: (pointer, length) UTF-8.
host_string :: proc "contextless" (data: [^]u8, n: uint) -> string {
	if data == nil || n == 0 {
		return ""
	}
	return string(data[:n])
}

// negs_buf_free for callers already in an Odin context (tests).
buf_free_in_context :: proc(b: Buf) {
	if b.data != nil {
		mem.free(b.data)
	}
}
