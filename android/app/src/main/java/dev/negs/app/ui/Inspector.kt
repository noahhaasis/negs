package dev.negs.app.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.StarBorder
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.negs.app.Action
import dev.negs.app.BorderControl
import dev.negs.app.BorderField
import dev.negs.app.BorderSettings
import dev.negs.app.BorderStyle
import dev.negs.app.Choice
import dev.negs.app.ColorFilters
import dev.negs.app.Control
import dev.negs.app.Controls
import dev.negs.app.Editor
import dev.negs.app.ExposureOffset
import dev.negs.app.MatchNote
import dev.negs.app.MatchSwitch
import dev.negs.app.Group
import dev.negs.app.Section
import dev.negs.app.Slider
import dev.negs.app.Stock
import dev.negs.app.StockLane
import dev.negs.app.Toggle
import dev.negs.app.bool
import dev.negs.app.json
import dev.negs.app.number
import dev.negs.app.text

/** The sheet's tabs and their contents (web/app.js TABS, buildControls). */
object Inspector {
    private val sections = Controls.PREP + Controls.LAB + Controls.OUTPUT

    fun tabs(editor: Editor): List<String> {
        val order = listOf("Prep", "Stock") + (Controls.LAB + Controls.OUTPUT).map { it.tab }
        return order.distinct().filter { tab ->
            // Scan's sections are all advanced: it shows with them, or once changed.
            tab != "Scan" || editor.advanced || sections.filter { it.tab == "Scan" }.any { s -> s.groups.any { g -> g.controls.any { modified(editor, it) } } }
        }
    }

    @Composable
    fun Content(editor: Editor, tab: String, modifier: Modifier) {
        val catalog = editor.catalog
        if (catalog == null) {
            Box(modifier)
            return
        }
        val open = remember { mutableStateMapOf<String, Boolean>() }
        LazyColumn(modifier.fillMaxWidth(), contentPadding = androidx.compose.foundation.layout.PaddingValues(bottom = 24.dp)) {
            if (tab == "Stock") stockItems(editor)
            else sections.filter { it.tab == tab && (!it.raw || editor.raw) }.forEach { section -> sectionItems(editor, section, open) }
        }
    }

