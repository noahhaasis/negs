package dev.negs.app

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** web/border.js STYLES, in macOS BorderStyle's order; [id] is the exporter's name. */
enum class BorderStyle(val id: String, val title: String, val help: String) {
    Solid("solid", "Solid", "A plain mat around the photograph."),
    Film("film", "Film", "A scanned 120 film frame with an irregular black edge."),
    Keyline("keyline", "Keyline", "A mat with a thin line around the photograph."),
    Strip35("35mm", "35mm", "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph."),
}

enum class BorderField { Enabled, Style, Width, Line, Color, LineColor, FilmWidth, Instagram, Export }

/**
 * The presentation border (macOS BorderSettings, web/border.js): outside the
 * edit's params, so it never renders the film. Widths are fractions of the
 * photograph's long edge; colours 0xRRGGBB.
 */
data class BorderSettings(
    val enabled: Boolean = false,
    val style: BorderStyle = BorderStyle.Solid,
    val export: Boolean = true,
    val instagram: Boolean = false,
    val width: Double = 0.03,
    val filmWidth: Double = 0.0,
    val line: Double = 0.004,
    val color: Int = 0xFFFFFF,
    val lineColor: Int = 0x000000,
) {
    /** The 35mm strip needs a 3:2 or 3:4 photograph (border.rs FilmFormat::of). */
    fun fits(width: Int, height: Int): Boolean {
        if (style != BorderStyle.Strip35) return true
        val ratio = max(width, height).toDouble() / max(1, min(width, height))
        return abs(ratio / 1.5 - 1) <= 0.01 || abs(ratio / (4.0 / 3.0) - 1) <= 0.01
    }

    fun shown(width: Int, height: Int) = enabled && fits(width, height)
    fun exported(width: Int, height: Int) = export && shown(width, height)

    /** The exporter's BorderOptions. */
    fun options(): JsonObject = buildJsonObject {
        put("line", line)
        put("color", hex(color))
        put("line_color", hex(lineColor))
        when (style) {
            BorderStyle.Solid, BorderStyle.Keyline -> put("width", width)
            BorderStyle.Film -> put("width", filmWidth)
            BorderStyle.Strip35 -> {}
        }
    }

    companion object {
        fun hex(rgb: Int) = "#%06x".format(rgb and 0xFFFFFF)
        /** macOS's mat colours: white, paper, grey, black. */
        val SWATCHES = listOf(0xFFFFFF, 0xF2EEE6, 0x808080, 0x000000)
    }
}

/** A rectangle in the bordered canvas's pixels. */
data class IRect(val x: Int, val y: Int, val width: Int, val height: Int) {
    fun moved(dx: Int, dy: Int) = IRect(x + dx, y + dy, width, height)
}

/**
 * The texture drawn over the photograph: the 120 frame as nine patches
 * ([kind] 1) or the 35mm strip whole (2), turned a quarter anticlockwise
 * when [turned]. [pad] and [scale] place the 120 frame's corners.
 */
data class BorderPatch(val kind: Int, val rect: IRect, val turned: Boolean, val pad: Int, val scale: Double)

/**
 * Where a border puts everything around a `w` × `h` photograph (web/border.js
 * layout, after border.rs and film35.rs): the canvas, the photograph's
 * corner, the mat, the keyline and the texture. Colours are 0xRRGGBB.
 */
