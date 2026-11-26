package dev.negs.app

import android.content.ComponentCallbacks2
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import dev.negs.app.ui.EditorScreen
import dev.negs.app.ui.NegsTheme

class MainActivity : ComponentActivity() {
    private val editor: Editor by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) opened(intent)
        setContent { NegsTheme { EditorScreen(editor) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opened(intent)
    }

    /** "Open with" and "Share to" Negs: a photograph, preset files, or a preset link. */
    @Suppress("DEPRECATION")
    private fun opened(intent: Intent?) {
        intent ?: return
        val uris: List<Uri> = when (intent.action) {
            Intent.ACTION_VIEW -> listOfNotNull(intent.data)
            Intent.ACTION_SEND -> listOfNotNull(intent.getParcelableExtra(Intent.EXTRA_STREAM))
            Intent.ACTION_SEND_MULTIPLE -> intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM).orEmpty()
            else -> emptyList()
        }
        val type = intent.type.orEmpty()
        val presets = uris.filter { type == "application/json" || it.lastPathSegment.orEmpty().endsWith(".json", ignoreCase = true) }
        when {
            presets.isNotEmpty() -> editor.importPresets(presets)
            uris.isNotEmpty() -> editor.open(uris.first())
            intent.action == Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)?.let { text ->
                if (!editor.importPresetLink(text)) editor.notify("There’s no preset link in what was shared.")
            }
        }
    }

    override fun onResume() {
        super.onResume()
        editor.resumed()
    }

    @Deprecated("Deprecated in Java")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW) editor.trimMemory(level)
    }
}