    private fun LazyListScope.sectionItems(editor: Editor, section: Section, open: MutableMap<String, Boolean>) {
        val skipped = section.print && editor.get("io.scan_film").bool() == true
        item(key = "h:${section.title}") {
            Text(
                section.title.uppercase(), color = Palette.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold,
                letterSpacing = 0.7.sp, modifier = Modifier.padding(start = 14.dp, end = 14.dp, top = 14.dp, bottom = 6.dp),
            )
        }
        if (skipped && section.footer != null) {
            item(key = "f:${section.title}") {
                Text(section.footer, color = Palette.muted, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 14.dp, vertical = 4.dp))
            }
        }
        val loose = section.controls.filter { shows(editor, it, basic = !it.advanced) }
        items(loose, key = { "c:${section.title}:${it.label}:${(it as? Slider)?.path ?: ""}" }) { control ->
            Box(Modifier.padding(horizontal = 14.dp, vertical = 4.dp)) { ControlRow(editor, control, enabled = !skipped) }
        }
        for (group in section.groups) {
            if (!groupShows(editor, group)) continue
            val id = "${section.title}/${group.title}"
            item(key = "g:$id") {
                GroupHeader(editor, group, open[id] ?: group.open, enabled = !skipped) { open[id] = !(open[id] ?: group.open) }
            }
            if (open[id] ?: group.open) {
                val shown = group.controls.withIndex().filter { (i, c) -> shows(editor, c, basic = i < group.basic) }.map { it.value }
                val active = group.enable == null || editor.get(group.enable).bool() == true
                items(shown, key = { "c:$id:${it.label}:${pathOf(it)}" }) { control ->
                    Box(Modifier.padding(start = 32.dp, end = 14.dp, top = 4.dp, bottom = 4.dp)) {
                        ControlRow(editor, control, enabled = active && !skipped)
                    }
                }
            }
        }
        item(key = "d:${section.title}") { HorizontalDivider(color = Palette.line, modifier = Modifier.padding(top = 8.dp)) }
    }

    private fun pathOf(c: Control) = when (c) {
        is Slider -> c.path
        is Toggle -> c.path
        is Choice -> c.path
        is StockLane -> "${c.path}.${c.index}"
        is ColorFilters -> c.path
        is BorderControl -> c.field.name
        is Action -> c.action
        is MatchSwitch -> "match"
        is ExposureOffset -> "prep.exposure_ev"
        is MatchNote -> "match-note"
    }

    private fun modified(editor: Editor, c: Control): Boolean = when (c) {
        is Slider -> editor.isModified(c.path)
        is Toggle -> editor.isModified(c.path)
        is Choice -> editor.isModified(c.path)
        is ColorFilters -> editor.isModified(c.path)
        is StockLane -> editor.get(c.path) != null
        is ExposureOffset -> kotlin.math.abs(editor.exposureOffset()) > 1e-6
        else -> false
    }

    /** Without Advanced controls: the basics, and whatever was changed. */
    private fun shows(editor: Editor, c: Control, basic: Boolean): Boolean {
        val path = c.whenPath
        if (path != null) {
            val value = editor.get(path)
            if (c.whenValue != null) { if (value.text() != c.whenValue) return false } else if (value.bool() != true) return false
        }
        if (c is StockLane && editor.filmIsBW() && c.bw == null) return false
        if (c is BorderControl && c.styles.isNotEmpty() && editor.border.style !in c.styles) return false
        if (c is MatchSwitch && !editor.raw) return false
        if (c is MatchNote) return editor.matchNote() != null
        // While matched the meter only sets where the highlight limit
        // starts: Auto exposure shows with matching off, or once changed.
        if (c is Toggle && c.path == "camera.auto_exposure" || c is Choice && c.path == "camera.auto_exposure_method") {
            if (editor.matchActive() && !editor.isModified("camera.auto_exposure") && !editor.isModified("camera.auto_exposure_method")) return false
        }
        return editor.advanced || basic || modified(editor, c)
    }

    private fun groupShows(editor: Editor, group: Group): Boolean {
        if (group.colorFilter) {
            return editor.filmIsBW() || (editor.get("camera.color_filter").text() ?: "none") != "none"
        }
        val enabled = group.enable?.let { editor.isModified(it) } ?: false
        return editor.advanced || group.basic > 0 || enabled || group.controls.any { modified(editor, it) }
    }

    @Composable
    private fun GroupHeader(editor: Editor, group: Group, expanded: Boolean, enabled: Boolean, toggle: () -> Unit) {
        Row(
            Modifier.fillMaxWidth().clickable(onClick = toggle).padding(horizontal = 14.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(if (expanded) Icons.Default.ExpandLess else Icons.Default.ExpandMore, null, tint = Palette.faint, modifier = Modifier.size(18.dp))
            Text(group.title, fontWeight = FontWeight.SemiBold, fontSize = 14.sp, modifier = Modifier.padding(start = 6.dp).weight(1f))
            val enable = group.enable
            if (enable != null) {
                Switch(
                    checked = editor.get(enable).bool() == true, enabled = enabled,
                    onCheckedChange = { editor.set(enable, it.json()) },
                    colors = switchColors(),
                )
            } else Spacer(Modifier.height(48.dp))
        }
    }

    @Composable
    private fun switchColors() = SwitchDefaults.colors(
        checkedThumbColor = Palette.bg, checkedTrackColor = Palette.accent,
        uncheckedThumbColor = Palette.muted, uncheckedTrackColor = Palette.field, uncheckedBorderColor = Palette.line,
    )

    @Composable
    private fun ControlRow(editor: Editor, c: Control, enabled: Boolean) {
        when (c) {
            is Slider -> SliderRow(
                c.label, editor.get(c.path).number() ?: editor.defaultValue(c.path).number() ?: c.min,
                c.min, c.max, c.step, enabled, editor.isModified(c.path),
                onReset = { editor.defaultValue(c.path)?.let { editor.set(c.path, it) } },
                onChange = { v, moving -> editor.set(c.path, v.json(), moving) },
                onDone = editor::settle,
            )
            is StockLane -> {
                val defaults = editor.stockDefaults(c.path)
                SliderRow(
                    if (editor.filmIsBW()) c.bw ?: c.label else c.label,
                    editor.stockValues(c.path).getOrNull(c.index) ?: 0.0, c.min, c.max, c.step, enabled,
                    editor.get(c.path) != null,
                    onReset = { defaults.getOrNull(c.index)?.let { editor.setStock(c.path, c.index, it, false) } },
                    onChange = { v, moving -> editor.setStock(c.path, c.index, v, moving) },
                    onDone = editor::settle,
                )
            }
            is Toggle -> SwitchRow(c.label, editor.get(c.path).bool() == true, enabled) { editor.set(c.path, it.json()) }
            is Choice -> ChoiceRow(c.label, editor.get(c.path).text() ?: "", c.options, enabled) { editor.set(c.path, it.json()) }
            is Action -> OutlinedButton(onClick = editor::balancePrint, enabled = enabled && !editor.balancing) {
                Text(if (editor.balancing) "Balancing…" else c.label, color = Palette.fg)
            }
            is ColorFilters -> ColorFilterRow(editor, c.path, enabled)
            is BorderControl -> BorderRow(editor, c)
            is MatchSwitch -> SwitchRow(c.label, editor.match != null, enabled) { editor.setMatch(it) }
            is ExposureOffset -> SliderRow(
                c.label, editor.exposureOffset(), -3.0, 3.0, 0.05, enabled, modified(editor, c),
                onReset = { editor.setExposureOffset(0.0, false) },
                onChange = { v, moving -> editor.setExposureOffset(v, moving) },
                onDone = editor::settle,
            )
            is MatchNote -> Text(editor.matchNote() ?: "", color = Palette.faint, fontSize = 12.sp)
        }
    }

    @OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
    @Composable
    private fun SliderRow(
        label: String, value: Double, min: Double, max: Double, step: Double, enabled: Boolean, modified: Boolean,
        onReset: () -> Unit, onChange: (Double, Boolean) -> Unit, onDone: () -> Unit,
    ) {
        Column(Modifier.fillMaxWidth()) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // Double tap the name to reset, as on desktop.
                Text(
                    label, color = if (modified) Palette.fg else Palette.muted, fontSize = 13.sp,
                    modifier = Modifier.weight(1f).combinedClickable(onClick = {}, onDoubleClick = onReset),
                )
                Text(Controls.format(value, step), color = Palette.muted, fontSize = 12.sp)
                if (modified) {
                    TextButton(onClick = onReset, contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 6.dp)) {
                        Text("Reset", color = Palette.faint, fontSize = 11.sp)
                    }
                }
            }
            Slider(
                value = value.toFloat().coerceIn(min.toFloat(), max.toFloat()),
                onValueChange = { onChange(Controls.snap(it.toDouble(), min, max, step), true) },
                onValueChangeFinished = onDone,
                valueRange = min.toFloat()..max.toFloat(),
                enabled = enabled,
                colors = SliderDefaults.colors(thumbColor = Palette.fg, activeTrackColor = Palette.accent, inactiveTrackColor = Palette.field),
                modifier = Modifier.height(32.dp),
            )
        }
    }

    @Composable
    private fun SwitchRow(label: String, checked: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, color = Palette.muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
            Switch(checked, onChange, enabled = enabled, colors = switchColors())
        }
    }

    @Composable
    private fun ChoiceRow(label: String, value: String, options: List<String>, enabled: Boolean, onChange: (String) -> Unit) {
        var open by remember { mutableStateOf(false) }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, color = Palette.muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
            Box {
                OutlinedButton(onClick = { open = true }, enabled = enabled) { Text(Controls.optionTitle(value), color = Palette.fg) }
                DropdownMenu(open, onDismissRequest = { open = false }) {
                    options.forEach { option ->
                        DropdownMenuItem(text = { Text(Controls.optionTitle(option)) }, onClick = { onChange(option); open = false })
                    }
                }
            }
        }
    }

    @OptIn(ExperimentalLayoutApi::class)
    @Composable
    private fun ColorFilterRow(editor: Editor, path: String, enabled: Boolean) {
        val current = editor.get(path).text() ?: "none"
        val glasses = editor.catalog?.colorFilters.orEmpty()
        Column {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Chip("None", current == "none", enabled, null) { editor.set(path, "none".json()) }
                glasses.forEach { g -> Chip(g.name, current == g.key, enabled, Color(g.colour)) { editor.set(path, g.key.json()) } }
            }
            Text(
                Controls.COLOR_FILTER_EFFECTS[current] ?: "", color = Palette.faint, fontSize = 12.sp,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
    }

    @Composable
    private fun Chip(label: String, selected: Boolean, enabled: Boolean, swatch: Color?, onClick: () -> Unit) {
        Row(
            Modifier.clip(RoundedCornerShape(14.dp))
                .border(1.dp, if (selected) Palette.accent else Palette.line, RoundedCornerShape(14.dp))
                .background(if (selected) Palette.accent.copy(alpha = .15f) else Palette.field)
                .clickable(enabled = enabled, onClick = onClick)
                .padding(horizontal = 10.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (swatch != null) {
                Box(Modifier.size(12.dp).clip(CircleShape).background(swatch))
                Spacer(Modifier.width(6.dp))
            }
            Text(label, fontSize = 12.sp, color = if (selected) Palette.fg else Palette.muted)
        }
    }

    @OptIn(ExperimentalLayoutApi::class)
    @Composable
    private fun BorderRow(editor: Editor, c: BorderControl) {
        val b = editor.border
        val on = b.enabled
        when (c.field) {
            BorderField.Enabled -> SwitchRow(c.label, b.enabled, true) { v -> editor.updateBorder { it.copy(enabled = v) } }
            BorderField.Style -> FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                BorderStyle.entries.forEach { s -> Chip(s.title, b.style == s, on, null) { editor.updateBorder { it.copy(style = s) } } }
            }
            BorderField.Width -> PercentRow(c.label, b.width, 0.0, 15.0, on) { v -> editor.updateBorder { it.copy(width = v) } }
            BorderField.FilmWidth -> PercentRow(c.label, b.filmWidth, 0.0, 15.0, on) { v -> editor.updateBorder { it.copy(filmWidth = v) } }
            BorderField.Line -> PercentRow(c.label, b.line, 0.05, 2.0, on, step = 0.05) { v -> editor.updateBorder { it.copy(line = v) } }
            BorderField.Color -> SwatchRow(c.label, b.color, on) { v -> editor.updateBorder { it.copy(color = v) } }
            BorderField.LineColor -> SwatchRow(c.label, b.lineColor, on) { v -> editor.updateBorder { it.copy(lineColor = v) } }
            BorderField.Instagram -> SwitchRow(c.label, b.instagram, on) { v -> editor.updateBorder { it.copy(instagram = v) } }
            BorderField.Export -> SwitchRow(c.label, b.export, on) { v -> editor.updateBorder { it.copy(export = v) } }
        }
        val size = editor.outputSize()
        if (c.field == BorderField.Style && on && size != null && !b.fits(size.first, size.second)) {
            Text(
                "The 35mm strip needs a 3:2 or 3:4 photograph. Crop it to one of those to see it.",
                color = Palette.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 36.dp),
            )
        }
    }

    /** A border width: a fraction of the long edge, shown in percent. */
    @Composable
    private fun PercentRow(label: String, fraction: Double, min: Double, max: Double, enabled: Boolean, step: Double = 0.1, onChange: (Double) -> Unit) {
        SliderRow(
            label, fraction * 100, min, max, step, enabled, false,
            onReset = {}, onChange = { v, _ -> onChange(v / 100) }, onDone = {},
        )
    }

    @Composable
    private fun SwatchRow(label: String, value: Int, enabled: Boolean, onChange: (Int) -> Unit) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, color = Palette.muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
            BorderSettings.SWATCHES.forEach { rgb ->
                Box(
                    Modifier.padding(start = 8.dp).size(28.dp).clip(CircleShape)
                        .background(Color(0xFF000000 or rgb.toLong()))
                        .border(2.dp, if (rgb == value) Palette.accent else Palette.line, CircleShape)
                        .clickable(enabled = enabled) { onChange(rgb) },
                )
            }
        }
    }

    // ---- Stock --------------------------------------------------------------

    private fun LazyListScope.stockItems(editor: Editor) {
        val catalog = editor.catalog ?: return
        item(key = "search") { StockSearch(editor) }
        val query = editor.stockQuery.trim().lowercase()
        fun matching(stocks: List<Stock>) = if (query.isEmpty()) stocks else stocks.filter { query in it.name.lowercase() }
        // The favourites first, in the order starred, and not while searching;
        // each stays in its group too (web/stocks.js).
        fun withFavorites(groups: List<Pair<String, List<Stock>>>, starred: List<String>): List<Pair<String, List<Stock>>> {
            if (query.isNotEmpty()) return groups.map { it.first to matching(it.second) }
            val listed = groups.flatMap { it.second }.associateBy { it.id }
            val favorites = starred.mapNotNull { listed[it] }
            return listOf("★ Favorites" to favorites) + groups
        }
        stockGrid("Film", withFavorites(catalog.filmGroups(editor.edit.film), editor.favoriteFilms), editor) { editor.chooseFilm(it.id) }
        if (editor.get("io.scan_film").bool() != true) {
            stockGrid("Paper", withFavorites(catalog.paperGroups(editor.edit.paper), editor.favoritePapers), editor) { editor.choosePaper(it.id) }
        } else {
            item(key = "scanned") {
                Text(
                    "Scanned directly: no paper.", color = Palette.faint, fontSize = 12.sp,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                )
            }
        }
    }

    @Composable
    private fun StockSearch(editor: Editor) {
        OutlinedTextField(
            value = editor.stockQuery, onValueChange = { editor.stockQuery = it },
            placeholder = { Text("Search films and papers", color = Palette.faint, fontSize = 13.sp) },
            singleLine = true, keyboardOptions = KeyboardOptions.Default,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp),
            textStyle = androidx.compose.ui.text.TextStyle(fontSize = 13.sp, color = Palette.fg),
        )
    }

    private const val COLUMNS = 3

    private fun LazyListScope.stockGrid(title: String, groups: List<Pair<String, List<Stock>>>, editor: Editor, choose: (Stock) -> Unit) {
        if (groups.all { it.second.isEmpty() }) return
        item(key = "t:$title") {
            val current = if (title == "Film") editor.edit.film else editor.edit.paper
            Row(Modifier.padding(start = 14.dp, end = 14.dp, top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(title, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
                Spacer(Modifier.weight(1f))
                Text(editor.catalog?.stock(current)?.name ?: current, color = Palette.muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        for ((group, stocks) in groups) {
            if (stocks.isEmpty()) continue
            item(key = "gt:$title:$group") {
                Text(
                    group.uppercase(), color = Palette.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(start = 14.dp, top = 12.dp, bottom = 6.dp),
                )
            }
            items(stocks.chunked(COLUMNS), key = { row -> "r:$title:$group:" + row.joinToString { it.id } }) { row ->
                Row(Modifier.padding(horizontal = 14.dp, vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    row.forEach { stock -> StockTile(editor, stock, Modifier.weight(1f), choose) }
                    repeat(COLUMNS - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }

    @OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
    @Composable
    private fun StockTile(editor: Editor, stock: Stock, modifier: Modifier, choose: (Stock) -> Unit) {
        val selected = stock.id == (if (stock.paper) editor.edit.paper else editor.edit.film)
        val thumbnail = editor.thumbnails[stock.id]
        val longSide = with(LocalDensity.current) { 140.dp.roundToPx() }
        // Asked for as the tile shows, and again once the edit it shows is stale.
        val wanted = editor.tileKey(stock)
        LaunchedEffect(wanted) { if (wanted != null) editor.wantThumbnail(stock, longSide) }
        val favorite = editor.isFavorite(stock)
        // Tap to choose; long press, or the star, to add to or remove from the favourites.
        Column(modifier.combinedClickable(onClick = { choose(stock) }, onLongClick = { editor.toggleFavorite(stock) })) {
            Box(
                Modifier.fillMaxWidth().aspectRatio(1.3f).clip(RoundedCornerShape(5.dp)).background(Palette.field)
                    .then(if (selected) Modifier.border(2.dp, Palette.accent, RoundedCornerShape(5.dp)) else Modifier),
            ) {
                if (thumbnail != null) {
                    val rotation = editor.rotation
                    val image = remember(thumbnail.second, rotation) { editor.turned(thumbnail.second).asImageBitmap() }
                    Image(
                        image, stock.name, contentScale = ContentScale.Crop,
                        alpha = if (thumbnail.first == wanted) 1f else .55f,
                        modifier = Modifier.fillMaxWidth().aspectRatio(1.3f),
                    )
                }
                Icon(
                    if (favorite) Icons.Default.Star else Icons.Default.StarBorder,
                    if (favorite) "Remove ${stock.name} from favorites" else "Add ${stock.name} to favorites",
                    tint = if (favorite) Palette.accent else Color.White.copy(alpha = .7f),
                    modifier = Modifier.align(Alignment.TopEnd).clickable { editor.toggleFavorite(stock) }
                        .padding(3.dp).size(18.dp),
                )
            }
            Text(
                stock.name, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                color = if (selected) Palette.fg else Palette.muted, modifier = Modifier.padding(top = 3.dp),
            )
        }
    }
}
