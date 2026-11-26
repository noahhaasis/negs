package dev.negs.app

/**
 * The inspector's controls on the Prep, Lab and Output tabs: RuntimeParams
 * paths with desktop's titles, ranges and steps, ported from web/controls.js
 * (itself macOS Inspector.swift's). Defaults come from the engine's
 * catalog, never from here.
 */
sealed interface Control {
    val label: String
    /** Hidden until Advanced controls is on, unless changed. */
    val advanced: Boolean get() = false
    /** Shown while this path is true, or (with [whenValue]) has that value. */
    val whenPath: String? get() = null
    val whenValue: String? get() = null
    val help: String? get() = null
}

data class Slider(
    override val label: String,
    val path: String,
    val min: Double,
    val max: Double,
    val step: Double,
    override val advanced: Boolean = false,
    override val whenPath: String? = null,
    override val whenValue: String? = null,
    override val help: String? = null,
) : Control

data class Toggle(
    override val label: String,
    val path: String,
    override val whenPath: String? = null,
    override val help: String? = null,
) : Control

data class Choice(
    override val label: String,
    val path: String,
    val options: List<String>,
    override val whenPath: String? = null,
    override val whenValue: String? = null,
) : Control

/** Prep › Match camera brightness, for camera RAWs (web/app.js `match`). */
data class MatchSwitch(override val label: String, override val help: String? = null) : Control

/** Prep's Exposure as an offset on the match (web controls.js `kind: "offset"`). */
data class ExposureOffset(override val label: String) : Control

/** The match's note under Exposure. */
data class MatchNote(override val label: String) : Control

/** A command rather than a value: the editor runs [action]. */
data class Action(override val label: String, val action: String, override val help: String? = null) : Control

/**
 * One lane of a grain array that stays null until edited, so the film's
 * preset from the catalog applies. B&W films have one emulsion: they show
 * the lane as [bw], or hide it when that is null.
 */
data class StockLane(
    override val label: String,
    val path: String,
    val index: Int,
    val min: Double,
    val max: Double,
    val step: Double,
    val bw: String?,
) : Control

/** The glasses for black-and-white film (issue #21). */
data class ColorFilters(override val label: String, val path: String) : Control

/** A border setting, outside the params (web/border.js). */
data class BorderControl(
    override val label: String,
    val field: BorderField,
    /** Only for these styles; empty for all. */
    val styles: List<BorderStyle> = emptyList(),
    override val help: String? = null,
) : Control

data class Group(
    val title: String,
    val controls: List<Control>,
    /** The switch in its header. */
    val enable: String? = null,
    /** How many of its first controls show without Advanced controls. */
    val basic: Int = Int.MAX_VALUE,
    val open: Boolean = false,
    val colorFilter: Boolean = false,
)

data class Section(
    val title: String,
    val tab: String,
    val controls: List<Control> = emptyList(),
    val groups: List<Group> = emptyList(),
    /** Skipped while scanning film directly. */
    val print: Boolean = false,
    /** For camera RAWs only. */
    val raw: Boolean = false,
    val footer: String? = null,
)

object Controls {
    val METERING = listOf("average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted")
    val RAW_WB_MODES = listOf("as_shot", "daylight", "tungsten", "custom")
    private val DIFFUSION_FILTERS = listOf("black_pro_mist", "glimmerglass", "pro_mist", "cinebloom")

    /** macOS ColorFilter.effect, under the catalog's names. */
    val COLOR_FILTER_EFFECTS = mapOf(
        "none" to "No glass in front of the lens.",
        "hoya_x1" to "Lightens foliage; darkens red lips and skin a little, for a tanned look.",
        "hoya_x0" to "Lightens foliage and darkens the sky a little. Kind to skin outdoors.",
        "hoya_y2" to "Darkens blue sky a touch so clouds stand out. The everyday filter.",
        "hoya_ya3" to "Darker sky, stronger clouds, less haze. Hides freckles and blemishes.",
        "hoya_r1" to "Near-black blue sky, bright red and brick, pale skin and lips. Cuts haze hard.",
    )

