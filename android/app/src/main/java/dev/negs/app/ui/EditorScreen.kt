package dev.negs.app.ui

import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.RotateLeft
import androidx.compose.material.icons.automirrored.filled.RotateRight
import androidx.compose.material.icons.filled.Compare
import androidx.compose.material.icons.filled.Crop
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.IosShare
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.negs.app.BorderSettings
import dev.negs.app.BorderStyle
import dev.negs.app.Editor
import dev.negs.app.tr
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** The editor on a phone: the photograph above, the inspector's sheet below. */
@Composable
fun EditorScreen(editor: Editor) {
    val context = LocalContext.current
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let(editor::open)
    }
    val open = { pick.launch(arrayOf("image/*", "application/octet-stream")) }
    val saveLut = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        uri?.let(editor::exportLut)
    }
    val exportLut = { saveLut.launch(editor.lutName()) }

    androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides Palette.fg) {
    Column(Modifier.fillMaxSize().background(Palette.bg)) {
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            PhotoView(editor, Modifier.fillMaxSize())
            if (editor.photo == null) EmptyState(editor, open, Modifier.align(Alignment.Center))
            if (editor.busy || editor.exporting) {
                LinearProgressIndicator(
                    Modifier.fillMaxWidth().height(2.dp).statusBarsPadding().align(Alignment.TopCenter),
                    color = Palette.accent, trackColor = Color.Transparent,
                )
            }
            val session = editor.cropSession
            if (session != null) {
                StraightenDial(session, Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp))
                CropBar(editor, Modifier.align(Alignment.TopCenter).statusBarsPadding())
            } else {
                TopBar(editor, open, exportLut, Modifier.align(Alignment.TopCenter).statusBarsPadding())
            }
            editor.message?.let { message ->
                MessageCard(message, onDismiss = editor::dismissMessage, onShare = { uri ->
                    context.startActivity(editor.shareIntent(uri))
                }, Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 56.dp))
            }
            if (editor.status.isNotEmpty() && editor.cropSession == null) {
                Text(
                    editor.status, color = Palette.faint, fontSize = 11.sp, fontFamily = FontFamily.Monospace,
                    modifier = Modifier.align(Alignment.BottomStart).padding(horizontal = 12.dp, vertical = 6.dp),
                )
            }
        }
        Sheet(editor)
    }
    }
}

@Composable
private fun TopBar(editor: Editor, open: () -> Unit, exportLut: () -> Unit, modifier: Modifier) {
    var menu by remember { mutableStateOf(false) }
    var settings by remember { mutableStateOf(false) }
    Row(modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        ToolButton(onClick = open) { Icon(Icons.Default.FileOpen, tr("Open photo")) }
        if (editor.photo != null) {
            ToolButton(onClick = editor::beginCrop) { Icon(Icons.Default.Crop, tr("Crop and straighten")) }
            // Show Original while held (macOS square.split.diagonal.fill).
            Box(
                Modifier.padding(4.dp).size(40.dp).clip(CircleShape)
                    .background(if (editor.showingOriginal) Palette.accent.copy(alpha = .25f) else Palette.panel.copy(alpha = .85f))
                    .pointerInput(Unit) {
                        detectTapGestures(onPress = {
                            editor.holdOriginal(true)
                            tryAwaitRelease()
                            editor.holdOriginal(false)
                        })
                    },
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Default.Compare, tr("Show Original"), tint = if (editor.showingOriginal) Palette.accent else Palette.muted) }
        }
        Spacer(Modifier.weight(1f))
        // The wordmark while there's room: before a photograph's tools.
        if (editor.photo == null) {
            Text(
                "Negs", color = Color(0xFFFFF6EA), fontSize = 26.sp, fontStyle = FontStyle.Italic, fontFamily = FontFamily.Serif,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
            Spacer(Modifier.weight(1f))
        }
        if (editor.photo != null) {
            Button(
                onClick = editor::export, enabled = !editor.exporting && editor.frame != null,
                colors = ButtonDefaults.buttonColors(containerColor = Palette.accent, contentColor = Palette.bg),
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
            ) {
                if (editor.exporting) CircularProgressIndicator(Modifier.size(16.dp), color = Palette.bg, strokeWidth = 2.dp)
                else Icon(Icons.Default.IosShare, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text(tr("Export"))
            }
        }
        Box {
            ToolButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, tr("More")) }
            DropdownMenu(menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(
                    text = { Text(tr(if (editor.advanced) "Hide advanced controls" else "Show advanced controls")) },
                    onClick = { editor.advanced = !editor.advanced; menu = false },
                )
                DropdownMenuItem(text = { Text(tr("Reset adjustments")) }, onClick = { editor.resetAdjustments(); menu = false })
                DropdownMenuItem(
                    text = { Text(tr("Export LUT…")) }, enabled = editor.catalog != null,
                    onClick = { menu = false; exportLut() },
                )
                DropdownMenuItem(text = { Text(tr("Settings…")) }, onClick = { menu = false; settings = true })
            }
        }
        if (settings) SettingsDialog(editor) { settings = false }
    }
}

