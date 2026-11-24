// The inspector's controls (state.rs's `Adjustment`, `AdjustmentGroup`,
// `ADJUSTMENT_GROUPS` and the loose controls; `adjustmentGroups` and the
// Prep tables in `Inspector.swift`): the same groups, paths, ranges and
// steps. Static.
package state

Adjustment_Kind :: enum u32 {
	Slider,
	Toggle,
	Choice,
	// Solves the enlarger's M/Y filter shifts so a mid-grey prints neutral.
	Balance_Print,
	// Enlarger › Shifts move: a preference, not an edit.
	Filter_Link,
}

// Laid out as negs_adjustment_t. `path` is empty for Balance Print and the
// filter link; `help` only for those two; `title` empty for the link.
Adjustment :: struct {
	kind:        Adjustment_Kind,
	title:       cstring,
	path:        cstring,
	help:        cstring,
	low:         f64,
	high:        f64,
	step:        f64,
	options:     [^]cstring,
	options_len: uint,
}

Section :: enum u32 {
	Camera,
	Film,
	Print,
	Scan,
}

Adjustment_Group :: struct {
	title:    cstring,
	section:  Section,
	// Switches the whole effect on and off from the group's header; "" none.
	enable:   cstring,
	// How many of the leading controls the Lab shows without Show all controls.
	basic:    int,
	controls: []Adjustment,
}

@(private)
slider :: proc "contextless" (title, path: cstring, low, high, step: f64) -> Adjustment {
	return Adjustment{kind = .Slider, title = title, path = path, help = "", low = low, high = high, step = step}
}

@(private)
toggle :: proc "contextless" (title, path: cstring) -> Adjustment {
	return Adjustment{kind = .Toggle, title = title, path = path, help = ""}
}

@(private)
choice :: proc "contextless" (title, path: cstring, options: []cstring) -> Adjustment {
	return Adjustment{kind = .Choice, title = title, path = path, help = "", options = raw_data(options), options_len = uint(len(options))}
}

adjustment_path :: proc "contextless" (a: Adjustment) -> string {
	return string(a.path)
}

adjustment_options :: proc "contextless" (a: Adjustment) -> []cstring {
	return a.options[:a.options_len] if a.options != nil else nil
}

@(private, rodata)
DIFFUSION_FAMILIES := [?]cstring{"black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"}
@(private, rodata)
OUTPUT_GAMUTS := [?]cstring{"off", "oklch", "oklrab", "cam16ucs", "aces_rgc"}
@(private, rodata)
INPUT_GAMUTS := [?]cstring{"off", "xy"}
@(private, rodata)
UPSAMPLINGS := [?]cstring{"hanatos2025", "arctic2026beta04", "mallett2019"}

FILTER_LINK_HELP :: "How the magenta and yellow shifts move. Together moves both the same way: more of both makes the print more cyan, less of both redder. Opposite ways moves one up as the other goes down. Linked, resetting either resets both. Each keeps its own value, so presets are unchanged."
BALANCE_PRINT_HELP :: "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base."

ADJUSTMENT_GROUPS: []Adjustment_Group
@(private)
GROUPS_STORE: [12]Adjustment_Group
@(private)
CONTROLS_STORE: [12][15]Adjustment

