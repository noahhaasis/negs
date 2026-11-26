package dev.negs.app

import android.content.Context
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.json.Json
import java.util.Locale

/**
 * The interface language (web/i18n.js): Settings › Language, else the
 * phone's. Strings pass through [tr] with their English as the key, as on
 * the web, against the web app's own dictionary (web/locales/<lang>.js,
 * copied into the assets at build time) and then the Android app's
 * strings the web doesn't have (assets/locales/<lang>.android.jsonc).
 * android/check-i18n.mjs checks that every tr() literal has an entry.
 *
 * The dictionary is Compose state: switching language redraws at once.
 */
object I18n {
    /** The languages Settings offers, by their own names. */
    val LANGUAGES = listOf("en" to "English", "zh-CN" to "简体中文")
    private const val PREF = "lang"

    var lang by mutableStateOf("en"); private set
    private var dict by mutableStateOf<Map<String, String>>(emptyMap())

    /** The saved choice, else the phone's language; once, before the first frame. */
    fun start(context: Context) {
        val saved = prefs(context).getString(PREF, null)
        val phone = Locale.getDefault().language
        load(context, saved?.takeIf { s -> LANGUAGES.any { it.first == s } } ?: if (phone == "zh") "zh-CN" else "en")
    }

    fun choose(context: Context, language: String) {
        if (LANGUAGES.none { it.first == language }) return
        prefs(context).edit().putString(PREF, language).apply()
        load(context, language)
    }

    fun tr(text: String, params: Array<out Pair<String, Any?>>): String {
        var s = dict[text] ?: text
        for ((name, value) in params) s = s.replace("{$name}", value.toString())
        return s
    }

    private fun prefs(context: Context) = context.getSharedPreferences("negs", Context.MODE_PRIVATE)

    private fun load(context: Context, language: String) {
        if (language == "en") {
            dict = emptyMap(); lang = "en"
            return
        }
        try {
            val entries = HashMap<String, String>()
            // The web's first; the app's own may add to or override it.
            for (name in listOf("locales/$language.js", "locales/$language.android.jsonc")) {
                context.assets.open(name).bufferedReader().use { entries += parse(it.readText()) }
            }
            dict = entries; lang = language
        } catch (e: Exception) {
            // As the web does when its dictionary can't load: English.
            Log.w(Engine.TAG, "no $language strings: ${e.message}")
            dict = emptyMap(); lang = "en"
        }
    }

    /**
     * The string pairs of a dictionary file: web/locales' script (a
     * `window.NEGS_I18N.dict = { "English": "译文", … };` object literal) or
     * the app's own (the same pairs between braces): JSON strings, `//`
     * comments, and nothing else holding a quote.
     */
    internal fun parse(source: String): Map<String, String> {
        val strings = ArrayList<String>()
        var i = 0
        while (i < source.length) {
            val c = source[i]
            when {
                c == '/' && source.startsWith("//", i) -> i = source.indexOf('\n', i).let { if (it < 0) source.length else it }
                c == '"' -> {
                    var j = i + 1
                    while (source[j] != '"') j += if (source[j] == '\\') 2 else 1
                    strings += Json.decodeFromString<String>(source.substring(i, j + 1))
                    i = j + 1
                }
                else -> i++
            }
        }
        require(strings.size % 2 == 0) { "an odd number of strings" }
        return strings.chunked(2).associate { (english, translated) -> english to translated }
    }
}

/**
 * [text] in the interface language; `{name}` holes filled from [params]
 * either way: tr("Saved “{name}”", "name" to file.name).
 */
fun tr(text: String, vararg params: Pair<String, Any?>): String = I18n.tr(text, params)
