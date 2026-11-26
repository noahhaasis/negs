package dev.negs.app.ui

import android.graphics.Bitmap
import android.graphics.RectF
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.systemGestureExclusion
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.LocalTextStyle
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.negs.app.Crop
import dev.negs.app.CropSession
import dev.negs.app.Editor
import dev.negs.app.Pt
import kotlin.math.abs
import kotlin.math.max

/** The view's room for the photograph in crop mode: below the top bar, above the dial. */
private fun cropArea(width: Float, height: Float, density: Float, top: Float) =
    RectF(28 * density, top + 64 * density, width - 28 * density, height - 84 * density)

/**
 * Crop mode's photograph and overlay (crop-editor.js): drag a corner or an
 * edge to resize, inside to move. The crop stays on the straightened photograph.
 */
@Composable
fun CropOverlay(editor: Editor, session: CropSession, bitmap: Bitmap, topInset: Float, modifier: Modifier) {
    val density = LocalDensity.current.density
    val bitmapState = rememberUpdatedState(bitmap)
    val insetState = rememberUpdatedState(topInset)
    // Back while cropping cancels it, as Escape does on the web.
    androidx.activity.compose.BackHandler { editor.endCrop(false) }
    Canvas(
        modifier.fillMaxSize()
            // The crop's side handles sit by the screen's edges, where the
            // back gesture starts: keep them for the crop.
            .systemGestureExclusion { coordinates ->
                val frame = CropFrame.of(cropArea(coordinates.size.width.toFloat(), coordinates.size.height.toFloat(), density, insetState.value), session)
                val c = frame.toView(session.shown)
                androidx.compose.ui.geometry.Rect(0f, c.y.toFloat() - 30 * density, coordinates.size.width.toFloat(), c.bottom.toFloat() + 30 * density)
            }
            .pointerInput(session) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false)
                val frame = CropFrame.of(cropArea(size.width.toFloat(), size.height.toFloat(), density, insetState.value), session)
                val crop = frame.toView(session.shown)
                val handle = Crop.handleAt(
                    Pt(down.position.x.toDouble(), down.position.y.toDouble()), crop,
                    edge = 18.0 * density, corner = 28.0 * density,
                ) ?: return@awaitEachGesture
                down.consume()
                val start = session.shown
                val at = down.position
                session.dragging = true
                try {
                    while (true) {
                        val event = awaitPointerEvent()
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        if (change.positionChange() == androidx.compose.ui.geometry.Offset.Zero) continue
                        change.consume()
                        val dx = (change.position.x - at.x) / frame.scale
                        val dy = (change.position.y - at.y) / frame.scale
                        val angle = session.settings.angle
                        val photo = session.photo
                        val bounds = Crop.bounds(photo, angle)
                        session.shown = if (handle.first == 0 && handle.second == 0) {
                            val across = Crop.constrain(start, Crop.move(start, dx, 0.0, bounds), angle, photo)
                            Crop.constrain(across, Crop.move(across, 0.0, dy, bounds), angle, photo)
                        } else {
                            val edge = Pt(
                                (if (handle.first < 0) start.x else start.right) + dx,
                                (if (handle.second < 0) start.y else start.bottom) + dy,
                            )
                            Crop.constrain(start, Crop.resize(start, handle, edge, session.ratio, bounds, 40.0 * density / frame.scale), angle, photo)
                        }
                    }
                } finally {
                    session.dragging = false
                }
            }
        },
    ) {
        val frame = CropFrame.of(cropArea(size.width, size.height, density, insetState.value), session)
        drawCropping(bitmapState.value, session, frame, density)
    }
}

/**
 * The straightening dial under the photograph (crop-editor.js): seven
 * points per degree, drag to level, double tap to reset.
 */