// The tables are built once (the first call into the core runs it, see
// abi/context.odin's runtime start), from procedures, so they stay one
// readable list.
@(init, private)
build_groups :: proc "contextless" () {
	add :: proc "contextless" (i: int, title: cstring, section: Section, enable: cstring, basic: int, controls: ..Adjustment) {
		for c, j in controls {
			CONTROLS_STORE[i][j] = c
		}
		GROUPS_STORE[i] = Adjustment_Group{title, section, enable, basic, CONTROLS_STORE[i][:len(controls)]}
	}
	add(0, "Lens Diffusion", .Camera, "camera.diffusion_filter.active", 2,
		choice("Filter", "camera.diffusion_filter.filter_family", DIFFUSION_FAMILIES[:]),
		slider("Strength", "camera.diffusion_filter.strength", 0.0, 2.0, 0.01),
		slider("Spatial scale", "camera.diffusion_filter.spatial_scale", 0.1, 3.0, 0.01),
		slider("Halo warmth", "camera.diffusion_filter.halo_warmth", -1.5, 1.5, 0.01),
		slider("Core intensity", "camera.diffusion_filter.core_intensity", 0.0, 2.0, 0.01),
		slider("Halo intensity", "camera.diffusion_filter.halo_intensity", 0.0, 2.0, 0.01),
		slider("Bloom intensity", "camera.diffusion_filter.bloom_intensity", 0.0, 2.0, 0.01),
		slider("Halo size", "camera.diffusion_filter.halo_size", 0.1, 3.0, 0.01),
		slider("Bloom size", "camera.diffusion_filter.bloom_size", 0.1, 3.0, 0.01))
	add(1, "Film Curves", .Film, "film_render.density_curves_morph.active", 1,
		slider("Gamma", "film_render.density_curves_morph.gamma_factor", 0.5, 2.0, 0.01),
		slider("Fast gamma", "film_render.density_curves_morph.gamma_factor_fast", 0.5, 2.0, 0.01),
		slider("Slow gamma", "film_render.density_curves_morph.gamma_factor_slow", 0.5, 2.0, 0.01),
		slider("Red gamma", "film_render.density_curves_morph.gamma_factor_red", 0.5, 2.0, 0.01),
		slider("Green gamma", "film_render.density_curves_morph.gamma_factor_green", 0.5, 2.0, 0.01),
		slider("Blue gamma", "film_render.density_curves_morph.gamma_factor_blue", 0.5, 2.0, 0.01),
		slider("Developer exhaustion", "film_render.density_curves_morph.developer_exhaustion", 0.0, 1.0, 0.01))
	add(2, "Grain", .Film, "film_render.grain.active", 3,
		slider("Red granularity", "film_render.grain.rms_granularity.0", 0.0, 80.0, 0.1),
		slider("Green granularity", "film_render.grain.rms_granularity.1", 0.0, 80.0, 0.1),
		slider("Blue granularity", "film_render.grain.rms_granularity.2", 0.0, 80.0, 0.1),
		slider("Red uniformity", "film_render.grain.uniformity.0", 0.0, 1.0, 0.01),
		slider("Green uniformity", "film_render.grain.uniformity.1", 0.0, 1.0, 0.01),
		slider("Blue uniformity", "film_render.grain.uniformity.2", 0.0, 1.0, 0.01),
		slider("Coarse grain size", "film_render.grain.particle_scale_sublayers.0", 0.05, 2.0, 0.01),
		slider("Medium grain size", "film_render.grain.particle_scale_sublayers.1", 0.05, 2.0, 0.01),
		slider("Fine grain size", "film_render.grain.particle_scale_sublayers.2", 0.05, 2.0, 0.01),
		slider("Dye clouds (µm)", "film_render.grain.blur_dye_clouds_um", 0.0, 5.0, 0.01),
		slider("Blur (px)", "film_render.grain.blur", 0.0, 3.0, 0.01),
		slider("Sharpen amount", "film_render.grain.mult_usm_amount", 0.0, 3.0, 0.01),
		slider("Sharpen radius (px)", "film_render.grain.mult_usm_sigma", 0.0, 3.0, 0.01),
		slider("Clump blur (µm)", "film_render.grain.micro_structure.0", 0.0, 1.0, 0.01),
		slider("Clump size (nm)", "film_render.grain.micro_structure.1", 0.0, 100.0, 1.0))
	add(3, "Halation", .Film, "film_render.halation.active", 1,
		slider("Amount", "film_render.halation.halation_amount", 0.0, 3.0, 0.01),
		slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1, 5.0, 0.01),
		slider("Scatter amount", "film_render.halation.scatter_amount", 0.0, 3.0, 0.01),
		slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1, 5.0, 0.01),
		slider("Bounces", "film_render.halation.halation_n_bounces", 1.0, 5.0, 1.0),
		slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0.0, 1.0, 0.01))
	add(4, "DIR Couplers", .Film, "film_render.dir_couplers.active", 1,
		slider("Amount", "film_render.dir_couplers.amount", 0.0, 2.0, 0.01),
		slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0.0, 100.0, 0.1),
		slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0.0, 400.0, 1.0),
		slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0.0, 1.0, 0.01))
	add(5, "Film Base", .Film, "film_render.base.active", 1,
		slider("Density", "film_render.base.scale", 0.0, 2.0, 0.01),
		slider("Tilt", "film_render.base.tilt", -1.0, 1.0, 0.01),
		slider("Cyan", "film_render.base.cyan", 0.0, 2.0, 0.01),
		slider("Magenta", "film_render.base.magenta", 0.0, 2.0, 0.01),
		slider("Yellow", "film_render.base.yellow", 0.0, 2.0, 0.01))
	add(6, "Enlarger", .Print, "", 5,
		slider("Print exposure", "enlarger.print_exposure", 0.1, 5.0, 0.01),
		slider("Magenta shift", "enlarger.m_filter_shift", -50.0, 50.0, 0.1),
		slider("Yellow shift", "enlarger.y_filter_shift", -50.0, 50.0, 0.1),
		Adjustment{kind = .Filter_Link, title = "", path = "", help = FILTER_LINK_HELP},
		Adjustment{kind = .Balance_Print, title = "Balance Print", path = "", help = BALANCE_PRINT_HELP},
		toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
		slider("Preflash exposure", "enlarger.preflash_exposure", 0.0, 0.5, 0.01),
		slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50.0, 50.0, 0.1),
		slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50.0, 50.0, 0.1))
	add(7, "Print Base", .Print, "print_render.base.active", 1,
		slider("Density", "print_render.base.scale", 0.0, 2.0, 0.01),
		slider("Cyan", "print_render.base.cyan", 0.0, 2.0, 0.01),
		slider("Magenta", "print_render.base.magenta", 0.0, 2.0, 0.01),
		slider("Yellow", "print_render.base.yellow", 0.0, 2.0, 0.01))
	add(8, "Print Curves", .Print, "print_render.density_curves_morph.active", 1,
		slider("Gamma", "print_render.density_curves_morph.gamma_factor", 0.5, 2.0, 0.01),
		slider("Fast gamma", "print_render.density_curves_morph.gamma_factor_fast", 0.5, 2.0, 0.01),
		slider("Slow gamma", "print_render.density_curves_morph.gamma_factor_slow", 0.5, 2.0, 0.01),
		slider("Red gamma", "print_render.density_curves_morph.gamma_factor_red", 0.5, 2.0, 0.01),
		slider("Green gamma", "print_render.density_curves_morph.gamma_factor_green", 0.5, 2.0, 0.01),
		slider("Blue gamma", "print_render.density_curves_morph.gamma_factor_blue", 0.5, 2.0, 0.01),
		slider("Developer exhaustion", "print_render.density_curves_morph.developer_exhaustion", 0.0, 1.0, 0.01))
	add(9, "Glare", .Print, "print_render.glare.active", 1,
		slider("Amount", "print_render.glare.percent", 0.0, 0.2, 0.001),
		slider("Roughness", "print_render.glare.roughness", 0.0, 2.0, 0.01),
		slider("Blur (px)", "print_render.glare.blur", 0.0, 5.0, 0.01))
	add(10, "Scanner", .Scan, "", 0,
		slider("Lens blur (px)", "scanner.lens_blur", 0.0, 5.0, 0.01),
		slider("Color noise reduction", "scanner.chroma_noise_reduction", 0.0, 100.0, 1.0),
		slider("Unsharp radius (px)", "scanner.unsharp_mask.0", 0.0, 3.0, 0.01),
		slider("Unsharp amount", "scanner.unsharp_mask.1", 0.0, 2.0, 0.01),
		toggle("White correction", "scanner.white_correction"),
		slider("White level", "scanner.white_level", 0.5, 1.0, 0.01),
		toggle("Black correction", "scanner.black_correction"),
		slider("Black level", "scanner.black_level", 0.0, 0.5, 0.01))
	add(11, "Color Management", .Scan, "", 0,
		choice("Output gamut", "io.output_gamut_compress.algorithm", OUTPUT_GAMUTS[:]),
		choice("Input gamut", "io.input_gamut_compress.algorithm", INPUT_GAMUTS[:]),
		toggle("CAT16 adaptation", "settings.use_cat16"),
		choice("Spectral upsampling", "settings.rgb_to_raw_method", UPSAMPLINGS[:]))
	ADJUSTMENT_GROUPS = GROUPS_STORE[:]

	PREP_TONE_STORE = {
		slider("Highlights", "prep.highlights", -180.0, 100.0, 1.0),
		slider("Shadows", "prep.shadows", -100.0, 100.0, 1.0),
		slider("Contrast", "prep.contrast", -100.0, 100.0, 1.0),
		slider("Lift", "prep.lift", -100.0, 100.0, 1.0),
		slider("Gain", "prep.gain", -100.0, 100.0, 1.0),
	}
	PREP_COLOR_STORE = {slider("Color Boost", "prep.color_boost", -100.0, 100.0, 1.0), slider("Saturation", "prep.saturation", -100.0, 100.0, 1.0)}
	PREP_DETAIL_STORE = {slider("Midtone Detail", "prep.midtone_detail", -100.0, 100.0, 1.0)}
	PREP_TONE = PREP_TONE_STORE[:]
	PREP_COLOR = PREP_COLOR_STORE[:]
	PREP_DETAIL = PREP_DETAIL_STORE[:]
	NEGATIVE_EXPOSURE = slider("Negative exposure (EV)", "camera.exposure_compensation_ev", -5.0, 5.0, 0.1)
	LENS_BLUR = slider("Lens blur (µm)", "camera.lens_blur_um", 0.0, 100.0, 0.1)
	WB_TEMPERATURE = slider("Temperature (K)", "io.raw_white_balance.temperature", 1667.0, 25000.0, 1.0)
	WB_TINT = slider("Tint", "io.raw_white_balance.tint", 0.01, 4.0, 0.01)
	FILM_FORMAT = slider("Long edge (mm)", "camera.film_format_mm", 4.0, 120.0, 0.1)
}

