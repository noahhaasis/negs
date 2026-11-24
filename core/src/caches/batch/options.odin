// The batch dialog's choices (`Options`), as the journal keeps them, and
// the file names they give.
package batch

import "core:strings"
import facts "../../rules/facts"

Options :: struct {
	format:       string,
	engine:       string,
	edge:         string,
	pixels:       u32,
	bit_depth:    u32,
	color_space:  u64,
	jpeg_quality: u32,
	naming:       Naming,
	custom:       string,
	existing:     Existing,
	low_memory:   bool,
	// Handed to Lightroom when the batch ends (ticket 48): its key.
	lightroom:    Maybe(string),
}

DEFAULT_CUSTOM :: "{name}_{film}"

// `Options::default()`. Its strings are static (borrowed).
DEFAULT_OPTIONS :: Options {
	format       = "jpeg",
	engine       = "reference",
	edge         = "full",
	pixels       = FULL_PIXELS,
	bit_depth    = 8,
	color_space  = 0,
	jpeg_quality = 95,
	naming       = .Name_Film,
	custom       = DEFAULT_CUSTOM,
	existing     = .Number,
}

options_clone :: proc(o: Options, allocator := context.allocator) -> Options {
	out := o
	out.format = strings.clone(o.format, allocator)
	out.engine = strings.clone(o.engine, allocator)
	out.edge = strings.clone(o.edge, allocator)
	out.custom = strings.clone(o.custom, allocator)
	if l, ok := o.lightroom.?; ok {
		out.lightroom = strings.clone(l, allocator)
	}
	return out
}

options_destroy :: proc(o: Options, allocator := context.allocator) {
	delete(o.format, allocator)
	delete(o.engine, allocator)
	delete(o.edge, allocator)
	delete(o.custom, allocator)
	if l, ok := o.lightroom.?; ok {
		delete(l, allocator)
	}
}

// `Options::save`: a JPEG is always 8 bits.
options_save :: proc(o: Options) -> Save {
	bit_depth := u32(8) if format_from_key(o.format) == .Jpeg else o.bit_depth
	return Save{bit_depth, effective_space(o.color_space, bit_depth), o.jpeg_quality}
}

// char::is_control: the C0 and C1 controls.
@(private = "file")
is_control :: proc "contextless" (r: rune) -> bool {
	return r <= 0x1F || (r >= 0x7F && r <= 0x9F)
}

// `Options::file_name`: the file name for a photograph named `name`
// (without its extension, a duplicate's number included) developed with
// `film`, with the format's extension. A file name, never a path, and
// nothing Windows refuses in one.
file_name :: proc(naming: Naming, custom: string, name: string, film: string, format: Output_Format, allocator := context.allocator) -> string {
	template: string
	switch naming {
	case .Name_Film:
		template = "{name}_{film}"
	case .Name:
		template = "{name}"
	case .Custom:
		template = custom if custom != "" else "{name}"
	}
	named, _ := strings.replace_all(template, "{name}", name, context.temp_allocator)
	filmed, _ := strings.replace_all(named, "{film}", film, context.temp_allocator)
	b := strings.builder_make(0, len(filmed), context.temp_allocator)
	for r in filmed {
		switch r {
		case '/', '\\', ':', '*', '?', '"', '<', '>', '|':
			strings.write_byte(&b, '-')
		case:
			if is_control(r) {
				strings.write_byte(&b, '-')
			} else {
				strings.write_rune(&b, r)
			}
		}
	}
	base := strings.to_string(b)
	// trim_end_matches(['.', ' ']).
	end := len(base)
	for end > 0 && (base[end - 1] == '.' || base[end - 1] == ' ') {
		end -= 1
	}
	base = base[:end]
	ext := FORMAT_EXTENSIONS[format]
	if facts.trim(base) == "" || strings.has_prefix(base, ".") {
		return strings.concatenate({name, base, ".", ext}, allocator)
	}
	return strings.concatenate({base, ".", ext}, allocator)
}

// `Options::file_name` of these options.
options_file_name :: proc(o: Options, name: string, film: string, allocator := context.allocator) -> string {
	return file_name(o.naming, o.custom, name, film, format_from_key(o.format), allocator)
}
