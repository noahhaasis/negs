// The inspector's controls, on its Prep, Lab and Output panes: RuntimeParams
// paths with desktop's titles, ranges and steps (macOS Inspector.swift
// `adjustmentGroups`, the Camera section and the Prep tab; Linux fixtures.rs
// agrees). Defaults come from the engine's
// catalog, never from here. Print Diffusion follows docs/print-diffusion-plan.md
// on main, which the desktop apps haven't shipped yet.

const slider = (label, path, min, max, step) => ({ kind: "slider", label, path, min, max, step });
const toggle = (label, path) => ({ kind: "toggle", label, path });
const choice = (label, path, options) => ({ kind: "choice", label, path, options });
// A command rather than a value: app.js runs `action`.
const action = (label, action, help) => ({ kind: "action", label, action, help });
// A border width (macOS borderSlider): a fraction of the photograph's long
// edge, shown and typed in percent.
const borderSlider = (label, border, min, max, step, styles, help) => ({ kind: "slider", label, border, percent: true, min, max, step, styles, help });
// One lane of a grain array that stays null until edited, so the film's
// preset from the catalog applies. B&W films have one emulsion: they show
// the lane under `bw`, or hide it when that is null.
const stock = (label, path, index, min, max, step, bw) => ({ kind: "stock", label, path, index, min, max, step, bw });

const METERING = ["average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"];
export const RAW_WB_MODES = ["as_shot", "daylight", "tungsten", "custom"];
const DIFFUSION_FILTERS = ["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"];
const curves = (at) => [
  slider("Gamma", `${at}.gamma_factor`, 0.5, 2, 0.01),
  slider("Fast gamma", `${at}.gamma_factor_fast`, 0.5, 2, 0.01),
  slider("Slow gamma", `${at}.gamma_factor_slow`, 0.5, 2, 0.01),
  slider("Red gamma", `${at}.gamma_factor_red`, 0.5, 2, 0.01),
  slider("Green gamma", `${at}.gamma_factor_green`, 0.5, 2, 0.01),
  slider("Blue gamma", `${at}.gamma_factor_blue`, 0.5, 2, 0.01),
  slider("Developer exhaustion", `${at}.developer_exhaustion`, 0, 1, 0.01),
];

// Sections hold loose controls (`controls`, each shown while its `when`
// path is true, or `[path, value]` has that value) and then their effect
// groups, if any; a `raw` group shows for camera RAWs only. A group's `enable` is the
// switch in its header; `print` sections are skipped while scanning film.
// Without Advanced controls (Settings) the Lab shows a group's first `basic`
// controls and the loose ones not marked `advanced`, plus any that's off its
// default (macOS AdjustmentGroup.basic); a group with no basics only shows
// once something in it changed.
// `tab` names a section's tab on phones when its title is too long.
// The Prep pane (macOS Inspector `prep`): the photograph before the film.
// A section's `raw` shows it for camera RAWs only. Brightness holds Match
// camera brightness (app.js `match`), Exposure as an offset on the match
// (`kind: "offset"`), the match's note, and Auto exposure, shown while the
// match is off or it's changed (`kind: "metering"` rows).
export const PREP_SECTIONS = [
  // A camera RAW's input white balance, which develops it again from its
  // bytes (app.js). Custom's fields only while it's chosen.
  { title: "RAW White Balance", tab: "Prep", raw: true, controls: [
    choice("White balance", "io.raw_white_balance.mode", RAW_WB_MODES),
    { ...slider("Temperature (K)", "io.raw_white_balance.temperature", 1667, 25000, 1), when: ["io.raw_white_balance.mode", "custom"] },
    { ...slider("Tint", "io.raw_white_balance.tint", 0.01, 4, 0.01), when: ["io.raw_white_balance.mode", "custom"] },
  ] },
  { title: "Brightness", tab: "Prep", controls: [
    { kind: "match", label: "Match camera brightness", tutorial: "match",
      help: "Develops the RAW about as bright as the camera’s own preview, and keeps it there when you change film, paper or a preset. Exposure adds to it." },
    { kind: "offset", label: "Exposure (EV)", path: "prep.exposure_ev", min: -3, max: 3, step: 0.05 },
    { kind: "note", label: "Camera brightness" },
    { ...toggle("Auto exposure", "camera.auto_exposure"), metering: true,
      help: "Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it." },
    { ...choice("Metering", "camera.auto_exposure_method", METERING), when: "camera.auto_exposure", metering: true },
  ] },
  { title: "Tone", tab: "Prep", controls: [
    slider("Highlights", "prep.highlights", -180, 100, 1),
    slider("Shadows", "prep.shadows", -100, 100, 1),
    slider("Contrast", "prep.contrast", -100, 100, 1),
    slider("Lift", "prep.lift", -100, 100, 1),
    slider("Gain", "prep.gain", -100, 100, 1),
  ] },
  { title: "Color", tab: "Prep", controls: [
    slider("Color Boost", "prep.color_boost", -100, 100, 1),
    slider("Saturation", "prep.saturation", -100, 100, 1),
  ] },
  { title: "Detail", tab: "Prep", controls: [
    slider("Midtone Detail", "prep.midtone_detail", -100, 100, 1),
  ] },
];

