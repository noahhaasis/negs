package dev.negs.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sign
import kotlin.math.sin

/**
 * Presentation geometry, ported from web/crop.js (macOS Crop.swift,
 * io/src/crop.rs). Coordinates are y-down; settings stay in the unrotated
 * frame. Neither a crop nor a turn changes what the film develops.
 */
data class Pt(val x: Double, val y: Double)

data class CropRect(val x: Double, val y: Double, val width: Double, val height: Double) {
    val right get() = x + width
    val bottom get() = y + height
    val center get() = Pt(x + width / 2, y + height / 2)
    val corners get() = listOf(Pt(x, y), Pt(right, y), Pt(right, bottom), Pt(x, bottom))
}

/** The crop as the engine takes it (fractions of the photograph) and the editor's aspect choice. */
data class CropSettings(
    val x: Double = 0.0,
    val y: Double = 0.0,
    val width: Double = 1.0,
    val height: Double = 1.0,
    val angle: Double = 0.0,
    val aspect: String = "original",
    val custom: Pair<Double, Double> = 5.0 to 4.0,
) {
    val rect get() = CropRect(x, y, width, height)
    fun with(r: CropRect) = copy(x = r.x, y = r.y, width = r.width, height = r.height)
    val isFull get() = angle == 0.0 && x == 0.0 && y == 0.0 && width == 1.0 && height == 1.0

    /** spektrafilm-io's Crop. */
    fun json(): JsonObject = buildJsonObject {
        put("x", x); put("y", y); put("width", width); put("height", height); put("angle", angle)
    }
}

object Crop {
    val ASPECTS = listOf(
        "original" to "Original", "freeform" to "Freeform", "square" to "Square · 6×6", "16:9" to "16:9",
        "3:2" to "3:2 · 35mm", "4:3" to "4:3 · 645", "5:4" to "5:4 · 4×5", "7:5" to "7:5 · 5×7", "custom" to "Custom",
    )

    fun clamp(v: Double, lo: Double, hi: Double) = min(max(v, lo), max(lo, hi))
    fun turns(n: Int) = ((n % 4) + 4) % 4

    fun shown(rect: CropRect, rotation: Int): CropRect {
        var r = rect
        repeat(turns(rotation)) { r = CropRect(1 - r.y - r.height, r.x, r.height, r.width) }
        return r
    }

    fun unrotated(r: CropRect, rotation: Int) = shown(r, -rotation)

    fun turnedSize(w: Double, h: Double, rotation: Int) = if (turns(rotation) % 2 == 1) h to w else w to h

    /** The crop in pixels of a `w` × `h` photograph, rounded as the exporter rounds it. */
    fun pixels(c: CropSettings, w: Int, h: Int): CropRect {
        if (c.angle != 0.0) return CropRect(c.x * w, c.y * h, max(1, (c.width * w).roundToInt()).toDouble(), max(1, (c.height * h).roundToInt()).toDouble())
        fun edge(f: Double, n: Int) = (f * n).roundToInt().coerceIn(0, n)
        val x1 = min(max(edge(c.x + c.width, w), edge(c.x, w) + 1), w)
        val y1 = min(max(edge(c.y + c.height, h), edge(c.y, h) + 1), h)
        val x0 = min(edge(c.x, w), x1 - 1)
        val y0 = min(edge(c.y, h), y1 - 1)
        return CropRect(x0.toDouble(), y0.toDouble(), (x1 - x0).toDouble(), (y1 - y0).toDouble())
    }

    /** The size a crop and turn give a `w` × `h` photograph. */
    fun outputSize(c: CropSettings, rotation: Int, w: Int, h: Int): Pair<Int, Int> {
        val r = pixels(c, w, h)
        val (tw, th) = turnedSize(r.width, r.height, rotation)
        return tw.roundToInt() to th.roundToInt()
    }

    fun turn(p: Pt, angle: Double, c: Pt): Pt {
        val a = angle * PI / 180
        val s = sin(a)
        val co = cos(a)
        val x = p.x - c.x
        val y = p.y - c.y
        return Pt(c.x + x * co - y * s, c.y + x * s + y * co)
    }

    fun bounds(photo: Pair<Double, Double>, angle: Double): CropRect {
        val (w, h) = photo
        val ps = CropRect(0.0, 0.0, w, h).corners.map { turn(it, angle, Pt(w / 2, h / 2)) }
        val x = ps.minOf { it.x }
        val y = ps.minOf { it.y }
        return CropRect(x, y, ps.maxOf { it.x } - x, ps.maxOf { it.y } - y)
    }

    fun fits(r: CropRect, angle: Double, photo: Pair<Double, Double>): Boolean {
        val (w, h) = photo
        val c = Pt(w / 2, h / 2)
        val eps = 1e-6 * max(max(w, h), 1.0)
        return r.corners.all { p ->
            val q = turn(p, -angle, c)
            q.x >= -eps && q.y >= -eps && q.x <= w + eps && q.y <= h + eps
        }
    }

