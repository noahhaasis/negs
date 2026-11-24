// Choosing a stock (state.rs's `choosing`, `pair_film`, `PreviewRules`),
// the stock menus' groups (`film_groups`, `paper_groups`) and the film
// search (`matching`).
//
// A choice changes only the film, the paper and a few params, so it is
// worked out as a Choice (what to set) that the core applies to an
// Edit_State, or a host to the state it keeps.
package state

import "core:strings"
import "../value"

Choice_Path :: enum u32 {
	Print_Development,
	Film_Development,
	Grain_Rms_Granularity,
	Grain_Density_Min,
	Grain_Uniformity,
	Grain_Particle_Scale_Sublayers,
	Dir_Samelayer,
	Dir_Interlayer_R,
	Dir_Interlayer_G,
	Dir_Interlayer_B,
	Halation_Strength,
	Halation_First_Sigma,
}

// The params a choice sets to null, by Choice_Path.
@(rodata)
CHOICE_PATHS := [Choice_Path]cstring {
	.Print_Development              = "print_render.development_time",
	.Film_Development               = "film_render.development_time",
	.Grain_Rms_Granularity          = "film_render.grain.rms_granularity",
	.Grain_Density_Min              = "film_render.grain.density_min",
	.Grain_Uniformity               = "film_render.grain.uniformity",
	.Grain_Particle_Scale_Sublayers = "film_render.grain.particle_scale_sublayers",
	.Dir_Samelayer                  = "film_render.dir_couplers.gamma_samelayer_rgb",
	.Dir_Interlayer_R               = "film_render.dir_couplers.gamma_interlayer_r_to_gb",
	.Dir_Interlayer_G               = "film_render.dir_couplers.gamma_interlayer_g_to_rb",
	.Dir_Interlayer_B               = "film_render.dir_couplers.gamma_interlayer_b_to_rg",
	.Halation_Strength              = "film_render.halation.halation_strength",
	.Halation_First_Sigma           = "film_render.halation.halation_first_sigma_um",
}

SCAN_FILM :: "io.scan_film"

// What choosing a stock sets: the film and paper (borrowed from the
// inputs), params to null (distinct leaves, so in any order), and
// `io.scan_film` when `sets_scan`.
Choice :: struct {
	film:      string,
	paper:     string,
	nulls:     bit_set[Choice_Path;u32],
	sets_scan: bool,
	scan:      bool,
}

// `pair_film`: scanning for slides, and the film's own paper unless
// `paper` is false. `c` holds the state's film and paper so far.
pair_plan :: proc(c: ^Choice, stocks: []Stock, paper: bool) {
	film := -1
	for s, i in stocks {
		if !s.paper && s.id == c.film {
			film = i
			break
		}
	}
	if film < 0 {
		return
	}
	f := stocks[film]
	c.sets_scan = true
	c.scan = f.positive
	if !paper || c.paper == f.target {
		return
	}
	for s in stocks {
		if s.paper && s.id == f.target {
			c.paper = f.target
			c.nulls += {.Print_Development}
			return
		}
	}
}

// `choosing`: the edit with stock `id` chosen, exactly as choosing it from
// the menu or a preview tile makes it. A film resets its development time
// and its grain, DIR and halation presets, sets scanning for slides and
// brings its own paper unless `keep_paper`; a paper keeps the film.
choosing_plan :: proc(film, paper, id: string, is_paper, keep_paper: bool, stocks: []Stock) -> Choice {
	c := Choice{film = film, paper = paper}
	if is_paper {
		c.paper = id
		c.nulls = {.Print_Development}
		return c
	}
	c.film = id
	c.nulls = {.Film_Development, .Grain_Rms_Granularity, .Grain_Density_Min, .Grain_Uniformity, .Grain_Particle_Scale_Sublayers,
		.Dir_Samelayer, .Dir_Interlayer_R, .Dir_Interlayer_G, .Dir_Interlayer_B, .Halation_Strength, .Halation_First_Sigma}
	pair_plan(&c, stocks, !keep_paper)
	return c
}

// The choice applied to `s` (in the state's allocator).
apply_choice :: proc(s: ^Edit_State, c: Choice, allocator := context.allocator) {
	if s.film != c.film {
		film := strings.clone(c.film, allocator)
		delete(s.film, allocator)
		s.film = film
	}
	if s.paper != c.paper {
		paper := strings.clone(c.paper, allocator)
		delete(s.paper, allocator)
		s.paper = paper
	}
	for p in Choice_Path {
		if p in c.nulls {
			value.set(&s.params, string(CHOICE_PATHS[p]), nil, allocator)
		}
	}
	if c.sets_scan {
		value.set(&s.params, SCAN_FILM, c.scan, allocator)
	}
}

choosing :: proc(s: ^Edit_State, id: string, is_paper, keep_paper: bool, stocks: []Stock, allocator := context.allocator) {
	apply_choice(s, choosing_plan(s.film, s.paper, id, is_paper, keep_paper, stocks), allocator)
}