export const SECTIONS = [
  { title: "Camera", controls: [
    slider("Negative exposure (EV)", "camera.exposure_compensation_ev", -5, 5, 0.1),
    slider("Film format (mm)", "camera.film_format_mm", 4, 120, 0.1),
    { ...slider("Lens blur (µm)", "camera.lens_blur_um", 0, 100, 0.1), advanced: true },
  ], groups: [
    { title: "Lens Diffusion", basic: 2, enable: "camera.diffusion_filter.active", controls: [
      choice("Filter", "camera.diffusion_filter.filter_family", DIFFUSION_FILTERS),
      slider("Strength", "camera.diffusion_filter.strength", 0, 2, 0.01),
      slider("Spatial scale", "camera.diffusion_filter.spatial_scale", 0.1, 3, 0.01),
      slider("Halo warmth", "camera.diffusion_filter.halo_warmth", -1.5, 1.5, 0.01),
      slider("Core intensity", "camera.diffusion_filter.core_intensity", 0, 2, 0.01),
      slider("Halo intensity", "camera.diffusion_filter.halo_intensity", 0, 2, 0.01),
      slider("Bloom intensity", "camera.diffusion_filter.bloom_intensity", 0, 2, 0.01),
      slider("Halo size", "camera.diffusion_filter.halo_size", 0.1, 3, 0.01),
      slider("Bloom size", "camera.diffusion_filter.bloom_size", 0.1, 3, 0.01),
    ] },
  ] },
  { title: "Film", groups: [
    { title: "Film Curves", basic: 1, enable: "film_render.density_curves_morph.active", controls: curves("film_render.density_curves_morph") },
    { title: "Grain", basic: 3, enable: "film_render.grain.active", controls: [
      stock("Red granularity", "film_render.grain.rms_granularity", 0, 0, 80, 0.1, "Granularity"),
      stock("Green granularity", "film_render.grain.rms_granularity", 1, 0, 80, 0.1, null),
      stock("Blue granularity", "film_render.grain.rms_granularity", 2, 0, 80, 0.1, null),
      stock("Red uniformity", "film_render.grain.uniformity", 0, 0, 1, 0.01, "Uniformity"),
      stock("Green uniformity", "film_render.grain.uniformity", 1, 0, 1, 0.01, null),
      stock("Blue uniformity", "film_render.grain.uniformity", 2, 0, 1, 0.01, null),
      stock("Coarse grain size", "film_render.grain.particle_scale_sublayers", 0, 0.05, 2, 0.01, "Coarse grain size"),
      stock("Medium grain size", "film_render.grain.particle_scale_sublayers", 1, 0.05, 2, 0.01, "Medium grain size"),
      stock("Fine grain size", "film_render.grain.particle_scale_sublayers", 2, 0.05, 2, 0.01, "Fine grain size"),
      slider("Dye clouds (µm)", "film_render.grain.blur_dye_clouds_um", 0, 5, 0.01),
      slider("Blur (px)", "film_render.grain.blur", 0, 3, 0.01),
      slider("Sharpen amount", "film_render.grain.mult_usm_amount", 0, 3, 0.01),
      slider("Sharpen radius (px)", "film_render.grain.mult_usm_sigma", 0, 3, 0.01),
      slider("Clump blur (µm)", "film_render.grain.micro_structure.0", 0, 1, 0.01),
      slider("Clump size (nm)", "film_render.grain.micro_structure.1", 0, 100, 1),
    ] },
    { title: "Halation", basic: 1, enable: "film_render.halation.active", controls: [
      slider("Amount", "film_render.halation.halation_amount", 0, 3, 0.01),
      slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1, 5, 0.01),
      slider("Scatter amount", "film_render.halation.scatter_amount", 0, 3, 0.01),
      slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1, 5, 0.01),
      slider("Bounces", "film_render.halation.halation_n_bounces", 1, 5, 1),
      slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0, 1, 0.01),
    ] },
    { title: "DIR Couplers", basic: 1, enable: "film_render.dir_couplers.active", controls: [
      slider("Amount", "film_render.dir_couplers.amount", 0, 2, 0.01),
      slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0, 100, 0.1),
      slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0, 400, 1),
      slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0, 1, 0.01),
    ] },
    { title: "Film Base", basic: 1, enable: "film_render.base.active", controls: [
      slider("Density", "film_render.base.scale", 0, 2, 0.01),
      slider("Tilt", "film_render.base.tilt", -1, 1, 0.01),
      slider("Cyan", "film_render.base.cyan", 0, 2, 0.01),
      slider("Magenta", "film_render.base.magenta", 0, 2, 0.01),
      slider("Yellow", "film_render.base.yellow", 0, 2, 0.01),
    ] },
  ] },
  { title: "Print", print: true, footer: "Printing is skipped while scanning film directly.", groups: [
    { title: "Enlarger", open: true, basic: 4, controls: [
      slider("Print exposure", "enlarger.print_exposure", 0.1, 5, 0.01),
      slider("Magenta shift", "enlarger.m_filter_shift", -50, 50, 0.1),
      slider("Yellow shift", "enlarger.y_filter_shift", -50, 50, 0.1),
      action("Balance Print", "balancePrint", "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base."),
      toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
      slider("Preflash exposure", "enlarger.preflash_exposure", 0, 0.5, 0.01),
      slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50, 50, 0.1),
      slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50, 50, 0.1),
    ] },
    { title: "Print Diffusion", basic: 2, enable: "enlarger.diffusion_filter.active", controls: [
      choice("Filter", "enlarger.diffusion_filter.filter_family", DIFFUSION_FILTERS),
      slider("Strength", "enlarger.diffusion_filter.strength", 0, 2, 0.01),
      slider("Spatial scale", "enlarger.diffusion_filter.spatial_scale", 0.1, 3, 0.01),
      slider("Halo warmth", "enlarger.diffusion_filter.halo_warmth", -1.5, 1.5, 0.01),
      slider("Core intensity", "enlarger.diffusion_filter.core_intensity", 0, 4, 0.05),
      slider("Core size", "enlarger.diffusion_filter.core_size", 0.1, 4, 0.05),
      slider("Halo intensity", "enlarger.diffusion_filter.halo_intensity", 0, 4, 0.05),
      slider("Halo size", "enlarger.diffusion_filter.halo_size", 0.1, 4, 0.05),
      slider("Bloom intensity", "enlarger.diffusion_filter.bloom_intensity", 0, 4, 0.05),
      slider("Bloom size", "enlarger.diffusion_filter.bloom_size", 0.1, 4, 0.05),
    ] },
    { title: "Print Curves", basic: 1, enable: "print_render.density_curves_morph.active", controls: curves("print_render.density_curves_morph") },
    { title: "Print Base", basic: 1, enable: "print_render.base.active", controls: [
      slider("Density", "print_render.base.scale", 0, 2, 0.01),
      slider("Cyan", "print_render.base.cyan", 0, 2, 0.01),
      slider("Magenta", "print_render.base.magenta", 0, 2, 0.01),
      slider("Yellow", "print_render.base.yellow", 0, 2, 0.01),
    ] },
    { title: "Glare", basic: 1, enable: "print_render.glare.active", controls: [
      slider("Amount", "print_render.glare.percent", 0, 0.2, 0.001),
      slider("Roughness", "print_render.glare.roughness", 0, 2, 0.01),
      slider("Blur (px)", "print_render.glare.blur", 0, 5, 0.01),
    ] },
  ] },
  { title: "Scan & Color", tab: "Scan", groups: [
    { title: "Scanner", controls: [
      slider("Lens blur (px)", "scanner.lens_blur", 0, 5, 0.01),
      slider("Unsharp radius (px)", "scanner.unsharp_mask.0", 0, 3, 0.01),
      slider("Unsharp amount", "scanner.unsharp_mask.1", 0, 2, 0.01),
      toggle("White correction", "scanner.white_correction"),
      slider("White level", "scanner.white_level", 0.5, 1, 0.01),
      toggle("Black correction", "scanner.black_correction"),
      slider("Black level", "scanner.black_level", 0, 0.5, 0.01),
    ] },
    // macOS Inspector's Color Management, less CAT16 (not a preset setting).
    // Only the algorithms: their other settings stay as they are.
    { title: "Color Management", controls: [
      choice("Output gamut", "io.output_gamut_compress.algorithm", ["off", "oklch", "oklrab", "cam16ucs", "aces_rgc"]),
      choice("Input gamut", "io.input_gamut_compress.algorithm", ["off", "xy"]),
      choice("Spectral upsampling", "settings.rgb_to_raw_method", ["hanatos2025", "arctic2026beta04", "mallett2019"]),
    ] },
  ] },
];

