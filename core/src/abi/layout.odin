// size_of / offset_of for every struct in negs_core.h, by its C name, so
// core/tests/c/layout_check.c can hold the two sides to the same layout.
package negs_core

import "../marks"
import "../theme"

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
	{"negs_pt_t", size_of(marks.Pt)},
	{"negs_pt_t.y", i64(offset_of(marks.Pt, y))},
	{"negs_rect_t", size_of(marks.Rect)},
	{"negs_rect_t.h", i64(offset_of(marks.Rect, h))},
	{"negs_seg_t", size_of(marks.Seg)},
	{"negs_seg_t.kind", i64(offset_of(marks.Seg, kind))},
	{"negs_seg_t.p", i64(offset_of(marks.Seg, p))},
	{"negs_radii_t", size_of(marks.Radii)},
	{"negs_radii_t.vertical", i64(offset_of(marks.Radii, vertical))},
	{"negs_radii_t.relative", i64(offset_of(marks.Radii, relative))},
	{"negs_strokes_t", size_of(Strokes)},
	{"negs_strokes_t.points", i64(offset_of(Strokes, points))},
	{"negs_strokes_t.counts", i64(offset_of(Strokes, counts))},
	{"negs_strokes_t.opacities", i64(offset_of(Strokes, opacities))},
	{"negs_strokes_t.stroke_count", i64(offset_of(Strokes, stroke_count))},
	{"negs_strokes_t.point_count", i64(offset_of(Strokes, point_count))},
	{"negs_strokes_t.kind", i64(offset_of(Strokes, kind))},
	{"negs_strokes_t.tilt", i64(offset_of(Strokes, tilt))},
	{"negs_marks_constants_t", size_of(Marks_Constants)},
	{"negs_marks_constants_t.reach", i64(offset_of(Marks_Constants, reach))},
	{"negs_theme_hex_t", size_of(theme.Hex)},
	{"negs_theme_hex_t.on_fill", i64(offset_of(theme.Hex, on_fill))},
	{"negs_theme_hex_t.blank", i64(offset_of(theme.Hex, blank))},
	{"negs_theme_t", size_of(Theme)},
	{"negs_theme_t.film", i64(offset_of(Theme, film))},
	{"negs_theme_t.dark", i64(offset_of(Theme, dark))},
	{"negs_theme_t.plain", i64(offset_of(Theme, plain))},
	{"negs_theme_t.id", i64(offset_of(Theme, id))},
	{"negs_theme_t.key", i64(offset_of(Theme, key))},
	{"negs_theme_t.title", i64(offset_of(Theme, title))},
	{"negs_theme_constants_t", size_of(Theme_Constants)},
	{"negs_theme_constants_t.highlighter_opacity", i64(offset_of(Theme_Constants, highlighter_opacity))},
	{"negs_theme_constants_t.dimming_light", i64(offset_of(Theme_Constants, dimming_light))},
	{"negs_bin_t", size_of(Bin_C)},
	{"negs_bin_t.user", i64(offset_of(Bin_C, user))},
	{"negs_bin_t.name", i64(offset_of(Bin_C, name))},
	{"negs_bin_t.available", i64(offset_of(Bin_C, available))},
	{"negs_bin_t.trash", i64(offset_of(Bin_C, trash))},
	{"negs_bin_t.put_back", i64(offset_of(Bin_C, put_back))},
	{"negs_bin_t.remove", i64(offset_of(Bin_C, remove))},
	{"negs_bin_t.release", i64(offset_of(Bin_C, release))},
	{"negs_library_config_t", size_of(Library_Config)},
	{"negs_library_config_t.folder", i64(offset_of(Library_Config, folder))},
	{"negs_library_config_t.folder_len", i64(offset_of(Library_Config, folder_len))},
	{"negs_library_config_t.data", i64(offset_of(Library_Config, data))},
	{"negs_library_config_t.data_len", i64(offset_of(Library_Config, data_len))},
	{"negs_library_config_t.app", i64(offset_of(Library_Config, app))},
	{"negs_library_config_t.app_len", i64(offset_of(Library_Config, app_len))},
	{"negs_library_config_t.bin", i64(offset_of(Library_Config, bin))},
	{"negs_library_config_t.pairing", i64(offset_of(Library_Config, pairing))},
	{"negs_library_config_t.sidecars", i64(offset_of(Library_Config, sidecars))},
	{"negs_library_config_t.has_id_seed", i64(offset_of(Library_Config, has_id_seed))},
	{"negs_library_config_t.id_seed", i64(offset_of(Library_Config, id_seed))},
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
