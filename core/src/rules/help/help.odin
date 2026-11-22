// The tooltips of the inspector's controls, keyed by parameter path, and of
// its groups, keyed by title (windows/src/help.rs, frozen in
// core/diff/oracle). A control without an entry has no tooltip.
//
// Two things come from modules ported later, so the host passes them: the
// negative's backlight path (`negative::SCAN_ILLUMINANT`) and the grain
// fields that have one lane per layer (`state::GRAIN_STOCK_FIELDS`).
package help

// A control's tooltip; a grain lane (`film_render.grain.rms_granularity.1`)
// shares its array's. The text is static.
text :: proc(path: string, scan_illuminant: string, grain_fields: []string) -> (string, bool) {
	if path == scan_illuminant {
		return SCAN_ILLUMINANT, true
	}
	if t, ok := lookup(path); ok {
		return t, true
	}
	field, lane := grain_lane(path, grain_fields)
	if !lane {
		return "", false
	}
	// "film_render.grain." + field, without building it: the path's own
	// prefix is that.
	return lookup(path[:len(GRAIN) + len(field)])
}

// A group's tooltip, by its title; static.
group :: proc(title: string) -> (string, bool) {
	for row in GROUPS {
		if row[0] == title {
			return row[1], true
		}
	}
	return "", false
}

@(private)
lookup :: proc(path: string) -> (string, bool) {
	for row in PATHS {
		if row[0] == path {
			return row[1], true
		}
	}
	return "", false
}

@(private)
GRAIN :: "film_render.grain."

// `state::grain_lane`: the field of `film_render.grain.<field>.<index>`
// when it's one of `fields` and the index a usize.
grain_lane :: proc(path: string, fields: []string) -> (field: string, ok: bool) {
	if len(path) < len(GRAIN) || path[:len(GRAIN)] != GRAIN {
		return "", false
	}
	rest := path[len(GRAIN):]
	dot := -1
	for i in 0 ..< len(rest) {
		if rest[i] == '.' {
			dot = i
			break
		}
	}
	if dot < 0 {
		return "", false
	}
	field = rest[:dot]
	known := false
	for f in fields {
		if f == field {
			known = true
			break
		}
	}
	if !known || !parses_as_usize(rest[dot + 1:]) {
		return "", false
	}
	return field, true
}

// `str::parse::<usize>` succeeds: an optional "+", then decimal digits
// that fit 64 bits.
parses_as_usize :: proc "contextless" (s: string) -> bool {
	digits := s
	if len(digits) > 0 && digits[0] == '+' {
		digits = digits[1:]
	}
	if len(digits) == 0 {
		return false
	}
	value: u64
	for c in transmute([]u8)digits {
		if c < '0' || c > '9' {
			return false
		}
		d := u64(c - '0')
		if value > (max(u64) - d) / 10 {
			return false
		}
		value = value * 10 + d
	}
	return true
}
