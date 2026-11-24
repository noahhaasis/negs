// Phase 4b's structs in negs_core.h: size_of / offset_of by C name
// (see layout.odin): presets, session.
package negs_core

@(private)
layout_4b := []Layout_Entry {
	{"negs_preset_file_t", size_of(Preset_File_C)},
	{"negs_preset_file_t.path", i64(offset_of(Preset_File_C, path))},
	{"negs_preset_file_t.path_len", i64(offset_of(Preset_File_C, path_len))},
	{"negs_preset_file_t.size", i64(offset_of(Preset_File_C, size))},
	{"negs_preset_file_t.modified_s", i64(offset_of(Preset_File_C, modified_s))},
	{"negs_preset_file_t.modified_ns", i64(offset_of(Preset_File_C, modified_ns))},
	{"negs_preset_list_t", size_of(Preset_List_C)},
	{"negs_preset_list_t.files", i64(offset_of(Preset_List_C, files))},
	{"negs_preset_list_t.count", i64(offset_of(Preset_List_C, count))},
}