// The Output pane (macOS Inspector `output`): what's added after the film.
// Post's tone and colour went with the Mac's round 2, for Prep's.
export const OUTPUT_SECTIONS = [
  // macOS Inspector `Section("Border")`: the border's settings
  // (web/border.js), outside the edit's params, so Reset Adjustments keeps
  // them. Everything after "Add border" is off while it is; `styles` shows a
  // control for those styles only.
  { title: "Border", tab: "Output", controls: [
    { kind: "toggle", label: "Add border", border: "enabled",
      help: "Frames the photograph in a mat or a film frame. The preview always shows it; Include in export decides whether files get it." },
    { kind: "styles", label: "Style", border: "style",
      help: "Solid is a plain mat; Keyline adds a thin line around the photograph; Film is a scanned 120 film frame; 35mm a negative strip with the film’s edge print." },
    borderSlider("Width", "width", 0, 15, 0.1, ["solid", "keyline"], "The mat’s width, in percent of the photograph’s long edge."),
    borderSlider("Line width", "line", 0.05, 2, 0.05, ["keyline"], "The line’s width, in percent of the photograph’s long edge."),
    { kind: "color", label: "Color", border: "color", styles: ["solid", "keyline"], help: "The mat’s colour." },
    { kind: "color", label: "Line color", border: "lineColor", styles: ["keyline"], help: "The line’s colour." },
    borderSlider("Width", "filmWidth", 0, 15, 0.1, ["film"], "Adds black around the film frame, in percent of the photograph’s long edge."),
    { kind: "toggle", label: "Extend to 3:4 for Instagram", border: "instagram",
      help: "Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post." },
    { kind: "toggle", label: "Include in export", border: "export", help: "Adds the border to exported photographs and saved previews." },
  ] },
];