    private fun curves(at: String) = listOf(
        Slider("Gamma", "$at.gamma_factor", 0.5, 2.0, 0.01),
        Slider("Fast gamma", "$at.gamma_factor_fast", 0.5, 2.0, 0.01),
        Slider("Slow gamma", "$at.gamma_factor_slow", 0.5, 2.0, 0.01),
        Slider("Red gamma", "$at.gamma_factor_red", 0.5, 2.0, 0.01),
        Slider("Green gamma", "$at.gamma_factor_green", 0.5, 2.0, 0.01),
        Slider("Blue gamma", "$at.gamma_factor_blue", 0.5, 2.0, 0.01),
        Slider("Developer exhaustion", "$at.developer_exhaustion", 0.0, 1.0, 0.01),
    )

    val PREP = listOf(
        Section("RAW White Balance", "Prep", raw = true, controls = listOf(
            Choice("White balance", "io.raw_white_balance.mode", RAW_WB_MODES),
            Slider("Temperature (K)", "io.raw_white_balance.temperature", 1667.0, 25000.0, 1.0,
                whenPath = "io.raw_white_balance.mode", whenValue = "custom"),
            Slider("Tint", "io.raw_white_balance.tint", 0.01, 4.0, 0.01,
                whenPath = "io.raw_white_balance.mode", whenValue = "custom"),
        )),
        Section("Brightness", "Prep", controls = listOf(
            MatchSwitch("Match camera brightness",
                help = "Develops the RAW about as bright as the camera’s own preview, and keeps it there when you change film, paper or a preset. Exposure adds to it."),
            ExposureOffset("Exposure (EV)"),
            MatchNote("Camera brightness"),
            Toggle("Auto exposure", "camera.auto_exposure",
                help = "Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it."),
            Choice("Metering", "camera.auto_exposure_method", METERING, whenPath = "camera.auto_exposure"),
        )),
        Section("Tone", "Prep", controls = listOf(
            Slider("Highlights", "prep.highlights", -180.0, 100.0, 1.0),
            Slider("Shadows", "prep.shadows", -100.0, 100.0, 1.0),
            Slider("Contrast", "prep.contrast", -100.0, 100.0, 1.0),
            Slider("Lift", "prep.lift", -100.0, 100.0, 1.0),
            Slider("Gain", "prep.gain", -100.0, 100.0, 1.0),
        )),
        Section("Color", "Prep", controls = listOf(
            Slider("Color Boost", "prep.color_boost", -100.0, 100.0, 1.0),
            Slider("Saturation", "prep.saturation", -100.0, 100.0, 1.0),
        )),
        Section("Detail", "Prep", controls = listOf(
            Slider("Midtone Detail", "prep.midtone_detail", -100.0, 100.0, 1.0),
        )),
    )