/** Crop mode's bar (web #crop-cancel, #crop-done): turn, then apply or cancel. */
@Composable
private fun CropBar(editor: Editor, modifier: Modifier) {
    Row(modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        OutlinedButton(onClick = { editor.endCrop(false) }) { Text(tr("Cancel"), color = Palette.fg) }
        Spacer(Modifier.weight(1f))
        ToolButton(onClick = { editor.rotate(-1) }) { Icon(Icons.AutoMirrored.Filled.RotateLeft, tr("Rotate counterclockwise")) }
        ToolButton(onClick = { editor.rotate(1) }) { Icon(Icons.AutoMirrored.Filled.RotateRight, tr("Rotate clockwise")) }
        Spacer(Modifier.weight(1f))
        Button(
            onClick = { editor.endCrop(true) },
            colors = ButtonDefaults.buttonColors(containerColor = Palette.accent, contentColor = Palette.bg),
        ) { Text(tr("Done")) }
    }
}

@Composable
private fun ToolButton(onClick: () -> Unit, content: @Composable () -> Unit) {
    IconButton(
        onClick = onClick,
        modifier = Modifier.padding(4.dp).size(40.dp).clip(CircleShape).background(Palette.panel.copy(alpha = .85f)),
    ) { content() }
}

@Composable
private fun EmptyState(editor: Editor, open: () -> Unit, modifier: Modifier) {
    Column(modifier.padding(24.dp).width(320.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        val error = editor.startupError
        when {
            error != null -> {
                Text(tr("The engine didn’t start"), color = Palette.danger, fontWeight = FontWeight.SemiBold)
                Text(error, color = Palette.muted, fontSize = 13.sp, modifier = Modifier.padding(top = 6.dp))
            }
            editor.catalog == null -> {
                CircularProgressIndicator(color = Palette.accent)
                Text(editor.status, color = Palette.muted, modifier = Modifier.padding(top = 12.dp))
            }
            else -> {
                Text(tr("Develop a photo on film"), fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
                Text(
                    tr("Open a RAW, JPEG, PNG or TIFF. It stays on your phone."),
                    color = Palette.muted, fontSize = 13.sp, modifier = Modifier.padding(top = 4.dp, bottom = 16.dp),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = open, colors = ButtonDefaults.buttonColors(containerColor = Palette.accent, contentColor = Palette.bg)) {
                        Text(tr("Open photo"))
                    }
                    OutlinedButton(onClick = editor::openSample) { Text(tr("Try a sample photo")) }
                }
            }
        }
    }
}

@Composable
private fun MessageCard(message: Editor.Message, onDismiss: () -> Unit, onShare: (android.net.Uri) -> Unit, modifier: Modifier) {
    LaunchedEffect(message) {
        if (!message.error && message.share == null && !message.text.endsWith("…")) {
            kotlinx.coroutines.delay(4000)
            onDismiss()
        }
    }
    Surface(
        modifier.padding(horizontal = 12.dp),
        shape = RoundedCornerShape(8.dp),
        color = Palette.panel,
        border = androidx.compose.foundation.BorderStroke(1.dp, if (message.error) Palette.danger else Palette.line),
        shadowElevation = 6.dp,
    ) {
        Row(Modifier.padding(start = 12.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                message.text, color = if (message.error) Palette.danger else Palette.fg, fontSize = 13.sp,
                modifier = Modifier.weight(1f, fill = false).padding(vertical = 10.dp),
            )
            message.share?.let { uri -> TextButton(onClick = { onShare(uri); onDismiss() }) { Text(tr("Share…"), color = Palette.accent) } }
            TextButton(onClick = onDismiss) { Text(tr("OK"), color = Palette.muted) }
        }
    }
}

/**
 * The photograph, fitted, with pinch to zoom and drag to pan; double tap
 * zooms in and back. It shows cropped, turned and in its border, as it will
 * export; while cropping, whole, under the crop's overlay.
 */
@Composable
private fun PhotoView(editor: Editor, modifier: Modifier) {
    val frame = editor.frame
    val shownBitmap: Bitmap? = if (editor.showingOriginal) editor.original ?: frame?.bitmap else frame?.bitmap
    val session = editor.cropSession
    if (session != null && frame != null) {
        val top = with(LocalDensity.current) { androidx.compose.foundation.layout.WindowInsets.Companion.statusBars.getTop(this).toFloat() }
        CropOverlay(editor, session, frame.bitmap, top, modifier)
        return
    }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    var canvasSize by remember { mutableStateOf(IntSize.Zero) }
    LaunchedEffect(editor.photo) { scale = 1f; offset = Offset.Zero }
    LaunchedEffect(canvasSize, scale) {
        if (canvasSize.width > 0 && canvasSize.height > 0) {
            editor.updateViewport(canvasSize.width, canvasSize.height, scale)
        }
    }
    val border = editor.border
    val out = editor.outputSize()
    // The film frame's or strip's texture, loaded once it's wanted.
    val textureKey = out?.let { (w, h) -> editor.textureKey(w, h) }
    LaunchedEffect(textureKey, editor.borderTexture) { textureKey?.let(editor::wantTexture) }
    val texture = editor.borderTexture?.takeIf { it.first == textureKey }?.second
    Box(
        modifier
            .pointerInput(Unit) {
                detectTransformGestures { _, pan, zoom, _ ->
                    scale = (scale * zoom).coerceIn(1f, 8f)
                    offset = if (scale == 1f) Offset.Zero else offset + pan
                }
            }
            .pointerInput(Unit) {
                detectTapGestures(onDoubleTap = { at ->
                    if (scale > 1f) { scale = 1f; offset = Offset.Zero } else {
                        scale = 3f
                        offset = (Offset(size.width / 2f, size.height / 2f) - at) * 2f
                    }
                })
            },
    ) {
        // Measure the padded photo area even before the first frame arrives so
        // the first settled render already targets the exact fitted viewport.
        Box(
            Modifier.fillMaxSize().statusBarsPadding().padding(start = 12.dp, end = 12.dp, top = 56.dp, bottom = 12.dp)
                .onSizeChanged { canvasSize = it },
        ) {
            if (shownBitmap != null && out != null) {
                Canvas(
                    Modifier.fillMaxSize()
                        .graphicsLayer(scaleX = scale, scaleY = scale, translationX = offset.x, translationY = offset.y),
                ) {
                    drawPresented(shownBitmap, editor.crop, editor.rotation, out, border, texture)
                }
            }
        }
    }
}

private enum class Snap { Low, Mid, High }

/**
 * The inspector's sheet (web/app.js buildSheet): its tabs, at one of three
 * heights. Drag the grabber between them, or tap it to step.
 */
@Composable
private fun Sheet(editor: Editor) {
    var snap by rememberSaveable { mutableStateOf(Snap.Mid) }
    var tab by rememberSaveable { mutableStateOf("Stock") }
    // A preset arrived from another app: show it among the presets.
    var presetsSeen by rememberSaveable { mutableStateOf(0) }
    LaunchedEffect(editor.presetsShown) {
        if (editor.presetsShown != presetsSeen) {
            presetsSeen = editor.presetsShown
            tab = "Presets"
            if (snap == Snap.Low) snap = Snap.Mid
        }
    }
    var dragged by remember { mutableFloatStateOf(0f) }
    BoxWithConstraints {
        val screen = maxHeight
        val density = LocalDensity.current
        val tabsOnly = 92.dp
        val target = when (snap) {
            Snap.Low -> tabsOnly
            Snap.Mid -> screen * 0.45f
            Snap.High -> screen * 0.8f
        }
        val height by animateDpAsState(
            (target + with(density) { (-dragged).toDp() }).coerceIn(tabsOnly, screen * 0.9f), label = "sheet",
        )
        Surface(color = Palette.panel, modifier = Modifier.fillMaxWidth().height(height)) {
            Column(Modifier.navigationBarsPadding()) {
                Box(
                    Modifier.fillMaxWidth().height(22.dp)
                        .pointerInput(Unit) {
                            detectTapGestures { snap = Snap.entries[(snap.ordinal + 1) % Snap.entries.size] }
                        }
                        .pointerInput(Unit) {
                            detectVerticalDragGestures(
                                onDragEnd = {
                                    val now = with(density) { (target.toPx() - dragged) }
                                    val heights = Snap.entries.associateWith { s ->
                                        with(density) {
                                            when (s) { Snap.Low -> tabsOnly; Snap.Mid -> screen * 0.45f; Snap.High -> screen * 0.8f }.toPx()
                                        }
                                    }
                                    snap = heights.minBy { kotlin.math.abs(it.value - now) }.key
                                    dragged = 0f
                                },
                            ) { _, dy -> dragged += dy }
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Box(Modifier.size(36.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(Palette.line))
                }
                val cropping = editor.cropSession
                if (cropping != null) {
                    LaunchedEffect(Unit) { if (snap == Snap.Low) snap = Snap.Mid }
                    CropInspector(editor, cropping, Modifier.weight(1f))
                    return@Column
                }
                val tabs = Inspector.tabs(editor)
                if (tab !in tabs) tab = "Stock"
                ScrollableTabRow(
                    selectedTabIndex = tabs.indexOf(tab).coerceAtLeast(0),
                    containerColor = Palette.panel, contentColor = Palette.fg, edgePadding = 8.dp,
                    divider = { HorizontalDivider(color = Palette.line) },
                ) {
                    tabs.forEach { t ->
                        Tab(
                            selected = t == tab,
                            onClick = { tab = t; if (snap == Snap.Low) snap = Snap.Mid },
                            text = { Text(tr(t), color = if (t == tab) Palette.fg else Palette.muted) },
                        )
                    }
                }
                Inspector.Content(editor, tab, Modifier.weight(1f))
            }
        }
    }
}
