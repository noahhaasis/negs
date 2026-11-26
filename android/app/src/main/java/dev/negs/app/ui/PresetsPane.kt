package dev.negs.app.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.negs.app.Editor
import dev.negs.app.PresetFile
import dev.negs.app.PresetName
import dev.negs.app.PresetStatus

/**
 * The Presets tab (macOS PresetInspector): the presets as tiles in two
 * columns, each the photograph developed through it, its name, and its film
 * and paper. A tap applies one; the one applied or saved last is ringed.
 * Holding a tile, or its ⋮, offers rename, share, save, move and delete.
 */
@Composable
fun PresetsPane(editor: Editor, modifier: Modifier) {
    val context = LocalContext.current
    val import = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        editor.importPresets(uris)
    }
    var saving by remember { mutableStateOf<PresetFile?>(null) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        val preset = saving
        saving = null
        if (uri != null && preset != null) editor.savePresetFile(preset, uri)
    }
    var naming by remember { mutableStateOf<Naming?>(null) }
    var deleting by remember { mutableStateOf<PresetFile?>(null) }
    val enabled = editor.canChangePresets
    val importFiles = { import.launch(arrayOf("application/json", "text/plain", "application/octet-stream")) }

    LazyColumn(modifier.fillMaxWidth(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "header") {
            Row(Modifier.padding(start = 14.dp, end = 8.dp, top = 10.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedButton(
                    onClick = { naming = Naming(null, editor.suggestedPresetName()) }, enabled = enabled,
                    contentPadding = PaddingValues(horizontal = 12.dp),
                ) {
                    Icon(Icons.Default.Add, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("New preset", color = if (enabled) Palette.fg else Palette.faint)
                }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = importFiles, enabled = enabled) {
                    Icon(Icons.Default.FileOpen, null, Modifier.size(18.dp), tint = Palette.muted)
                    Spacer(Modifier.width(6.dp))
                    Text("Import…", color = if (enabled) Palette.muted else Palette.faint)
                }
            }
        }
        if (editor.presets.isEmpty()) {
            item(key = "empty") {
                Text(
                    "A preset keeps a film, a paper and the Lab settings, to use again on other photos. " +
                        "Save this look with New preset, or import presets made in Negs or spektrafilm.",
                    color = Palette.muted, fontSize = 13.sp, lineHeight = 19.sp, textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 28.dp, vertical = 28.dp),
                )
            }
        }
        items(editor.presets.chunked(2), key = { row -> "p:" + row.joinToString { it.file.path } }) { row ->
            Row(Modifier.padding(horizontal = 14.dp, vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                row.forEach { preset ->
                    PresetTile(
                        editor, preset, Modifier.weight(1f),
                        onRename = { naming = Naming(preset, preset.name) },
                        onShare = { editor.sharePreset(preset) { context.startActivity(it) } },
                        onSave = { saving = preset; save.launch("${preset.name}.json") },
                        onDelete = { deleting = preset },
                    )
                }
                if (row.size == 1) Spacer(Modifier.weight(1f))
            }
        }
        if (editor.presets.isNotEmpty()) {
            item(key = "foot") {
                Text(
                    "tap to apply · hold for more", color = Palette.faint, fontSize = 11.sp, textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(top = 14.dp),
                )
            }
        }
    }

    naming?.let { n ->
        NameDialog(
            title = if (n.preset == null) "New preset" else "Rename preset",
            initial = n.name,
            confirm = if (n.preset == null) "Save" else "Rename",
            problem = { editor.presetNameProblem(it, n.preset) },
            onDone = { name ->
                if (n.preset == null) editor.newPreset(name)
                else if (name != n.preset.name) editor.renamePreset(n.preset, name)
                naming = null
            },
            onCancel = { naming = null },
        )
    }
    deleting?.let { preset ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text("Delete “${preset.name}”?") },
            text = { Text("The preset is removed from Negs. The photo you’re editing keeps its settings.") },
            confirmButton = { TextButton(onClick = { editor.deletePreset(preset); deleting = null }) { Text("Delete", color = Palette.danger) } },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text("Cancel", color = Palette.fg) } },
            containerColor = Palette.panel,
        )
    }
}