// Prep's controls, applied to the scene before the film.
PREP_TONE: []Adjustment
PREP_COLOR: []Adjustment
PREP_DETAIL: []Adjustment
@(private)
PREP_TONE_STORE: [5]Adjustment
@(private)
PREP_COLOR_STORE: [2]Adjustment
@(private)
PREP_DETAIL_STORE: [1]Adjustment

NEGATIVE_EXPOSURE: Adjustment
LENS_BLUR: Adjustment
WB_TEMPERATURE: Adjustment
WB_TINT: Adjustment
// Lab › Camera › Film format's Custom… slider.
FILM_FORMAT: Adjustment

// The engine keeps Prep's exposure within ±3 EV; the slider moves in 0.05 EV.
PREP_EXPOSURE_LOW :: -3.0
PREP_EXPOSURE_HIGH :: 3.0
PREP_EXPOSURE_STEP :: 0.05

@(rodata)
METERING := [?]cstring{"average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"}
// How a non-RAW photograph's values are read (`io.input_interpretation`).
@(rodata)
INPUT_MODES := [?]cstring{"auto", "assume", "manual"}
// The colour spaces Assume offers, as spektrafilm-io::colour::ASSUMABLE.
@(rodata)
ASSUMED_SPACES := [?]cstring{"sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020"}
// Manual: the pipeline's own input colour space, as Negs 0.8 used it.
@(rodata)
MANUAL_SPACES := [?]cstring{"sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020", "ACES2065-1", "ACEScg"}
@(rodata)
WHITE_BALANCE := [?]cstring{"as_shot", "daylight", "tungsten", "custom"}