    fun constrain(start: CropRect, target: CropRect, angle: Double, photo: Pair<Double, Double>): CropRect {
        if (fits(target, angle, photo)) return target
        if (!fits(start, angle, photo)) return start
        fun lerp(t: Double) = CropRect(
            start.x + (target.x - start.x) * t, start.y + (target.y - start.y) * t,
            start.width + (target.width - start.width) * t, start.height + (target.height - start.height) * t,
        )
        var lo = 0.0
        var hi = 1.0
        repeat(24) {
            val mid = (lo + hi) / 2
            if (fits(lerp(mid), angle, photo)) lo = mid else hi = mid
        }
        return lerp(lo)
    }

    /** The largest copy of `r` around its centre (or nearer the photo's) that stays on the turned photograph. */
    fun inscribe(r: CropRect, angle: Double, photo: Pair<Double, Double>): CropRect {
        val (w, h) = photo
        val c = Pt(w / 2, h / 2)
        val mid = r.center
        val half = CropRect(-r.width / 2, -r.height / 2, r.width, r.height).corners.map { turn(it, -angle, Pt(0.0, 0.0)) }
        val candidates = (0..20).map { i ->
            val m = Pt(mid.x + (c.x - mid.x) * i / 20, mid.y + (c.y - mid.y) * i / 20)
            val q = turn(m, -angle, c)
            var s = if (q.x < 0 || q.y < 0 || q.x > w || q.y > h) 0.0 else 1.0
            for (v in half) {
                if (v.x > 0) s = min(s, (w - q.x) / v.x) else if (v.x < 0) s = min(s, q.x / -v.x)
                if (v.y > 0) s = min(s, (h - q.y) / v.y) else if (v.y < 0) s = min(s, q.y / -v.y)
            }
            m to max(s, 0.0)
        }
        val best = candidates.maxOf { it.second }
        if (best == 0.0) return r
        val (m, s) = candidates.first { it.second >= best * 0.98 }
        return CropRect(m.x - r.width * s / 2, m.y - r.height * s / 2, r.width * s, r.height * s)
    }

    fun move(r: CropRect, dx: Double, dy: Double, b: CropRect) =
        r.copy(x = clamp(r.x + dx, b.x, b.right - r.width), y = clamp(r.y + dy, b.y, b.bottom - r.height))

    fun resize(r: CropRect, handle: Pair<Int, Int>, point: Pt, ratio: Double?, b: CropRect, minimum: Double): CropRect {
        val (hx, hy) = handle
        val p = Pt(clamp(point.x, b.x, b.right), clamp(point.y, b.y, b.bottom))
        val anchor = Pt(if (hx < 0) r.right else r.x, if (hy < 0) r.bottom else r.y)
        val roomW = if (hx > 0) b.right - anchor.x else if (hx < 0) anchor.x - b.x else b.width
        val roomH = if (hy > 0) b.bottom - anchor.y else if (hy < 0) anchor.y - b.y else b.height
        val pulledW = max(hx * (p.x - anchor.x), 0.0)
        val pulledH = max(hy * (p.y - anchor.y), 0.0)
        var width = r.width
        var height = r.height
        if (ratio != null) {
            width = minOf(
                maxOf(if (hx != 0) pulledW else 0.0, if (hy != 0) pulledH * ratio else 0.0, minimum, minimum * ratio),
                roomW, roomH * ratio,
            )
            height = width / ratio
        } else {
            if (hx != 0) width = clamp(pulledW, min(minimum, roomW), roomW)
            if (hy != 0) height = clamp(pulledH, min(minimum, roomH), roomH)
        }
        fun origin(hh: Int, a: Double, n: Double, mid: Double, lo: Double, hi: Double) =
            if (hh > 0) a else if (hh < 0) a - n else clamp(mid - n / 2, lo, hi - n)
        return CropRect(
            origin(hx, anchor.x, width, r.center.x, b.x, b.right),
            origin(hy, anchor.y, height, r.center.y, b.y, b.bottom), width, height,
        )
    }

    /** Which corner or edge a touch at `p` takes ((0, 0): inside, to move), or null. */
    fun handleAt(p: Pt, r: CropRect, edge: Double, corner: Double): Pair<Int, Int>? {
        val reach = minOf(corner, r.width / 3, r.height / 3)
        if (p.x < r.x - edge || p.x > r.right + edge || p.y < r.y - edge || p.y > r.bottom + edge) return null
        fun side(v: Double, lo: Double, hi: Double, tolerance: Double) =
            if (abs(v - lo) <= tolerance) -1 else if (abs(v - hi) <= tolerance) 1 else 0
        val x = side(p.x, r.x, r.right, reach)
        val y = side(p.y, r.y, r.bottom, reach)
        return if (x != 0 && y != 0) x to y else side(p.x, r.x, r.right, edge) to side(p.y, r.y, r.bottom, edge)
    }

