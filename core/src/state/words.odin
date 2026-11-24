// How the inspector writes values and options (state.rs's option_title,
// snap, decimals, format_value, ratio_name).
package state

import "core:strings"

// Human-readable names for engine option identifiers; nil for an option
// without its own name (option_title then capitalises its words).
option_name :: proc "contextless" (option: string) -> cstring {
	switch option {
	case "average":
		return "Average"
	case "median":
		return "Median"
	case "center_weighted":
		return "Center-Weighted"
	case "partial":
		return "Partial"
	case "matrix":
		return "Matrix"
	case "multi_zone":
		return "Multi-Zone"
	case "highlight_weighted":
		return "Highlight-Weighted"
	case "as_shot":
		return "As Shot"
	case "auto":
		return "Automatic"
	case "daylight":
		return "Daylight"
	case "tungsten":
		return "Tungsten"
	case "D55":
		return "Daylight, 5500 K"
	case "D50":
		return "Light box, 5000 K"
	case "D65":
		return "Cool LED, 6500 K"
	case "custom":
		return "Custom"
	case "black_pro_mist":
		return "Black Pro-Mist"
	case "glimmerglass":
		return "Glimmerglass"
	case "pro_mist":
		return "Pro-Mist"
	case "cinebloom":
		return "CineBloom"
	case "off":
		return "Off"
	case "oklch":
		return "OKLCh"
	case "oklrab":
		return "OKLrab"
	case "cam16ucs":
		return "CAM16-UCS"
	case "aces_rgc":
		return "ACES RGC"
	case "xy":
		return "xy Chromaticity"
	case "hanatos2025":
		return "Hanatos 2025"
	case "arctic2026beta04":
		return "Arctic 2026 (Beta 04)"
	case "mallett2019":
		return "Mallett 2019"
	}
	return nil
}

write_option_title :: proc(b: ^strings.Builder, option: string) {
	if name := option_name(option); name != nil {
		strings.write_string(b, string(name))
		return
	}
	// Each `_`-separated word capitalised, joined by spaces.
	rest := option
	for {
		i := strings.index_byte(rest, '_')
		word := rest if i < 0 else rest[:i]
		write_capitalised(b, word)
		if i < 0 {
			return
		}
		strings.write_byte(b, ' ')
		rest = rest[i + 1:]
	}
}

option_title :: proc(option: string, allocator := context.allocator) -> string {
	b := strings.builder_make(0, len(option) + 8, allocator)
	write_option_title(&b, option)
	return strings.to_string(b)
}

// Sliders move in steps; the engine expects the stepped value (integer
// fields reject fractions). Like `EditorModel.snap`. low <= high.
snap :: proc "contextless" (v, low, high, step: f64) -> f64 {
	clamped := clamp_f64(v, low, high)
	steps := round((clamped - low) / step)
	decimals := max(saturating_i32(floor(-log10(step))) + 1, 0)
	scale := powi(10.0, decimals)
	return fmin(round((low + steps * step) * scale) / scale, high)
}

// Decimal places a value with this step shows: as many as the step has
// (0.05 shows two), at most 6.
decimals :: proc "contextless" (step: f64) -> int {
	for d in 0 ..< 6 {
		scaled := step * powi(10.0, i32(d))
		if abs(scaled - round(scaled)) < 1e-9 {
			return d
		}
	}
	return 6
}

// A value as the inspector shows it: up to the step's decimals, no
// trailing zeros, never "-0".
write_value :: proc(b: ^strings.Builder, v: f64, step: f64) {
	buf: [400]u8
	t := strings.builder_from_bytes(buf[:])
	write_fixed(&t, v, decimals(step))
	text := strings.to_string(t)
	if strings.index_byte(text, '.') >= 0 {
		text = strings.trim_right(text, "0")
		text = strings.trim_right(text, ".")
	}
	strings.write_string(b, "0" if text == "-0" else text)
}

format_value :: proc(v: f64, step: f64, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 16, allocator)
	write_value(&b, v, step)
	return strings.to_string(b)
}

// A photograph's aspect ratio as people name it, such as 16:9.
ratio_name :: proc(width, height: f64, allocator := context.allocator) -> string {
	ratio := fmax(width, height) / fmin(width, height)
	NAMED :: [?]struct {
		r:    f64,
		name: string,
	}{{1.0, "1:1"}, {5.0 / 4.0, "5:4"}, {7.0 / 5.0, "7:5"}, {16.0 / 9.0, "16:9"}, {2.0, "2:1"}, {21.0 / 9.0, "21:9"}}
	for n in NAMED {
		if abs(ratio / n.r - 1.0) <= 0.01 {
			return strings.clone(n.name, allocator)
		}
	}
	b := strings.builder_make(0, 16, allocator)
	write_fixed(&b, ratio, 2)
	strings.write_string(&b, ":1")
	return strings.to_string(b)
}