// Grain arrays that stay null until edited, so the film's preset applies.
@(rodata)
GRAIN_STOCK_FIELDS := [?]cstring{"rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"}

// The DIR gammas and halation baseline, null until set so the film's
// presets apply.
@(rodata)
DIR_HALATION_STOCK_PATHS := [?]cstring {
	"film_render.dir_couplers.gamma_samelayer_rgb",
	"film_render.dir_couplers.gamma_interlayer_r_to_gb",
	"film_render.dir_couplers.gamma_interlayer_g_to_rb",
	"film_render.dir_couplers.gamma_interlayer_b_to_rg",
	"film_render.halation.halation_strength",
	"film_render.halation.halation_first_sigma_um",
}

// Stills formats by the long edge of the image area, in mm.
Film_Format :: struct {
	name: cstring,
	mm:   f64,
}

@(rodata)
FILM_FORMATS := [?]Film_Format {
	{"110", 17.0},
	{"Half frame", 24.0},
	{"35 mm (135)", 35.0},
	{"6×4.5 / 6×6", 56.0},
	{"6×7", 70.0},
	{"6×9", 84.0},
	{"4×5", 120.0},
}

// The format with this long edge; -1 reads as Custom.
film_format :: proc "contextless" (mm: f64) -> int {
	for f, i in FILM_FORMATS {
		if abs(f.mm - mm) < 0.05 {
			return i
		}
	}
	return -1
}