    /**
     * Bitmap pixels → the displayed picture's pixels (presentation() in
     * crop.js, the forward map): straightened about the centre, cut to the
     * crop, turned. `editing` shows the whole straightened photograph.
     * Returns the displayed size and the map.
     */
    fun displayMatrix(c: CropSettings, rotation: Int, w: Int, h: Int, editing: Boolean): Pair<Pair<Float, Float>, android.graphics.Matrix> {
        val r = if (editing) CropRect(0.0, 0.0, w.toDouble(), h.toDouble()) else CropRect(c.x * w, c.y * h, c.width * w, c.height * h)
        val m = android.graphics.Matrix()
        m.postRotate(c.angle.toFloat(), w / 2f, h / 2f)
        m.postTranslate(-r.x.toFloat(), -r.y.toFloat())
        when (turns(rotation)) {
            1 -> { m.postRotate(90f); m.postTranslate(r.height.toFloat(), 0f) }
            2 -> { m.postRotate(180f); m.postTranslate(r.width.toFloat(), r.height.toFloat()) }
            3 -> { m.postRotate(270f); m.postTranslate(0f, r.width.toFloat()) }
        }
        val (tw, th) = turnedSize(r.width, r.height, rotation)
        return (tw.toFloat() to th.toFloat()) to m
    }
}

/**
 * A crop under way (crop.js Session): the settings while they're edited,
 * shown in the turned frame. Compose reads [settings] and [rotation].
 */
class CropSession(start: CropSettings, private val sourceW: Int, private val sourceH: Int, rotation: Int) {
    var settings by mutableStateOf(start)
    var rotation by mutableStateOf(rotation); private set
    var portrait by mutableStateOf(false); private set
    /** The crop a straightening began from, kept until it ends. */
    var intended: CropRect? by mutableStateOf(null)
    /** A drag is under way: thirds show. */
    var dragging by mutableStateOf(false)

    init {
        val s = shown
        portrait = s.height > s.width
        if (settings.aspect == "custom" && (settings.custom.second > settings.custom.first) != portrait) settings = settings.copy(custom = settings.custom.second to settings.custom.first)
    }

    val photo: Pair<Double, Double> get() = Crop.turnedSize(sourceW.toDouble(), sourceH.toDouble(), rotation)

    /** The crop in the turned photograph's pixels. */
    var shown: CropRect
        get() {
            val r = Crop.shown(settings.rect, rotation)
            val (w, h) = photo
            return CropRect(r.x * w, r.y * h, r.width * w, r.height * h)
        }
        set(r) {
            val (w, h) = photo
            settings = settings.with(Crop.unrotated(CropRect(r.x / w, r.y / h, r.width / w, r.height / h), rotation))
        }

    val ratio: Double?
        get() {
            val s = settings
            if (s.aspect == "freeform") return null
            if (s.aspect == "custom") return s.custom.first / s.custom.second
            val (w, h) = photo
            val r = when (s.aspect) {
                "original" -> max(w, h) / min(w, h)
                "square" -> 1.0
                else -> s.aspect.split(":").map { it.toDouble() }.let { it[0] / it[1] }
            }
            return if (portrait) 1 / r else r
        }

    fun refit() {
        val ratio = ratio ?: return
        val (pw, ph) = photo
        val mid = shown.center
        val width = min(pw, ph * ratio)
        val height = width / ratio
        shown = Crop.inscribe(
            CropRect(Crop.clamp(mid.x - width / 2, 0.0, pw - width), Crop.clamp(mid.y - height / 2, 0.0, ph - height), width, height),
            settings.angle, photo,
        )
    }

    fun choose(aspect: String) {
        settings = settings.copy(aspect = aspect)
        if (aspect == "original") portrait = photo.second > photo.first
        refit()
    }

    fun orient(portrait: Boolean) {
        this.portrait = portrait
        if (settings.aspect == "custom" && (settings.custom.second > settings.custom.first) != portrait) settings = settings.copy(custom = settings.custom.second to settings.custom.first)
        refit()
    }

    fun setCustom(index: Int, value: Double) {
        val v = Crop.clamp(value, 0.1, 1000.0)
        settings = settings.copy(custom = if (index == 0) v to settings.custom.second else settings.custom.first to v)
        refit()
    }

    fun rotate(n: Int) {
        rotation = Crop.turns(rotation + n)
        if (Crop.turns(n) % 2 == 1) {
            portrait = !portrait
            settings = settings.copy(custom = settings.custom.second to settings.custom.first)
        }
        intended = null
    }

    fun straighten(angle: Double) {
        val start = intended ?: shown
        intended = start
        val a = Crop.clamp(sign(angle) * (abs(angle) * 10).roundToInt() / 10.0, -45.0, 45.0)
        settings = settings.copy(angle = a)
        shown = Crop.inscribe(start, a, photo)
    }

    fun endStraighten() {
        intended = null
    }

    fun reset() {
        settings = CropSettings(custom = settings.custom)
        portrait = photo.second > photo.first
        intended = null
    }

    fun committed(): CropSettings {
        val s = settings
        return if (s.angle == 0.0 && abs(s.x) < 1e-3 && abs(s.y) < 1e-3 && abs(s.x + s.width - 1) < 1e-3 && abs(s.y + s.height - 1) < 1e-3)
            s.copy(x = 0.0, y = 0.0, width = 1.0, height = 1.0) else s
    }
}