@Composable
fun StraightenDial(session: CropSession, modifier: Modifier) {
    val angle = session.settings.angle
    Column(
        modifier.widthIn(max = 340.dp).fillMaxWidth().padding(horizontal = 16.dp)
            .background(Palette.panel.copy(alpha = .9f), RoundedCornerShape(12.dp))
            .border(1.dp, Palette.line, RoundedCornerShape(12.dp))
            .padding(horizontal = 14.dp, vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("%.1f°".format(angle), fontSize = 12.sp, color = Palette.fg)
        var from by remember { mutableStateOf(0.0) }
        var moved by remember { mutableStateOf(0f) }
        val density = LocalDensity.current.density
        Canvas(
            Modifier.fillMaxWidth().height(24.dp)
                .pointerInput(session) {
                    detectHorizontalDragGestures(
                        onDragStart = { from = session.settings.angle; moved = 0f },
                        onDragEnd = { session.endStraighten() },
                        onDragCancel = { session.endStraighten() },
                    ) { change, dx ->
                        change.consume()
                        moved += dx
                        var next = from - moved / density / 7
                        if (abs(next) < 0.4) next = 0.0
                        session.straighten(next)
                    }
                }
                .pointerInput(session) {
                    detectTapGestures(onDoubleTap = { session.straighten(0.0); session.endStraighten() })
                },
        ) {
            val dw = size.width
            val dh = size.height
            val pt = density
            for (i in -45..45) {
                val x = dw / 2 + ((i - angle) * 7 * pt).toFloat()
                val h = if (i == 0) dh else if (i % 5 == 0) dh * 0.7f else dh * 0.4f
                val fade = max(0f, 1 - ((x - dw / 2) / (dw / 2)).let { it * it }) * (if (i % 5 == 0) 0.75f else 0.4f)
                drawLine(Color(0xFFE8E6E3).copy(alpha = fade), androidx.compose.ui.geometry.Offset(x, (dh - h) / 2), androidx.compose.ui.geometry.Offset(x, (dh + h) / 2), strokeWidth = pt)
            }
            drawLine(Palette.accent, androidx.compose.ui.geometry.Offset(dw / 2, 0f), androidx.compose.ui.geometry.Offset(dw / 2, dh), strokeWidth = 2 * pt)
        }
    }
}

/** The crop's inspector, in place of the tabs while cropping (index.html #crop-inspector). */
@Composable
fun CropInspector(editor: Editor, session: CropSession, modifier: Modifier) {
    val s = session.settings
    LazyColumn(modifier.fillMaxWidth(), contentPadding = PaddingValues(14.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Aspect ratio", color = Palette.muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
                var open by remember { mutableStateOf(false) }
                Box {
                    OutlinedButton(onClick = { open = true }) { Text(Crop.ASPECTS.first { it.first == s.aspect }.second, color = Palette.fg) }
                    DropdownMenu(open, onDismissRequest = { open = false }) {
                        Crop.ASPECTS.forEach { (id, title) ->
                            DropdownMenuItem(text = { Text(title) }, onClick = { session.choose(id); open = false })
                        }
                    }
                }
            }
        }
        if (s.aspect == "custom") item {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                RatioField("Width", s.custom.first) { session.setCustom(0, it) }
                Text("×", color = Palette.muted)
                RatioField("Height", s.custom.second) { session.setCustom(1, it) }
            }
        }
        item {
            val fixed = s.aspect == "freeform" || s.aspect == "square"
            val portrait = if (s.aspect == "custom") s.custom.second > s.custom.first else session.portrait
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("Landscape" to false, "Portrait" to true).forEach { (label, value) ->
                    val on = portrait == value && !fixed
                    OutlinedButton(
                        onClick = { session.orient(value) }, enabled = !fixed, modifier = Modifier.weight(1f),
                        border = androidx.compose.foundation.BorderStroke(1.dp, if (on) Palette.accent else Palette.line),
                    ) { Text(label, color = if (on) Palette.accent else Palette.fg) }
                }
            }
        }
        item {
            Column {
                // Fixed height and a kept Reset slot, as Inspector's SliderRow.
                Row(Modifier.height(28.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Straighten", color = Palette.muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
                    Text(
                        "%.1f°".format(s.angle), color = Palette.muted, fontSize = 12.sp,
                        style = LocalTextStyle.current.copy(fontFeatureSettings = "tnum"),
                    )
                    val modified = s.angle != 0.0
                    Text(
                        "Reset", color = Palette.faint, fontSize = 11.sp,
                        modifier = Modifier.alpha(if (modified) 1f else 0f)
                            .clickable(enabled = modified) { session.straighten(0.0); session.endStraighten() }
                            .padding(horizontal = 8.dp, vertical = 6.dp),
                    )
                }
                Slider(
                    value = s.angle.toFloat(), valueRange = -45f..45f,
                    onValueChange = { session.straighten(it.toDouble()) },
                    onValueChangeFinished = { session.endStraighten() },
                    colors = SliderDefaults.colors(thumbColor = Palette.fg, activeTrackColor = Palette.accent, inactiveTrackColor = Palette.field),
                    modifier = Modifier.height(32.dp),
                )
            }
        }
        item {
            val size = editor.sourceSize()?.let { (w, h) -> Crop.outputSize(s, session.rotation, w, h) }
            Row {
                Text("Size", color = Palette.muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
                Text(size?.let { "${it.first} × ${it.second} px" } ?: "", color = Palette.fg, fontSize = 13.sp)
            }
        }
        item { OutlinedButton(onClick = session::reset, modifier = Modifier.fillMaxWidth()) { Text("Reset Crop", color = Palette.fg) } }
        item {
            Text(
                "Drag corners or edges to resize, or drag inside to move. Drag the dial below the photograph to level it. Borders are added after cropping.",
                color = Palette.faint, fontSize = 12.sp,
            )
        }
    }
}

@Composable
private fun RatioField(label: String, value: Double, onChange: (Double) -> Unit) {
    var text by remember { mutableStateOf(formatRatio(value)) }
    // Changed from elsewhere (turning swaps the ratio): show it.
    var last by remember { mutableStateOf(value) }
    if (value != last) {
        last = value
        if (text.toDoubleOrNull() != value) text = formatRatio(value)
    }
    OutlinedTextField(
        value = text,
        onValueChange = { t -> text = t; t.toDoubleOrNull()?.takeIf { it > 0 && it.isFinite() }?.let(onChange) },
        label = { Text(label, fontSize = 11.sp) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
        textStyle = TextStyle(fontSize = 13.sp, color = Palette.fg),
        modifier = Modifier.width(96.dp),
    )
}

private fun formatRatio(v: Double) = if (v == v.toLong().toDouble()) v.toLong().toString() else "%.1f".format(v)