// MARK: Grain lanes

// A slider on one lane of a grain preset array:
// `film_render.grain.<field>.<index>`: the field's place in
// GRAIN_STOCK_FIELDS and the lane, or ok false.
grain_lane :: proc "contextless" (path: string) -> (field: int, lane: u64, ok: bool) {
	PREFIX :: "film_render.grain."
	if len(path) < len(PREFIX) || path[:len(PREFIX)] != PREFIX {
		return
	}
	rest := path[len(PREFIX):]
	dot := -1
	for i in 0 ..< len(rest) {
		if rest[i] == '.' {
			dot = i
			break
		}
	}
	if dot < 0 {
		return
	}
	name := rest[:dot]
	field = -1
	for f, i in GRAIN_STOCK_FIELDS {
		if string(f) == name {
			field = i
		}
	}
	if field < 0 {
		return
	}
	lane, ok = parse_usize(rest[dot + 1:])
	return
}

// `str::parse::<usize>()` on 64 bits: ASCII digits after an optional '+'.
parse_usize :: proc "contextless" (text: string) -> (u64, bool) {
	s := text
	if len(s) > 0 && s[0] == '+' {
		s = s[1:]
	}
	if len(s) == 0 {
		return 0, false
	}
	n: u64 = 0
	for i in 0 ..< len(s) {
		c := s[i]
		if c < '0' || c > '9' {
			return 0, false
		}
		d := u64(c - '0')
		if n > (max(u64) - d) / 10 {
			return 0, false
		}
		n = n * 10 + d
	}
	return n, true
}

// B&W films have one emulsion: per-channel lanes past the first are hidden.
color_only :: proc "contextless" (path: string) -> bool {
	field, lane, ok := grain_lane(path)
	return ok && string(GRAIN_STOCK_FIELDS[field]) != "particle_scale_sublayers" && lane > 0
}

// The first lane's title on a B&W film, without its colour; "" for none.
bw_title :: proc "contextless" (path: string) -> cstring {
	switch path {
	case "film_render.grain.rms_granularity.0":
		return "Granularity"
	case "film_render.grain.uniformity.0":
		return "Uniformity"
	}
	return nil
}
