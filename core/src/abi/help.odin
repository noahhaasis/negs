// The tooltips' C ABI (negs_core.h, "Phase 3c"): static texts.
package negs_core

import "../rules/help"

// negs_str_t: a borrowed UTF-8 string, (pointer, length).
Help_Str_C :: struct {
	data: [^]u8,
	len:  uint,
}

@(private = "file")
static_out :: proc "contextless" (s: string, ok: bool, out: ^Help_Str_C) -> u8 {
	if !ok {
		return 0
	}
	if out != nil {
		out^ = Help_Str_C{raw_data(s), uint(len(s))}
	}
	return 1
}

@(export, link_name = "negs_help_text")
help_text :: proc "c" (path: [^]u8, path_len: uint, scan_illuminant: [^]u8, scan_len: uint, grain_fields: [^]Help_Str_C, grain_count: uint, out: ^Help_Str_C) -> u8 {
	context = core_context()
	FIELDS_MAX :: 16
	fields: [FIELDS_MAX]string
	n := min(int(grain_count), FIELDS_MAX) if grain_fields != nil else 0
	for i in 0 ..< n {
		fields[i] = host_string(grain_fields[i].data, grain_fields[i].len)
	}
	s, ok := help.text(host_string(path, path_len), host_string(scan_illuminant, scan_len), fields[:n])
	return static_out(s, ok, out)
}

@(export, link_name = "negs_help_group")
help_group :: proc "c" (title: [^]u8, title_len: uint, out: ^Help_Str_C) -> u8 {
	context = core_context()
	s, ok := help.group(host_string(title, title_len))
	return static_out(s, ok, out)
}

// 0 SCAN_ILLUMINANT, 1 FILM, 2 PAPER, 3 PAPER_LOCK, 4 PAPER_KEPT.
@(export, link_name = "negs_help_constant")
help_constant :: proc "c" (which: u32, out: ^Help_Str_C) -> u8 {
	texts := [?]string{help.SCAN_ILLUMINANT, help.FILM, help.PAPER, help.PAPER_LOCK, help.PAPER_KEPT}
	if which >= len(texts) {
		return 0
	}
	return static_out(texts[which], true, out)
}

// The tables, row by row, for the hosts' checks against ControlHelp.swift:
// table 0 paths, 1 groups; 0 past the end.
@(export, link_name = "negs_help_row")
help_row :: proc "c" (table: u32, index: uint, key: ^Help_Str_C, text: ^Help_Str_C) -> u8 {
	rows := help.PATHS[:] if table == 0 else help.GROUPS[:] if table == 1 else nil
	if index >= uint(len(rows)) {
		return 0
	}
	static_out(rows[index][0], true, key)
	return static_out(rows[index][1], true, text)
}
