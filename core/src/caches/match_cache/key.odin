// The names of solves and engines (match_cache.rs's `key` and
// `engine_stamp`).
package match_cache

import "core:strings"
import "../../library"
import "../../value"
import files "../files"

// The file's size and modification time in seconds (`as_secs_f64`); none
// if it can't be read or is from before 1970.
stamp :: proc(path: string) -> (size: u64, modified: f64, ok: bool) {
	st := files.stat(path) or_return
	t := st.modified.? or_return
	return st.size, files.secs_f64(t), true
}

// The text a solve is named by, before its hash:
// `1|{file}|{size}|{modified}|{engine}|{request}`, the request
// `{"film","paper","params"}` as `serde_json::to_string(&json!(…))`
// writes it (keys sorted).
key_text :: proc(file: string, film, paper: string, params: value.Value, engine: string, allocator := context.allocator) -> (text: string, ok: bool) {
	size, modified := stamp(file) or_return
	b := strings.builder_make(0, 256, allocator)
	strings.write_string(&b, "1|")
	strings.write_string(&b, files.display(file, context.temp_allocator))
	strings.write_byte(&b, '|')
	value.write_u64(&b, size)
	strings.write_byte(&b, '|')
	value.write_display(&b, modified)
	strings.write_byte(&b, '|')
	strings.write_string(&b, engine)
	strings.write_string(&b, `|{"film":`)
	value.write_string(&b, film)
	strings.write_string(&b, `,"paper":`)
	value.write_string(&b, paper)
	strings.write_string(&b, `,"params":`)
	value.write_compact(&b, params)
	strings.write_byte(&b, '}')
	return strings.to_string(b), true
}

// `key`: the SHA-256 of key_text in lower-case hex; none if the file
// can't be read (nothing is then looked up or kept).
key :: proc(file: string, film, paper: string, params: value.Value, engine: string, allocator := context.allocator) -> (hex: string, ok: bool) {
	text := key_text(file, film, paper, params, engine, context.temp_allocator) or_return
	digest := library.sha256(transmute([]u8)text)
	HEX := "0123456789abcdef"
	out := make([]u8, 64, allocator)
	for d, i in digest {
		out[2 * i] = HEX[d >> 4]
		out[2 * i + 1] = HEX[d & 15]
	}
	return string(out), true
}

// `engine_stamp`: `{version}|{helper}|{size}|{modified}|{data}`, the
// helper's size u64::MAX and time 0 when it can't be read.
engine_stamp :: proc(version: string, helper: string, data: string, allocator := context.allocator) -> string {
	size, modified, ok := stamp(helper)
	if !ok {
		size, modified = max(u64), 0
	}
	b := strings.builder_make(0, 128, allocator)
	strings.write_string(&b, version)
	strings.write_byte(&b, '|')
	strings.write_string(&b, files.display(helper, context.temp_allocator))
	strings.write_byte(&b, '|')
	value.write_u64(&b, size)
	strings.write_byte(&b, '|')
	value.write_display(&b, modified)
	strings.write_byte(&b, '|')
	strings.write_string(&b, files.display(data, context.temp_allocator))
	return strings.to_string(b)
}