/** A name being chosen: for a new preset ([preset] null) or to rename one. */
private data class Naming(val preset: PresetFile?, val name: String)

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PresetTile(
    editor: Editor, preset: PresetFile, modifier: Modifier,
    onRename: () -> Unit, onShare: () -> Unit, onSave: () -> Unit, onDelete: () -> Unit,
) {
    val status = editor.presetStatus[preset]
    val valid = status as? PresetStatus.Valid
    val ringed = editor.appliedPreset == PresetName.key(preset.name)
    val thumbnail = editor.presetThumbnails[preset.file.path]
    val longSide = with(LocalDensity.current) { 220.dp.roundToPx() }
    // Asked for as the tile shows, and again once what it shows is stale.
    val wanted = editor.presetTileKey(preset)
    LaunchedEffect(wanted) { if (wanted != null) editor.wantPresetThumbnail(preset, longSide) }
    var menu by remember { mutableStateOf(false) }
    val index = editor.presets.indexOf(preset)
    val catalog = editor.catalog

    Column(
        modifier.combinedClickable(
            enabled = editor.canChangePresets,
            onClick = { if (valid != null) editor.applyPreset(preset) else menu = true },
            onLongClick = { menu = true },
        ),
    ) {
        Box(
            Modifier.fillMaxWidth().aspectRatio(1.5f).clip(RoundedCornerShape(5.dp)).background(Palette.field)
                .then(if (ringed) Modifier.border(2.dp, Palette.accent, RoundedCornerShape(5.dp)) else Modifier),
            contentAlignment = Alignment.Center,
        ) {
            when {
                status is PresetStatus.Invalid -> Text(
                    status.reason, color = Palette.faint, fontSize = 11.sp, lineHeight = 14.sp, textAlign = TextAlign.Center,
                    maxLines = 5, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(10.dp),
                )
                thumbnail != null -> {
                    val rotation = editor.rotation
                    val image = remember(thumbnail.second, rotation) { editor.turned(thumbnail.second).asImageBitmap() }
                    Image(
                        image, preset.name, contentScale = ContentScale.Crop,
                        alpha = if (thumbnail.first == wanted) 1f else .55f,
                        modifier = Modifier.fillMaxWidth().aspectRatio(1.5f),
                    )
                }
            }
        }
        Row(verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f).padding(top = 5.dp)) {
                Text(
                    preset.name, fontFamily = FontFamily.Serif, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    color = if (status is PresetStatus.Invalid) Palette.faint else Palette.fg,
                )
                if (valid != null) {
                    Text(
                        catalog?.stock(valid.film)?.name ?: valid.film, color = Palette.muted, fontSize = 11.sp,
                        maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        buildAnnotatedString {
                            if (valid.scan) {
                                withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append("scanned directly") }
                            } else {
                                withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append("printed on ") }
                                append(catalog?.stock(valid.paper)?.name ?: valid.paper)
                            }
                        },
                        color = Palette.faint, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Box {
                Icon(
                    Icons.Default.MoreVert, "More for ${preset.name}", tint = Palette.faint,
                    modifier = Modifier.clickable(enabled = editor.canChangePresets) { menu = true }.padding(4.dp).size(18.dp),
                )
                DropdownMenu(menu, onDismissRequest = { menu = false }) {
                    if (valid != null) {
                        DropdownMenuItem(text = { Text("Apply") }, onClick = { menu = false; editor.applyPreset(preset) })
                    }
                    DropdownMenuItem(text = { Text("Rename…") }, onClick = { menu = false; onRename() })
                    if (valid != null) {
                        DropdownMenuItem(text = { Text("Share…") }, onClick = { menu = false; onShare() })
                        DropdownMenuItem(text = { Text("Save as file…") }, onClick = { menu = false; onSave() })
                    }
                    if (index > 0) DropdownMenuItem(text = { Text("Move earlier") }, onClick = { menu = false; editor.movePreset(preset, -1) })
                    if (index in 0 until editor.presets.lastIndex) DropdownMenuItem(text = { Text("Move later") }, onClick = { menu = false; editor.movePreset(preset, 1) })
                    DropdownMenuItem(text = { Text("Delete…", color = Palette.danger) }, onClick = { menu = false; onDelete() })
                }
            }
        }
    }
}

/** A preset's name: checked as it's typed, the reason shown under the field. */
@Composable
private fun NameDialog(
    title: String, initial: String, confirm: String,
    problem: (String) -> String?, onDone: (String) -> Unit, onCancel: () -> Unit,
) {
    var field by remember { mutableStateOf(TextFieldValue(initial, TextRange(0, initial.length))) }
    val reason = problem(field.text).takeIf { field.text != initial || it != null }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(title) },
        text = {
            Column {
                OutlinedTextField(
                    field, onValueChange = { field = it }, singleLine = true, isError = reason != null,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { if (problem(field.text) == null) onDone(field.text) }),
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                )
                if (reason != null) Text(reason, color = Palette.danger, fontSize = 12.sp, modifier = Modifier.padding(top = 6.dp))
            }
        },
        confirmButton = {
            TextButton(onClick = { onDone(field.text) }, enabled = problem(field.text) == null) {
                Text(confirm, fontWeight = FontWeight.SemiBold, color = if (problem(field.text) == null) Palette.accent else Palette.faint)
            }
        },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel", color = Palette.fg) } },
        containerColor = Palette.panel,
    )
}
