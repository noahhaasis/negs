// size_of / offset_of for every struct in negs_core.h, by its C name, so
// core/tests/c/layout_check.c can hold the two sides to the same layout.
package negs_core

@(private)
layout_table := [?]struct {
	name:  string,
	value: i64,
} {
	{"negs_buf_t", size_of(Buf)},
	{"negs_buf_t.data", i64(offset_of(Buf, data))},
	{"negs_buf_t.len", i64(offset_of(Buf, len))},
	{"negs_alloc_stats_t", size_of(Alloc_Stats)},
	{"negs_alloc_stats_t.live_bytes", i64(offset_of(Alloc_Stats, live_bytes))},
	{"negs_alloc_stats_t.live_count", i64(offset_of(Alloc_Stats, live_count))},
	{"negs_alloc_stats_t.total_count", i64(offset_of(Alloc_Stats, total_count))},
	{"negs_alloc_stats_t.bad_frees", i64(offset_of(Alloc_Stats, bad_frees))},
	{"negs_alloc_stats_t.tracked", i64(offset_of(Alloc_Stats, tracked))},
}

@(export, link_name = "negs_core_layout")
layout :: proc "c" (name: [^]u8, n: uint) -> i64 {
	key := host_string(name, n)
	for entry in layout_table {
		if entry.name == key {
			return entry.value
		}
	}
	return -1
}
