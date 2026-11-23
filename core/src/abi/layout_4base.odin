// Phase 4 base's structs in negs_core.h: size_of / offset_of by C name
// (see layout.odin): value, edit state (core/src/value, core/src/state).
package negs_core

@(private)
layout_4base := []Layout_Entry {
	{"negs_str_t", size_of(Str_C)},
	{"negs_str_t.data", i64(offset_of(Str_C, data))},
	{"negs_str_t.len", i64(offset_of(Str_C, len))},
}