// Human-readable names for engine option identifiers (macOS optionTitle).
const TITLES = {
  average: "Average", median: "Median", center_weighted: "Center-Weighted", partial: "Partial",
  matrix: "Matrix", multi_zone: "Multi-Zone", highlight_weighted: "Highlight-Weighted",
  as_shot: "As Shot", daylight: "Daylight", tungsten: "Tungsten (2850 K)", custom: "Custom",
  black_pro_mist: "Black Pro-Mist", glimmerglass: "Glimmerglass", pro_mist: "Pro-Mist", cinebloom: "CineBloom",
  off: "Off", oklch: "OKLCh", oklrab: "OKLrab", cam16ucs: "CAM16-UCS", aces_rgc: "ACES RGC", xy: "xy Chromaticity",
  hanatos2025: "Hanatos 2025", arctic2026beta04: "Arctic 2026 (Beta 04)", mallett2019: "Mallett 2019",
};
export const optionTitle = (option) => TITLES[option] ?? option.replaceAll("_", " ").replace(/\b\w/g, (c) => c.toUpperCase());

// A slider's value, snapped to its step and range as desktop stores it
// (macOS EditorModel.snap): integer fields reject fractions.
export function snap(value, { min, max, step }) {
  const clamped = Math.min(Math.max(value, min), max);
  const steps = Math.round((clamped - min) / step);
  const scale = 10 ** Math.max(0, Math.floor(-Math.log10(step)) + 1);
  return Math.min(Math.round((min + steps * step) * scale) / scale, max);
}

// As desktop shows a value: up to the step's decimals, no trailing zeros.
export function formatValue(value, step) {
  const decimals = Math.max(0, Math.floor(-Math.log10(step) + 1e-9));
  return String(Number(Number(value).toFixed(decimals)));
}
