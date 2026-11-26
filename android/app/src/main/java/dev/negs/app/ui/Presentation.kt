package dev.negs.app.ui

import android.graphics.Bitmap
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.nativeCanvas
import dev.negs.app.BorderLayout
import dev.negs.app.BorderSettings
import dev.negs.app.CropRect
import dev.negs.app.Crop
import dev.negs.app.CropSession
import dev.negs.app.CropSettings
import dev.negs.app.Pt
import kotlin.math.min
import kotlin.math.roundToInt

private val filtered = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
private fun fill(rgb: Int) = Paint().apply { color = 0xFF000000.toInt() or rgb; style = Paint.Style.FILL }

/**
 * The photograph as it exports (web/app.js showView, border.js paintUnder
 * and paintOver): the frame `bitmap` (the whole photograph, developed)
 * straightened, cropped and turned to the output's `out` size, inside its
 * border, fitted to the view. Rectangles follow border.rs to the pixel at
 * the output's size.
 */
fun DrawScope.drawPresented(
    bitmap: Bitmap, crop: CropSettings, rotation: Int, out: Pair<Int, Int>,
    border: BorderSettings, texture: Bitmap?,
) {
    val (w, h) = out
    val layout = if (border.shown(w, h)) BorderLayout.of(border, w, h, texture?.let { it.width to it.height }) else null
    val cw = layout?.width ?: w
    val ch = layout?.height ?: h
    val fit = min(size.width / cw, size.height / ch)
    val left = (size.width - cw * fit) / 2
    val top = (size.height - ch * fit) / 2
    val canvas = drawContext.canvas.nativeCanvas
    fun rect(x: Int, y: Int, rw: Int, rh: Int) = RectF(left + x * fit, top + y * fit, left + (x + rw) * fit, top + (y + rh) * fit)
    if (layout != null) {
        canvas.drawRect(rect(0, 0, cw, ch), fill(layout.fill))
        layout.frame.let { canvas.drawRect(rect(it.x, it.y, it.width, it.height), fill(layout.mat)) }
        layout.line?.let { canvas.drawRect(rect(it.x, it.y, it.width, it.height), fill(layout.lineColor)) }
    }
    val photo = rect(layout?.x ?: 0, layout?.y ?: 0, w, h)
    val (shown, matrix) = Crop.displayMatrix(crop, rotation, bitmap.width, bitmap.height, editing = false)
    matrix.postScale(photo.width() / shown.first, photo.height() / shown.second)
    matrix.postTranslate(photo.left, photo.top)
    canvas.save()
    canvas.clipRect(photo)
    canvas.drawBitmap(bitmap, matrix, filtered)
    canvas.restore()
    val patch = layout?.patch
    if (patch != null && texture != null) {
        val r = rect(patch.rect.x, patch.rect.y, patch.rect.width, patch.rect.height)
        canvas.save()
        canvas.translate(r.left, r.top)
        var dw = r.width()
        var dh = r.height()
        // Turned: a quarter anticlockwise, as film35.rs's Renderer turns it.
        if (patch.turned) {
            canvas.translate(0f, r.height())
            canvas.rotate(-90f)
            dw = r.height(); dh = r.width()
        }
        if (patch.kind == 2) {
            canvas.drawBitmap(texture, null, RectF(0f, 0f, dw, dh), filtered)
        } else {
            // Nine patches: corners `pad` output pixels, edges and centre stretched.
            val tw = texture.width.toFloat()
            val th = texture.height.toFloat()
            val pad = patch.pad * fit
            val tp = (patch.pad / patch.scale).toFloat()
            val xs = listOf(floatArrayOf(0f, tp, 0f, pad), floatArrayOf(tp, tw - 2 * tp, pad, dw - 2 * pad), floatArrayOf(tw - tp, tp, dw - pad, pad))
            val ys = listOf(floatArrayOf(0f, tp, 0f, pad), floatArrayOf(tp, th - 2 * tp, pad, dh - 2 * pad), floatArrayOf(th - tp, tp, dh - pad, pad))
            for (x in xs) for (y in ys) {
                if (x[1] > 0 && y[1] > 0 && x[3] > 0 && y[3] > 0) {
                    val src = Rect(x[0].roundToInt(), y[0].roundToInt(), (x[0] + x[1]).roundToInt(), (y[0] + y[1]).roundToInt())
                    canvas.drawBitmap(texture, src, RectF(x[2], y[2], x[2] + x[3], y[2] + y[3]), filtered)
                }
            }
        }
        canvas.restore()
    }
}

