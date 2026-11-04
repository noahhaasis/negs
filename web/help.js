// The inspector's tooltips, behind each control's (i): macOS ControlHelp.swift's
// texts, keyed by parameter path and group title (checks/help.test.mjs keeps
// them equal), then the web's own: Print Diffusion, which the Mac hasn't
// shipped, and the controls the Mac describes in place.

export const FILM = "The film the photograph is shot on: its colour response, contrast, grain and halation, from the stock’s measured data.";
// The Mac's text, less its Keep paper setting, which the web doesn't have.
export const PAPER = "The paper the negative is printed on: its contrast, colour and blacks. Changing the film picks the paper it was made for.";

export const PATHS = {
  // Prep: white balance and brightness
  "io.raw_white_balance.mode": "How the RAW’s colours are balanced before the film. As Shot uses the camera’s setting; Daylight and Tungsten balance for that light; Custom sets temperature and tint. Presets set it too, as they do in the Python app.",
  "io.raw_white_balance.temperature": "The colour of the light to balance for, in kelvin. Lower makes the photograph cooler, higher warmer; 6504 K is Daylight.",
  "io.raw_white_balance.tint": "1 is neutral. Higher is greener, lower more magenta.",
  "camera.auto_exposure_method": "Which part of the photograph the meter reads. Average and Median read the whole frame; Center-Weighted, Partial and Multi-Zone favour the centre; Matrix weighs a 5 × 5 grid; Highlight-Weighted favours bright areas, to keep them from clipping.",

  // Prep: tone, colour, detail. Applied to the scene before the film.
  "prep.highlights": "Darkens (left) or brightens (right) the brightest parts of the scene before the film sees them. Far left recovers highlights that would otherwise crowd the film’s shoulder.",
  "prep.shadows": "Brightens (right) or darkens (left) the darkest parts of the scene before the film sees them.",
  "prep.contrast": "Spreads the scene’s tones apart or draws them together around the mid-tones, before the film adds its own contrast.",
  "prep.lift": "Raises or lowers the scene’s blacks, with less effect on brighter tones.",
  "prep.gain": "Brightens or darkens the scene’s brighter tones, with less effect on the blacks.",
  "prep.color_boost": "Strengthens or mutes muted colours more than colours that are already strong, before the film.",
  "prep.saturation": "Strengthens or mutes all colours evenly, before the film.",
  "prep.midtone_detail": "Local contrast in the mid-tones: right adds texture and clarity, left softens it.",

  // Prep: how the file is read
  "io.input_interpretation.linear": "The file holds linear values, without the colour space’s curve.",
  "io.input_color_space": "The colour space the file’s values are in.",
  "io.input_cctf_decoding": "Decodes the colour space’s curve (its gamma) to linear light. Turn it off for files that are already linear.",

  // Lab: camera
  "camera.exposure_compensation_ev": "Over- or underexposes the film. With Enlarger › Exposure compensation on (the default), the print makes up for it: mid-tones keep their brightness while the negative’s density, grain and highlight and shadow detail change. To make the photograph brighter, use Prep › Exposure.",
  "camera.film_format_mm": "The film the photograph was taken on, by the frame’s long edge. Grain, halation, scatter and diffusion are all sized on the film, so a smaller format shows them larger.",
  "camera.lens_blur_um": "Softness of the taking lens, measured on the film. 0 is a perfect lens.",

  // Lens Diffusion
  "camera.diffusion_filter.filter_family": "The filter in front of the lens. Glimmerglass is the subtlest and tightest; Black Pro-Mist a concentrated, warm halo that also lifts the shadows; Pro-Mist a wider glow; CineBloom the widest, warmest bloom.",
  "camera.diffusion_filter.strength": "The filter’s grade, as sold: 1/8, 1/4, 1/2, 1 and 2. More spreads more light into the halo, which also lifts shadows and lowers contrast.",
  "camera.diffusion_filter.spatial_scale": "How far the halo spreads from each highlight, without changing how much light it takes.",
  "camera.diffusion_filter.halo_warmth": "Warms (right) or cools (left) the halo, on top of the filter’s own tint. 0 keeps the filter’s.",
  "camera.diffusion_filter.core_intensity": "The share of diffused light kept close to each highlight. 1 is the filter’s own; the core, halo and bloom shares are balanced to add up.",
  "camera.diffusion_filter.halo_intensity": "The share of diffused light in the mid-sized halo. 1 is the filter’s own.",
  "camera.diffusion_filter.bloom_intensity": "The share of diffused light in the wide bloom. 1 is the filter’s own.",
  "camera.diffusion_filter.halo_size": "The mid-sized halo’s radius. 1 is the filter’s own.",
  "camera.diffusion_filter.bloom_size": "The wide bloom’s radius. 1 is the filter’s own.",

  // Film Curves
  "film_render.density_curves_morph.gamma_factor": "The film’s development contrast. 1 is normal development; higher is like pushing, lower like pulling.",
  "film_render.density_curves_morph.gamma_factor_fast": "Contrast of the film’s fastest, most light-sensitive layer, which records the shadows.",
  "film_render.density_curves_morph.gamma_factor_slow": "Contrast of the film’s medium and slow layers, which record the mid-tones and highlights.",
  "film_render.density_curves_morph.gamma_factor_red": "Contrast of the film’s red-sensitive (cyan) layer alone. Uneven contrast between layers gives shadows and highlights opposite colour casts.",
  "film_render.density_curves_morph.gamma_factor_green": "Contrast of the film’s green-sensitive (magenta) layer alone.",
  "film_render.density_curves_morph.gamma_factor_blue": "Contrast of the film’s blue-sensitive (yellow) layer alone.",
  "film_render.density_curves_morph.developer_exhaustion": "Developer running out in dense, brightly exposed areas: the highlights roll off more gently, mid-grey stays put. 0 is off.",

  // Grain
  "film_render.grain.rms_granularity": "How coarse the grain is, from the film’s datasheet (RMS granularity). Fine films measure about 4, coarse ones 40.",
  "film_render.grain.uniformity": "How evenly the crystals are spread. Higher makes the grain peak in the mid-tones and ease off in the densest areas; lower lets it keep growing into them.",
  "film_render.grain.particle_scale_sublayers": "The crystal size of one of the emulsion’s three sub-layers, relative to the film’s own. Larger crystals make coarser, stronger grain.",
  "film_render.grain.blur_dye_clouds_um": "How far the dye cloud around each developed crystal spreads, on the film. Visible only at high magnification.",
  "film_render.grain.blur": "Softens the grain after it’s drawn, in pixels. Raise it for large exports, where a crystal covers several pixels; lower it for small ones.",
  "film_render.grain.mult_usm_amount": "Restores the sharpness the grain blur takes away, without adding noise. Too much looks crunchy; 0 is off.",
  "film_render.grain.mult_usm_sigma": "The radius of that sharpening, in pixels.",
  "film_render.grain.micro_structure.0": "Blur of the grain’s finest structure, in µm. Only for extreme magnification.",
  "film_render.grain.micro_structure.1": "Size of the molecular clumps within the grain, in nm. Only for extreme magnification.",

  // Halation
  "film_render.halation.halation_amount": "The red glow around bright highlights, from light reflecting off the back of the film base. 1 is the film’s own; films with little or no anti-halation layer glow most.",
  "film_render.halation.halation_spatial_scale": "How far the halation glow spreads. 1 is the film’s own.",
  "film_render.halation.scatter_amount": "Light scattering sideways inside the emulsion, which softens the whole frame slightly. 1 is the film’s own and the most; lower is sharper than the film.",
  "film_render.halation.scatter_spatial_scale": "How far that scatter spreads. 1 is the film’s own; above it the whole frame softens quickly.",
  "film_render.halation.halation_n_bounces": "How many times light bounces between the emulsion and the back of the base. Typically 2 or 3.",
  "film_render.halation.halation_bounce_decay": "How much of the glow each bounce keeps. Lower fades the later bounces faster.",

  // DIR Couplers
  "film_render.dir_couplers.amount": "Development inhibitor couplers: each colour layer holds back the others as it develops, which gives colour negative film its saturation and edge sharpness. 1 is the film’s own, 0 off.",
  "film_render.dir_couplers.diffusion_size_um": "How far the inhibitor spreads in the emulsion, in µm. Wider makes bright areas hold back development past their edges, which reads as extra clarity; 5–20 µm is the useful range.",
  "film_render.dir_couplers.diffusion_tail_um": "The length of a faint, long-range tail on that spread, in µm, which gives large bright areas a broad, gentle falloff.",
  "film_render.dir_couplers.diffusion_tail_weight": "The share of the inhibitor in the long tail. 0 removes the tail.",

  // Film Base
  "film_render.base.scale": "The density of the film base and fog (the orange mask of colour negatives). 1 is the film’s own. After changing it, use Balance Print.",
  "film_render.base.tilt": "Tilts the base’s density across the spectrum: right adds density in the reds and takes it from the blues, left the reverse.",
  "film_render.base.cyan": "Adds or removes cyan in the film base. 1 is neutral.",
  "film_render.base.magenta": "Adds or removes magenta in the film base. 1 is neutral.",
  "film_render.base.yellow": "Adds or removes yellow in the film base. 1 is neutral.",

  // Enlarger
  "enlarger.print_exposure": "The enlarger’s exposure time. 1 is a normal print; higher prints darker, lower lighter, as with a real enlarger.",
  "enlarger.m_filter_shift": "Magenta filtration of the enlarger, in Kodak CC units from neutral. More magenta in the light makes the print greener; less makes it more magenta.",
  "enlarger.y_filter_shift": "Yellow filtration of the enlarger, in Kodak CC units from neutral. More yellow in the light makes the print bluer; less makes it more yellow.",
  "enlarger.print_exposure_compensation": "Prints the negative to the same mid-grey whatever its Negative exposure, as a printer would. Off, Negative exposure changes the print’s brightness, as with a fixed enlarger setting.",
  "enlarger.preflash_exposure": "A brief, even exposure of the paper before the print: lifts the deepest shadows and lowers contrast. 0 is off.",
  "enlarger.preflash_m_filter_shift": "Magenta filtration for the preflash alone, in Kodak CC units, to tint the lifted shadows.",
  "enlarger.preflash_y_filter_shift": "Yellow filtration for the preflash alone, in Kodak CC units, to tint the lifted shadows.",

  // Print Curves
  "print_render.density_curves_morph.gamma_factor": "The paper’s contrast, like a paper grade. 1 is the paper’s own.",
  "print_render.density_curves_morph.gamma_factor_fast": "Contrast of the paper’s fastest layer, which prints the photograph’s highlights.",
  "print_render.density_curves_morph.gamma_factor_slow": "Contrast of the paper’s medium and slow layers, which print the mid-tones and shadows.",
  "print_render.density_curves_morph.gamma_factor_red": "Contrast of the paper’s red-sensitive (cyan) layer alone. Uneven layers give shadows and highlights opposite casts that filtration can’t fix.",
  "print_render.density_curves_morph.gamma_factor_green": "Contrast of the paper’s green-sensitive (magenta) layer alone.",
  "print_render.density_curves_morph.gamma_factor_blue": "Contrast of the paper’s blue-sensitive (yellow) layer alone.",
  "print_render.density_curves_morph.developer_exhaustion": "Developer running out in the print’s dense, dark areas: the shadows roll off more gently, mid-grey stays put. 0 is off.",

  // Print Base
  "print_render.base.scale": "The density of the paper’s base, its whitest white. 1 is the paper’s own.",
  "print_render.base.cyan": "Scales the cyan in the paper’s base. 1 is the paper’s own.",
  "print_render.base.magenta": "Scales the magenta in the paper’s base. 1 is the paper’s own.",
  "print_render.base.yellow": "Scales the yellow in the paper’s base. 1 is the paper’s own.",

  // Glare
  "print_render.glare.percent": "A faint veil of room light reflecting off the print’s surface, which lifts the deepest blacks slightly.",
  "print_render.glare.roughness": "How unevenly the veil falls across the print, as on a textured surface.",
  "print_render.glare.blur": "Softens the veil’s unevenness, in pixels.",

  // Scanner
  "scanner.lens_blur": "Softness of the scanner’s lens, in pixels. 0 is off.",
  "scanner.unsharp_mask.0": "The radius of the scanner’s sharpening, in pixels.",
  "scanner.unsharp_mask.1": "The strength of the scanner’s sharpening. 0 is off, for sharpening later in another app.",
  "scanner.white_correction": "Sets the scan’s lightest tone to White level. Off, the highlights stay where the paper puts them. Mostly for slides.",
  "scanner.white_level": "The lightest tone with White correction on. 1 is full white.",
  "scanner.black_correction": "Sets the scan’s darkest tone to Black level. Off, the shadows stay where the paper puts them. Mostly for slides.",
  "scanner.black_level": "The darkest tone with Black correction on. 0 is true black; higher lifts and fades the blacks.",

  // Color Management
  "io.output_gamut_compress.algorithm": "Rolls colours the output colour space can’t hold back inside it, leaving colours within it untouched. ACES RGC is the industry standard; OKLCh, OKLrab and CAM16-UCS keep hue more evenly. Off clips them.",
  "io.input_gamut_compress.algorithm": "Pulls the photograph’s colours that lie outside what eyes can see (possible in wide-gamut RAWs) inside, before they become spectra. Off passes them as they are.",
  "settings.use_cat16": "Adapts the photograph’s white to the light the film was characterised in, with CAT16. Off uses CAT02.",
  "settings.rgb_to_raw_method": "How each colour is turned into the spectrum of light the film is exposed to. Hanatos 2025 covers every visible colour; Mallett 2019 only sRGB, clipping the rest; Arctic 2026 is a newer method in testing.",

  // The web's own, from here.
  // Print Diffusion (docs/print-diffusion-plan.md)
  "enlarger.diffusion_filter.filter_family": "The filter under the enlarger lens. Glimmerglass is the subtlest and tightest; Black Pro-Mist a concentrated, warm halo; Pro-Mist a wider glow; CineBloom the widest, warmest bloom. On a print the glow spreads from the dark areas, the negative’s brightest.",
  "enlarger.diffusion_filter.strength": "The filter’s grade, as sold: 1/8, 1/4, 1/2, 1 and 2. More spreads more light into the halo, which softens the print and lowers its contrast.",
  "enlarger.diffusion_filter.spatial_scale": "How far the halo spreads, without changing how much light it takes.",
  "enlarger.diffusion_filter.halo_warmth": "Warms (right) or cools (left) the halo, on top of the filter’s own tint. 0 keeps the filter’s.",
  "enlarger.diffusion_filter.core_intensity": "The share of diffused light kept close to its source. 1 is the filter’s own; the core, halo and bloom shares are balanced to add up.",
  "enlarger.diffusion_filter.core_size": "The core’s radius. 1 is the filter’s own.",
  "enlarger.diffusion_filter.halo_intensity": "The share of diffused light in the mid-sized halo. 1 is the filter’s own.",
  "enlarger.diffusion_filter.halo_size": "The mid-sized halo’s radius. 1 is the filter’s own.",
  "enlarger.diffusion_filter.bloom_intensity": "The share of diffused light in the wide bloom. 1 is the filter’s own.",
  "enlarger.diffusion_filter.bloom_size": "The wide bloom’s radius. 1 is the filter’s own.",

  // Described in place on the Mac (Inspector.swift)
  "camera.auto_exposure": "Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it.",
  "io.scan_film": "Skip printing and scan the developed film, as for slides.",
  "film_render.development_time": "How long the film develops, from the times the stock was measured at. Longer gives more density and contrast.",
  "print_render.development_time": "How long the paper develops, from the times it was measured at. Longer gives deeper blacks and more contrast.",
};

export const GROUPS = {
  "Lens Diffusion": "A diffusion filter on the camera lens: highlights bloom into a soft halo.",
  "Film Curves": "Shapes how the film develops: its contrast, per layer and per colour.",
  "Grain": "The film’s silver grain, drawn from the stock’s measured granularity.",
  "Halation": "Light scattering inside the film and bouncing back off its base: a red glow around bright highlights.",
  "DIR Couplers": "Chemical crosstalk between the film’s colour layers: saturation and edge sharpness.",
  "Film Base": "The colour and density of the clear film, the orange mask of colour negatives.",
  "Enlarger": "Prints the negative: exposure time, colour filtration and preflash.",
  "Print Curves": "Shapes how the paper develops: its contrast, per layer and per colour.",
  "Print Base": "The colour and density of the paper itself, its whitest white.",
  "Glare": "Room light reflecting off the print’s surface.",
  "Scanner": "Scans the finished print: sharpness and black and white points.",
  "Color Management": "How colours enter and leave the spectral simulation.",
  "Input Colour": "How the file’s values are read: its colour space and curve.",
  "Output": "The colour space the preview develops in.",

  // The web's own.
  "Print Diffusion": "A diffusion filter on the enlarger lens: the print glows softly around its shadows.",
};
