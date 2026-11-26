package dev.negs.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** The web app's colours (web/index.html `:root`). */
object Palette {
    val bg = Color(0xFF161616)
    val panel = Color(0xFF1F1F1F)
    val line = Color(0xFF2E2E2E)
    val field = Color(0xFF2A2A2A)
    val fg = Color(0xFFF2F0ED)
    val muted = Color(0xFFBCB8B1)
    val faint = Color(0xFF8E8A83)
    val accent = Color(0xFFE0A458)
    val danger = Color(0xFFE5786D)
}

@Composable
fun NegsTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Palette.accent,
            onPrimary = Palette.bg,
            secondary = Palette.accent,
            background = Palette.bg,
            onBackground = Palette.fg,
            surface = Palette.panel,
            onSurface = Palette.fg,
            surfaceVariant = Palette.field,
            onSurfaceVariant = Palette.muted,
            outline = Palette.line,
            outlineVariant = Palette.line,
            error = Palette.danger,
            surfaceContainer = Palette.panel,
            surfaceContainerHigh = Palette.field,
            surfaceContainerHighest = Palette.field,
        ),
        content = content,
    )
}
