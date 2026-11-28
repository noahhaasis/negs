package dev.negs.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.negs.app.Editor
import dev.negs.app.I18n
import dev.negs.app.tr

/**
 * The ⋮ menu's Settings (web #settings-dialog): the Stock tab's previews,
 * which films it lists, and the interface language. The film lists change
 * only the lists; a photo or preset keeps its film.
 */
@Composable
fun SettingsDialog(editor: Editor, onDismiss: () -> Unit) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(tr("Settings")) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                Column {
                    SwitchSetting(tr("Film and paper previews"), editor.stockPreviews, editor::chooseStockPreviews)
                    Hint(tr("Show each film and paper on your photograph. Off, the Stock tab lists them by name, and Negs uses less memory."))
                }
                Column {
                    Text(tr("Fotufilm films"), color = Palette.fg, fontSize = 15.sp)
                    listOf("all" to tr("All"), "datasheets" to tr("From datasheets"), "off" to tr("None")).forEach { (value, title) ->
                        Radio(title, editor.fotufilmFilms == value) { editor.chooseFotufilmFilms(value) }
                    }
                    Hint(tr("Films ported from Fotufilm. From datasheets hides the eight it models from sample photographs rather than the manufacturer’s data. Photos already using a hidden film keep it."))
                }
                Column {
                    SwitchSetting(tr("Show Negs-built films"), editor.showNegsBuilt, editor::chooseShowNegsBuilt)
                    Hint(tr("Films Negs prepared itself from manufacturers’ datasheets, still being checked: the Eterna cine films; Pro 160S, 160C and 800Z, Superia 1600, True Definition 400 and Reala Ace; Kodak Portra 400NC, 400VC and EXR 100T; Agfa Portrait XPS 160 and Konica Minolta Professional 160; the Velvia 100F, Provia 400F and 400X, Sensia and 64T slides; and the Eterna-CP and Agfa Print CP30 print films. Photos already using one keep it."))
                }
                Column {
                    Text(tr("Language"), color = Palette.fg, fontSize = 15.sp)
                    // Each language by its own name.
                    I18n.LANGUAGES.forEach { (value, title) ->
                        Radio(title, I18n.lang == value) { I18n.choose(context, value) }
                    }
                    Hint(tr("The interface language."))
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(tr("Done"), color = Palette.accent) } },
        containerColor = Palette.panel,
    )
}

@Composable
private fun SwitchSetting(title: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(title, color = Palette.fg, fontSize = 15.sp, modifier = Modifier.weight(1f))
        Switch(
            checked, onChange,
            colors = SwitchDefaults.colors(
                checkedThumbColor = Palette.bg, checkedTrackColor = Palette.accent,
                uncheckedThumbColor = Palette.muted, uncheckedTrackColor = Palette.field, uncheckedBorderColor = Palette.line,
            ),
        )
    }
}

@Composable
private fun Radio(title: String, selected: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick), verticalAlignment = Alignment.CenterVertically) {
        RadioButton(
            selected = selected, onClick = onClick,
            colors = RadioButtonDefaults.colors(selectedColor = Palette.accent, unselectedColor = Palette.muted),
        )
        Text(title, color = Palette.fg, fontSize = 14.sp)
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, color = Palette.faint, fontSize = 12.sp, lineHeight = 16.sp, modifier = Modifier.padding(top = 4.dp))
}