/**
 * Where crop mode puts the whole photograph in a view: inside [area]
 * (view pixels), the turned photograph fitted. [scale] is view pixels per
 * photograph pixel, the session's frame.
 */
data class CropFrame(val left: Float, val top: Float, val width: Float, val height: Float, val scale: Double) {
    fun toView(p: Pt) = Pt(left + p.x * scale, top + p.y * scale)
    fun toView(r: CropRect) = CropRect(left + r.x * scale, top + r.y * scale, r.width * scale, r.height * scale)

    companion object {
        fun of(area: RectF, session: CropSession): CropFrame {
            val (pw, ph) = session.photo
            val fit = min(area.width() / pw, area.height() / ph)
            val w = (pw * fit).toFloat()
            val h = (ph * fit).toFloat()
            return CropFrame(area.left + (area.width() - w) / 2, area.top + (area.height() - h) / 2, w, h, fit)
        }
    }
}

/**
 * Crop mode (crop-editor.js refresh): the whole photograph straightened
 * and turned, darkened outside the crop, with the crop's frame, corners and
 * thirds while dragging (ninths while straightening).
 */
fun DrawScope.drawCropping(bitmap: Bitmap, session: CropSession, frame: CropFrame, density: Float) {
    val canvas = drawContext.canvas.nativeCanvas
    val settings = session.settings
    val (shown, matrix) = Crop.displayMatrix(settings, session.rotation, bitmap.width, bitmap.height, editing = true)
    matrix.postScale(frame.width / shown.first, frame.height / shown.second)
    matrix.postTranslate(frame.left, frame.top)
    canvas.drawBitmap(bitmap, matrix, filtered)

    val (pw, ph) = session.photo
    val polygon = CropRect(0.0, 0.0, pw, ph).corners.map { frame.toView(Crop.turn(it, settings.angle, Pt(pw / 2, ph / 2))) }
    val c = frame.toView(session.shown)
    val shade = Path().apply {
        fillType = Path.FillType.EVEN_ODD
        polygon.forEachIndexed { i, p -> if (i == 0) moveTo(p.x.toFloat(), p.y.toFloat()) else lineTo(p.x.toFloat(), p.y.toFloat()) }
        close()
        addRect(c.x.toFloat(), c.y.toFloat(), c.right.toFloat(), c.bottom.toFloat(), Path.Direction.CW)
    }
    canvas.drawPath(shade, Paint().apply { color = 0x8C000000.toInt() })
    val lines = if (session.intended != null) 9 else if (session.dragging) 3 else 0
    val thin = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0x80FFFFFF.toInt(); strokeWidth = 0.5f * density; style = Paint.Style.STROKE }
    for (i in 1 until lines) {
        val x = (c.x + c.width * i / lines).toFloat()
        val y = (c.y + c.height * i / lines).toFloat()
        canvas.drawLine(x, c.y.toFloat(), x, c.bottom.toFloat(), thin)
        canvas.drawLine(c.x.toFloat(), y, c.right.toFloat(), y, thin)
    }
    val edge = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0xFFFFFFFF.toInt(); strokeWidth = density; style = Paint.Style.STROKE }
    canvas.drawRect(c.x.toFloat(), c.y.toFloat(), c.right.toFloat(), c.bottom.toFloat(), edge)
    val length = minOf(22 * density.toDouble(), c.width / 2, c.height / 2).toFloat()
    val corner = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xFFFFFFFF.toInt(); strokeWidth = 3 * density; style = Paint.Style.STROKE
        setShadowLayer(3 * density, 0f, 0f, 0xFF000000.toInt())
    }
    for ((x, y, dx, dy) in listOf(
        listOf(c.x, c.y, 1.0, 1.0), listOf(c.right, c.y, -1.0, 1.0), listOf(c.x, c.bottom, 1.0, -1.0), listOf(c.right, c.bottom, -1.0, -1.0),
    )) {
        val path = Path().apply {
            moveTo((x + dx * length).toFloat(), y.toFloat())
            lineTo(x.toFloat(), y.toFloat())
            lineTo(x.toFloat(), (y + dy * length).toFloat())
        }
        canvas.drawPath(path, corner)
    }
}