    val LAB = listOf(
        Section("Camera", "Camera", controls = listOf(
            Slider("Negative exposure (EV)", "camera.exposure_compensation_ev", -5.0, 5.0, 0.1),
            Slider("Film format (mm)", "camera.film_format_mm", 4.0, 120.0, 0.1),
            Slider("Lens blur (µm)", "camera.lens_blur_um", 0.0, 100.0, 0.1, advanced = true),
        ), groups = listOf(
            Group("Color Filter", listOf(ColorFilters("Color filter", "camera.color_filter")), basic = 1, open = true, colorFilter = true),
            Group("Lens Diffusion", enable = "camera.diffusion_filter.active", basic = 2, controls = listOf(
                Choice("Filter", "camera.diffusion_filter.filter_family", DIFFUSION_FILTERS),
                Slider("Strength", "camera.diffusion_filter.strength", 0.0, 2.0, 0.01),
                Slider("Spatial scale", "camera.diffusion_filter.spatial_scale", 0.1, 3.0, 0.01),
                Slider("Halo warmth", "camera.diffusion_filter.halo_warmth", -1.5, 1.5, 0.01),
                Slider("Core intensity", "camera.diffusion_filter.core_intensity", 0.0, 2.0, 0.01),
                Slider("Halo intensity", "camera.diffusion_filter.halo_intensity", 0.0, 2.0, 0.01),
                Slider("Bloom intensity", "camera.diffusion_filter.bloom_intensity", 0.0, 2.0, 0.01),
                Slider("Halo size", "camera.diffusion_filter.halo_size", 0.1, 3.0, 0.01),
                Slider("Bloom size", "camera.diffusion_filter.bloom_size", 0.1, 3.0, 0.01),
            )),
        )),
        Section("Film", "Film", groups = listOf(
            Group("Film Curves", curves("film_render.density_curves_morph"), enable = "film_render.density_curves_morph.active", basic = 1),
            Group("Grain", enable = "film_render.grain.active", basic = 3, controls = listOf(
                StockLane("Red granularity", "film_render.grain.rms_granularity", 0, 0.0, 80.0, 0.1, "Granularity"),
                StockLane("Green granularity", "film_render.grain.rms_granularity", 1, 0.0, 80.0, 0.1, null),
                StockLane("Blue granularity", "film_render.grain.rms_granularity", 2, 0.0, 80.0, 0.1, null),
                StockLane("Red uniformity", "film_render.grain.uniformity", 0, 0.0, 1.0, 0.01, "Uniformity"),
                StockLane("Green uniformity", "film_render.grain.uniformity", 1, 0.0, 1.0, 0.01, null),
                StockLane("Blue uniformity", "film_render.grain.uniformity", 2, 0.0, 1.0, 0.01, null),
                StockLane("Coarse grain size", "film_render.grain.particle_scale_sublayers", 0, 0.05, 2.0, 0.01, "Coarse grain size"),
                StockLane("Medium grain size", "film_render.grain.particle_scale_sublayers", 1, 0.05, 2.0, 0.01, "Medium grain size"),
                StockLane("Fine grain size", "film_render.grain.particle_scale_sublayers", 2, 0.05, 2.0, 0.01, "Fine grain size"),
                Slider("Dye clouds (µm)", "film_render.grain.blur_dye_clouds_um", 0.0, 5.0, 0.01),
                Slider("Blur (px)", "film_render.grain.blur", 0.0, 3.0, 0.01),
                Slider("Sharpen amount", "film_render.grain.mult_usm_amount", 0.0, 3.0, 0.01),
                Slider("Sharpen radius (px)", "film_render.grain.mult_usm_sigma", 0.0, 3.0, 0.01),
                Slider("Clump blur (µm)", "film_render.grain.micro_structure.0", 0.0, 1.0, 0.01),
                Slider("Clump size (nm)", "film_render.grain.micro_structure.1", 0.0, 100.0, 1.0),
            )),
            Group("Halation", enable = "film_render.halation.active", basic = 1, controls = listOf(
                Slider("Amount", "film_render.halation.halation_amount", 0.0, 3.0, 0.01),
                Slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1, 5.0, 0.01),
                Slider("Scatter amount", "film_render.halation.scatter_amount", 0.0, 3.0, 0.01),
                Slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1, 5.0, 0.01),
                Slider("Bounces", "film_render.halation.halation_n_bounces", 1.0, 5.0, 1.0),
                Slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0.0, 1.0, 0.01),
            )),
            Group("DIR Couplers", enable = "film_render.dir_couplers.active", basic = 1, controls = listOf(
                Slider("Amount", "film_render.dir_couplers.amount", 0.0, 2.0, 0.01),
                Slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0.0, 100.0, 0.1),
                Slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0.0, 400.0, 1.0),
                Slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0.0, 1.0, 0.01),
            )),
            Group("Film Base", enable = "film_render.base.active", basic = 1, controls = listOf(
                Slider("Density", "film_render.base.scale", 0.0, 2.0, 0.01),
                Slider("Tilt", "film_render.base.tilt", -1.0, 1.0, 0.01),
                Slider("Cyan", "film_render.base.cyan", 0.0, 2.0, 0.01),
                Slider("Magenta", "film_render.base.magenta", 0.0, 2.0, 0.01),
                Slider("Yellow", "film_render.base.yellow", 0.0, 2.0, 0.01),
            )),
        )),
        Section("Print", "Print", print = true, footer = "Printing is skipped while scanning film directly.", groups = listOf(
            Group("Enlarger", open = true, basic = 4, controls = listOf(
                Slider("Print exposure", "enlarger.print_exposure", 0.1, 5.0, 0.01),
                Slider("Magenta shift", "enlarger.m_filter_shift", -50.0, 50.0, 0.1),
                Slider("Yellow shift", "enlarger.y_filter_shift", -50.0, 50.0, 0.1),
                Action("Balance Print", "balancePrint",
                    "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base."),
                Toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
                Slider("Preflash exposure", "enlarger.preflash_exposure", 0.0, 0.5, 0.01),
                Slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50.0, 50.0, 0.1),
                Slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50.0, 50.0, 0.1),
            )),
            Group("Print Diffusion", enable = "enlarger.diffusion_filter.active", basic = 2, controls = listOf(
                Choice("Filter", "enlarger.diffusion_filter.filter_family", DIFFUSION_FILTERS),
                Slider("Strength", "enlarger.diffusion_filter.strength", 0.0, 2.0, 0.01),
                Slider("Spatial scale", "enlarger.diffusion_filter.spatial_scale", 0.1, 3.0, 0.01),
                Slider("Halo warmth", "enlarger.diffusion_filter.halo_warmth", -1.5, 1.5, 0.01),
                Slider("Core intensity", "enlarger.diffusion_filter.core_intensity", 0.0, 4.0, 0.05),
                Slider("Core size", "enlarger.diffusion_filter.core_size", 0.1, 4.0, 0.05),
                Slider("Halo intensity", "enlarger.diffusion_filter.halo_intensity", 0.0, 4.0, 0.05),
                Slider("Halo size", "enlarger.diffusion_filter.halo_size", 0.1, 4.0, 0.05),
                Slider("Bloom intensity", "enlarger.diffusion_filter.bloom_intensity", 0.0, 4.0, 0.05),
                Slider("Bloom size", "enlarger.diffusion_filter.bloom_size", 0.1, 4.0, 0.05),
            )),
            Group("Print Curves", curves("print_render.density_curves_morph"), enable = "print_render.density_curves_morph.active", basic = 1),
            Group("Print Base", enable = "print_render.base.active", basic = 1, controls = listOf(
                Slider("Density", "print_render.base.scale", 0.0, 2.0, 0.01),
                Slider("Cyan", "print_render.base.cyan", 0.0, 2.0, 0.01),
                Slider("Magenta", "print_render.base.magenta", 0.0, 2.0, 0.01),
                Slider("Yellow", "print_render.base.yellow", 0.0, 2.0, 0.01),
            )),
            Group("Glare", enable = "print_render.glare.active", basic = 1, controls = listOf(
                Slider("Amount", "print_render.glare.percent", 0.0, 0.2, 0.001),
                Slider("Roughness", "print_render.glare.roughness", 0.0, 2.0, 0.01),
                Slider("Blur (px)", "print_render.glare.blur", 0.0, 5.0, 0.01),
            )),
        )),
        Section("Scan & Color", "Scan", groups = listOf(
            Group("Scanner", basic = 0, controls = listOf(
                Slider("Lens blur (px)", "scanner.lens_blur", 0.0, 5.0, 0.01),
                Slider("Color noise reduction", "scanner.chroma_noise_reduction", 0.0, 100.0, 1.0),
                Slider("Unsharp radius (px)", "scanner.unsharp_mask.0", 0.0, 3.0, 0.01),
                Slider("Unsharp amount", "scanner.unsharp_mask.1", 0.0, 2.0, 0.01),
                Toggle("White correction", "scanner.white_correction"),
                Slider("White level", "scanner.white_level", 0.5, 1.0, 0.01),
                Toggle("Black correction", "scanner.black_correction"),
                Slider("Black level", "scanner.black_level", 0.0, 0.5, 0.01),
            )),
            Group("Color Management", basic = 0, controls = listOf(
                Choice("Output gamut", "io.output_gamut_compress.algorithm", listOf("off", "oklch", "oklrab", "cam16ucs", "aces_rgc")),
                Choice("Input gamut", "io.input_gamut_compress.algorithm", listOf("off", "xy")),
                Choice("Spectral upsampling", "settings.rgb_to_raw_method", listOf("hanatos2025", "arctic2026beta04", "mallett2019")),
            )),
        )),
    )

    val OUTPUT = listOf(
        Section("Border", "Output", controls = listOf(
            BorderControl("Add border", BorderField.Enabled,
                help = "Frames the photograph in a mat or a film frame. The preview always shows it; Include in export decides whether files get it."),
            BorderControl("Style", BorderField.Style,
                help = "Solid is a plain mat; Keyline adds a thin line around the photograph; Film is a scanned 120 film frame; 35mm a negative strip with the film’s edge print."),
            BorderControl("Width", BorderField.Width, listOf(BorderStyle.Solid, BorderStyle.Keyline), "The mat’s width, in percent of the photograph’s long edge."),
            BorderControl("Line width", BorderField.Line, listOf(BorderStyle.Keyline), "The line’s width, in percent of the photograph’s long edge."),
            BorderControl("Color", BorderField.Color, listOf(BorderStyle.Solid, BorderStyle.Keyline), "The mat’s colour."),
            BorderControl("Line color", BorderField.LineColor, listOf(BorderStyle.Keyline), "The line’s colour."),
            BorderControl("Width", BorderField.FilmWidth, listOf(BorderStyle.Film), "Adds black around the film frame, in percent of the photograph’s long edge."),
            BorderControl("Extend to 3:4 for Instagram", BorderField.Instagram,
                help = "Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post."),
            BorderControl("Include in export", BorderField.Export, help = "Adds the border to exported photographs."),
        )),
    )

    /** Human-readable names for engine option identifiers (macOS optionTitle). */
    private val TITLES = mapOf(
        "average" to "Average", "median" to "Median", "center_weighted" to "Center-Weighted", "partial" to "Partial",
        "matrix" to "Matrix", "multi_zone" to "Multi-Zone", "highlight_weighted" to "Highlight-Weighted",
        "as_shot" to "As Shot", "daylight" to "Daylight", "tungsten" to "Tungsten (2850 K)", "custom" to "Custom",
        "black_pro_mist" to "Black Pro-Mist", "glimmerglass" to "Glimmerglass", "pro_mist" to "Pro-Mist", "cinebloom" to "CineBloom",
        "off" to "Off", "oklch" to "OKLCh", "oklrab" to "OKLrab", "cam16ucs" to "CAM16-UCS", "aces_rgc" to "ACES RGC", "xy" to "xy Chromaticity",
        "hanatos2025" to "Hanatos 2025", "arctic2026beta04" to "Arctic 2026 (Beta 04)", "mallett2019" to "Mallett 2019",
    )

    fun optionTitle(option: String) = TITLES[option]
        ?: option.replace('_', ' ').split(' ').joinToString(" ") { w -> w.replaceFirstChar { it.uppercase() } }

    /** A slider's value, snapped to its step and range as desktop stores it. */
    fun snap(value: Double, min: Double, max: Double, step: Double): Double {
        val clamped = value.coerceIn(min, max)
        val steps = Math.round((clamped - min) / step)
        val decimals = maxOf(0, Math.floor(-Math.log10(step)).toInt() + 1)
        val scale = Math.pow(10.0, decimals.toDouble())
        return minOf(Math.round((min + steps * step) * scale) / scale, max)
    }

    /** As desktop shows a value: up to the step's decimals, no trailing zeros. */
    fun format(value: Double, step: Double): String {
        val decimals = maxOf(0, Math.ceil(-Math.log10(step) - 1e-9).toInt())
        val text = "%.${decimals}f".format(java.util.Locale.ROOT, value)
        return if ('.' in text) text.trimEnd('0').trimEnd('.').let { if (it == "-0") "0" else it } else text
    }
}
