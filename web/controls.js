// The Lab inspector's controls: RuntimeParams paths with desktop's titles,
// ranges and steps (macOS Inspector.swift `adjustmentGroups` and the Camera
// section; Linux fixtures.rs agrees). Defaults come from the engine's
// catalog, never from here. Print Diffusion follows docs/print-diffusion-plan.md
// on main, which the desktop apps haven't shipped yet.

const slider = (label, path, min, max, step) => ({ kind: "slider", label, path, min, max, step });
const toggle = (label, path) => ({ kind: "toggle", label, path });
const choice = (label, path, options) => ({ kind: "choice", label, path, options });
// One lane of a grain array that stays null until edited, so the film's
// preset from the catalog applies. B&W films have one emulsion: they show
// the lane under `bw`, or hide it when that is null.
const stock = (label, path, index, min, max, step, bw) => ({ kind: "stock", label, path, index, min, max, step, bw });

const METERING = ["average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"];
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
// path is true) and then their effect groups. A group's `enable` is the
// switch in its header; `print` sections are skipped while scanning film.
// `tab` names a section's tab on phones when its title is too long.
export const SECTIONS = [
  { title: "Camera", controls: [
    toggle("Auto exposure", "camera.auto_exposure"),
    { ...choice("Metering", "camera.auto_exposure_method", METERING), when: "camera.auto_exposure" },
    slider("Exposure (EV)", "camera.exposure_compensation_ev", -5, 5, 0.1),
    slider("Film format (mm)", "camera.film_format_mm", 4, 120, 0.1),
    slider("Lens blur (µm)", "camera.lens_blur_um", 0, 100, 0.1),
  ], groups: [
    { title: "Lens Diffusion", enable: "camera.diffusion_filter.active", controls: [
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
    { title: "Film Curves", enable: "film_render.density_curves_morph.active", controls: curves("film_render.density_curves_morph") },
    { title: "Grain", enable: "film_render.grain.active", controls: [
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
    { title: "Halation", enable: "film_render.halation.active", controls: [
      slider("Amount", "film_render.halation.halation_amount", 0, 3, 0.01),
      slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1, 5, 0.01),
      slider("Scatter amount", "film_render.halation.scatter_amount", 0, 3, 0.01),
      slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1, 5, 0.01),
      slider("Bounces", "film_render.halation.halation_n_bounces", 1, 5, 1),
      slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0, 1, 0.01),
    ] },
    { title: "DIR Couplers", enable: "film_render.dir_couplers.active", controls: [
      slider("Amount", "film_render.dir_couplers.amount", 0, 2, 0.01),
      slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0, 100, 0.1),
      slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0, 400, 1),
      slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0, 1, 0.01),
    ] },
    { title: "Film Base", enable: "film_render.base.active", controls: [
      slider("Density", "film_render.base.scale", 0, 2, 0.01),
      slider("Tilt", "film_render.base.tilt", -1, 1, 0.01),
      slider("Cyan", "film_render.base.cyan", 0, 2, 0.01),
      slider("Magenta", "film_render.base.magenta", 0, 2, 0.01),
      slider("Yellow", "film_render.base.yellow", 0, 2, 0.01),
    ] },
  ] },
  { title: "Print", print: true, footer: "Printing is skipped while scanning film directly.", groups: [
    { title: "Enlarger", open: true, controls: [
      slider("Print exposure", "enlarger.print_exposure", 0.1, 5, 0.01),
      toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
      slider("Magenta shift", "enlarger.m_filter_shift", -50, 50, 0.1),
      slider("Yellow shift", "enlarger.y_filter_shift", -50, 50, 0.1),
      slider("Preflash exposure", "enlarger.preflash_exposure", 0, 0.5, 0.01),
      slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50, 50, 0.1),
      slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50, 50, 0.1),
    ] },
    { title: "Print Diffusion", enable: "enlarger.diffusion_filter.active", controls: [
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
    { title: "Print Curves", enable: "print_render.density_curves_morph.active", controls: curves("print_render.density_curves_morph") },
    { title: "Print Base", enable: "print_render.base.active", controls: [
      slider("Density", "print_render.base.scale", 0, 2, 0.01),
      slider("Cyan", "print_render.base.cyan", 0, 2, 0.01),
      slider("Magenta", "print_render.base.magenta", 0, 2, 0.01),
      slider("Yellow", "print_render.base.yellow", 0, 2, 0.01),
    ] },
    { title: "Glare", enable: "print_render.glare.active", controls: [
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
  ] },
];

// Human-readable names for engine option identifiers (macOS optionTitle).
const TITLES = {
  average: "Average", median: "Median", center_weighted: "Center-Weighted", partial: "Partial",
  matrix: "Matrix", multi_zone: "Multi-Zone", highlight_weighted: "Highlight-Weighted",
  as_shot: "As Shot", daylight: "Daylight", tungsten: "Tungsten", custom: "Custom",
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