data class BorderLayout(
    val width: Int, val height: Int, val x: Int, val y: Int,
    val frame: IRect, val line: IRect?, val patch: BorderPatch?,
    val fill: Int, val mat: Int, val lineColor: Int,
) {
    companion object {
        const val FILM_TEXTURE = "borders/film.png"

        /** "full" (36 × 24 mm, 3:2), "half" (18 × 24 mm, 3:4) or null, within 1%. */
        fun filmFormat(w: Int, h: Int): String? {
            val ratio = max(w, h).toDouble() / max(1, min(w, h))
            if (abs(ratio / 1.5 - 1) <= 0.01) return "full"
            if (abs(ratio / (4.0 / 3.0) - 1) <= 0.01) return "half"
            return null
        }

        fun stripPath(format: String, film: String) = "borders/35mm/$format/$film.png"
        fun genericStrip(format: String) = "borders/35mm/$format/generic.png"

        private fun margin(w: Int, h: Int, fraction: Double) = kotlin.math.floor(max(w, h) * fraction).toInt()
        private fun keylineWidth(w: Int, h: Int, fraction: Double) = max(1, (max(w, h) * fraction).roundToInt())

        private const val FILM_WIDTH = 35.0
        private const val GATE_HEIGHT = 24.0
        private const val PERF_PITCH = 4.75
        private fun stripSize(format: String) = if (format == "full") 8 * PERF_PITCH to 36.0 else 4 * PERF_PITCH to 18.0
        private fun stripTurned(format: String, w: Int, h: Int) = (stripSize(format).second > GATE_HEIGHT) != (w > h)
        private fun stripCanvas(format: String, w: Int, h: Int): Pair<Int, Int> {
            val (length, gate) = stripSize(format)
            var tw = length; var th = FILM_WIDTH
            var ww = gate / length; var wh = GATE_HEIGHT / FILM_WIDTH
            if (stripTurned(format, w, h)) { tw = th.also { th = tw }; ww = wh.also { wh = ww } }
            val k = max(w / (ww * tw), h / (wh * th))
            return (tw * k).roundToInt() to (th * k).roundToInt()
        }

        private fun instagramCanvas(w: Int, h: Int, style: BorderStyle): Pair<Int, Int> {
            val m = if (style == BorderStyle.Film || style == BorderStyle.Strip35) margin(w, h, 0.03) else 0
            val cw = w + 2 * m
            val ch = h + 2 * m
            return if (cw.toLong() * 4 > ch.toLong() * 3) cw to kotlin.math.ceil(cw * 4.0 / 3).toInt() else kotlin.math.ceil(ch * 3.0 / 4).toInt() to ch
        }

        /**
         * `texture`: the texture's size once loaded. Without the strip's, the
         * 35mm canvas is the photograph alone, as macOS shows it until it loads.
         */
        fun of(b: BorderSettings, w: Int, h: Int, texture: Pair<Int, Int>?): BorderLayout {
            var width: Int; var height: Int; var x: Int; var y: Int
            var line: IRect? = null
            var patch: BorderPatch? = null
            when (b.style) {
                BorderStyle.Solid, BorderStyle.Keyline -> {
                    val m = margin(w, h, b.width)
                    val l = if (b.style == BorderStyle.Keyline) keylineWidth(w, h, b.line) else 0
                    width = w + 2 * (m + l); height = h + 2 * (m + l); x = m + l; y = m + l
                    if (l > 0) line = IRect(m, m, w + 2 * l, h + 2 * l)
                }
                BorderStyle.Film -> {
                    val short = min(w, h)
                    val inset = max(1, (short * 0.003).roundToInt())
                    val pad = max(2, (short * 0.025).roundToInt())
                    val extra = margin(w, h, b.filmWidth)
                    val iw = w + 2 * inset
                    val ih = h + 2 * inset
                    width = iw + 2 * extra; height = ih + 2 * extra; x = inset + extra; y = inset + extra
                    if (texture != null) {
                        val scale = min(iw, ih).toDouble() / min(texture.first, texture.second)
                        patch = BorderPatch(1, IRect(extra, extra, iw, ih), h > w, pad, scale)
                    }
                }
                BorderStyle.Strip35 -> {
                    val format = filmFormat(w, h)
                    if (texture != null && format != null) {
                        val (cw, ch) = stripCanvas(format, w, h)
                        width = cw; height = ch; x = (cw - w) / 2; y = (ch - h) / 2
                        patch = BorderPatch(2, IRect(0, 0, cw, ch), stripTurned(format, w, h), 0, 0.0)
                    } else {
                        width = w; height = h; x = 0; y = 0
                    }
                }
            }
            var frame = IRect(0, 0, width, height)
            val film = b.style == BorderStyle.Film || b.style == BorderStyle.Strip35
            val mat = if (film) 0x000000 else b.color
            var fill = mat
            if (b.instagram) {
                val (iw, ih) = instagramCanvas(width, height, b.style)
                val dx = (iw - width) / 2
                val dy = (ih - height) / 2
                frame = frame.moved(dx, dy)
                line = line?.moved(dx, dy)
                patch = patch?.copy(rect = patch.rect.moved(dx, dy))
                width = iw; height = ih; x += dx; y += dy
                fill = if (film) 0xFFFFFF else mat
            }
            return BorderLayout(width, height, x, y, frame, line, patch, fill, mat, b.lineColor)
        }
    }
}