pair_film :: proc(s: ^Edit_State, stocks: []Stock, paper: bool, allocator := context.allocator) {
	c := Choice{film = s.film, paper = s.paper}
	pair_plan(&c, stocks, paper)
	apply_choice(s, c, allocator)
}

// MARK: Groups

// The stock menus' sections, in order.
Group_Title :: enum u32 {
	Color_Negative,
	Color_Slide,
	Motion_Picture,
	Black_White,
	Color_Paper,
	Motion_Picture_Print,
}

@(rodata)
GROUP_TITLES := [Group_Title]cstring {
	.Color_Negative       = "Color Negative",
	.Color_Slide          = "Color Slide",
	.Motion_Picture       = "Motion Picture",
	.Black_White          = "Black & White",
	.Color_Paper          = "Color Paper",
	.Motion_Picture_Print = "Motion Picture Print",
}

// One section: its title (static for the menus' groups) and its stocks,
// as places in the list grouped.
Stock_Group :: struct {
	title:  string,
	stocks: [dynamic]int,
}

@(private)
group :: proc(kinds: []Group_Title, order: []Group_Title, allocator := context.allocator) -> [dynamic]Stock_Group {
	out := make([dynamic]Stock_Group, 0, len(order), allocator)
	for title in order {
		g := Stock_Group{string(GROUP_TITLES[title]), make([dynamic]int, 0, 8, allocator)}
		for k, i in kinds {
			if k == title {
				append(&g.stocks, i)
			}
		}
		if len(g.stocks) > 0 {
			append(&out, g)
		} else {
			delete(g.stocks)
		}
	}
	return out
}

film_groups :: proc(films: []Stock, papers: []Stock, allocator := context.allocator) -> [dynamic]Stock_Group {
	kinds := make([]Group_Title, len(films), context.temp_allocator)
	for s, i in films {
		switch {
		case s.bw:
			kinds[i] = .Black_White
		case s.positive:
			kinds[i] = .Color_Slide
		case:
			kinds[i] = .Color_Negative
			for p in papers {
				if is_motion_picture_print(p) && p.id == s.target {
					kinds[i] = .Motion_Picture
					break
				}
			}
		}
	}
	return group(kinds, {.Color_Negative, .Color_Slide, .Motion_Picture, .Black_White}, allocator)
}

paper_groups :: proc(papers: []Stock, allocator := context.allocator) -> [dynamic]Stock_Group {
	kinds := make([]Group_Title, len(papers), context.temp_allocator)
	for s, i in papers {
		switch {
		case s.bw:
			kinds[i] = .Black_White
		case is_motion_picture_print(s):
			kinds[i] = .Motion_Picture_Print
		case:
			kinds[i] = .Color_Paper
		}
	}
	return group(kinds, {.Color_Paper, .Motion_Picture_Print, .Black_White}, allocator)
}

groups_destroy :: proc(groups: [dynamic]Stock_Group) {
	for g in groups {
		delete(g.stocks)
	}
	delete(groups)
}

// MARK: Matching

// Whether `text` contains `word`, ignoring case and diacritics, as the
// host's platform compares (Windows: FindNLSStringEx in the invariant
// locale; the Mac: localizedStandardContains). Registered once with
// negs_text_set_contains; without one, a plain byte search.
Contains_Fn :: #type proc "c" (text: [^]u8, text_len: uint, word: [^]u8, word_len: uint) -> u8

contains_fn: Contains_Fn

contains_folded :: proc(text, word: string) -> bool {
	if contains_fn != nil {
		return contains_fn(raw_data(text), uint(len(text)), raw_data(word), uint(len(word))) != 0
	}
	return strings.contains(text, word)
}

// `matching`: the stocks matching every word of `query` in their name,
// id (its `_` read as spaces) or group title; groups left empty drop out.
// An empty query keeps them all. `stocks` are the groups' stocks.
matching :: proc(query: string, groups: ^[dynamic]Stock_Group, stocks: []Stock) {
	words := split_whitespace(query, context.temp_allocator)
	if len(words) == 0 {
		return
	}
	b := strings.builder_make(0, 128, context.temp_allocator)
	kept := 0
	for &g in groups {
		n := 0
		for i in g.stocks {
			s := stocks[i]
			strings.builder_reset(&b)
			strings.write_string(&b, s.name)
			strings.write_byte(&b, ' ')
			for c in transmute([]u8)s.id {
				strings.write_byte(&b, ' ' if c == '_' else c)
			}
			strings.write_byte(&b, ' ')
			strings.write_string(&b, g.title)
			text := strings.to_string(b)
			all := true
			for w in words {
				if !contains_folded(text, w) {
					all = false
					break
				}
			}
			if all {
				g.stocks[n] = i
				n += 1
			}
		}
		resize(&g.stocks, n)
		if n > 0 {
			groups[kept] = g
			kept += 1
		} else {
			delete(g.stocks)
		}
	}
	resize(groups, kept)
}
