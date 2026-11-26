package dev.negs.app

import android.util.Base64
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.FileAlreadyExistsException
import java.nio.file.Files
import java.text.Normalizer
import java.util.Locale
import java.util.UUID
import java.util.zip.Deflater
import java.util.zip.Inflater

/**
 * A preset: a file in the app's presets folder holding Python spektrafilm's
 * saved GUI state, named after the file (docs/presets-design.md, macOS
 * Presets.swift). The engine reads and writes the format; this only
 * handles the files.
 */
data class PresetFile(val file: File, val size: Long, val modified: Long) {
    val name: String get() = file.name.substringBeforeLast('.')
}

/** What the engine made of a preset file. */
sealed interface PresetStatus {
    data class Valid(val film: String, val paper: String, val scan: Boolean, val text: String) : PresetStatus
    data class Invalid(val reason: String) : PresetStatus
}

/**
 * One naming policy on every platform (macOS PresetName, web presets.js):
 * names Windows, macOS and Linux all accept as file names, compared without
 * regard to case or Unicode form.
 */
object PresetName {
    const val MAX_LENGTH = 120
    private const val INVALID = "/\\:*?\"<>|"
    private val RESERVED = setOf("CON", "PRN", "AUX", "NUL") + (1..9).flatMap { listOf("COM$it", "LPT$it") }

    private fun length(s: String) = s.codePointCount(0, s.length)

    /** Why [name] can't name a preset, or null. */
    fun problem(name: String): String? {
        if (name.isBlank()) return tr("Enter a name.")
        if (length(name) > MAX_LENGTH) return tr("Use at most {n} characters.", "n" to MAX_LENGTH)
        if (name.any { Character.getType(it) == Character.CONTROL.toInt() || it in INVALID }) return tr("A name can’t contain / \\ : * ? \" < > or |.")
        if (name.startsWith(".")) return tr("A name can’t start with a period.")
        if (name.endsWith(".") || name.endsWith(" ") || name.startsWith(" ")) return tr("A name can’t start with a space or end with a space or period.")
        if (name.substringBefore('.').uppercase(Locale.ROOT).trim() in RESERVED) return tr("“{name}” is reserved on Windows.", "name" to name)
        return null
    }

    /** Names that are the same preset: Unicode-normalized and case-folded. */
    fun key(name: String): String = Normalizer.normalize(name, Normalizer.Form.NFC).lowercase(Locale.ROOT)

    /** [name], or "name 2", "name 3"… the first not in [taken] (keys). */
    fun unique(name: String, taken: Set<String>): String {
        if (key(name) !in taken) return name
        var n = 2
        while (key("$name $n") in taken) n++
        return "$name $n"
    }

    /** A file's name as a preset name: invalid characters become spaces. */
    fun from(fileName: String): String {
        val trim = { s: String -> s.trim(' ', '.') }
        var name = trim(fileName.map { if (it in INVALID || it == '\n' || it == '\r' || it == '\u000B' || it == '\u000C' || it == '\u0085' || it == ' ' || it == ' ') ' ' else it }.joinToString(""))
            .replace("  ", " ")
        if (length(name) > MAX_LENGTH) name = trim(name.substring(0, name.offsetByCodePoints(0, MAX_LENGTH)))
        return if (problem(name) == null) name else "Preset"
    }

    /** The name without its extension, as desktop names an imported file. */
    fun stem(fileName: String) = fileName.substringBeforeLast('.', fileName).ifEmpty { fileName }
}

/** A preset couldn't be saved, renamed, read or deleted; the message says why, for the user. */
class PresetError(message: String) : Exception(message)

/**
 * The presets folder (macOS PresetLibrary). Only the app writes here, one
 * change at a time (the caller serializes them); writes never replace a
 * file, and renames and deletes act only on the file as it was listed.
 *
 * The user's order is `.order.json` in the folder, `{"order": [file names]}`,
 * as on desktop: presets it doesn't name follow in name order; added
 * presets go to the end.
 */
class PresetLibrary(val folder: File) {
    private val orderFile get() = File(folder, ".order.json")

    fun list(): List<PresetFile> {
        val files = (folder.listFiles() ?: return emptyList())
            .filter { !it.name.startsWith(".") && it.extension.equals("json", ignoreCase = true) && it.isFile }
            .map { PresetFile(it, it.length(), it.lastModified()) }
            .sortedBy { PresetName.key(it.name) }
        val position = HashMap<String, Int>()
        readOrder().forEachIndexed { i, n -> position.putIfAbsent(PresetName.key(n), i) }
        // Stable: unordered files keep their name order after the ordered ones.
        return files.sortedBy { position[PresetName.key(it.name)] ?: Int.MAX_VALUE }
    }

    /** Remembers this order of the presets. */
    fun setOrder(presets: List<PresetFile>) = writeOrder(presets.map { it.name })

    /** Writes [data] as a new preset named [name], or "name 2"… if taken. */
    fun add(data: ByteArray, name: String): PresetFile {
        PresetName.problem(name)?.let { throw PresetError(it) }
        folder.mkdirs()
        val temporary = File(folder, ".${UUID.randomUUID()}.tmp")
        temporary.writeBytes(data)
        try {
            val taken = list().map { PresetName.key(it.name) }.toMutableSet()
            repeat(1000) {
                val candidate = PresetName.unique(name, taken)
                val target = File(folder, "$candidate.json")
                try {
                    Files.move(temporary.toPath(), target.toPath())
                    writeOrder(list().map { it.name }.filter { PresetName.key(it) != PresetName.key(candidate) } + candidate)
                    return PresetFile(target, target.length(), target.lastModified())
                } catch (e: FileAlreadyExistsException) {
                    taken += PresetName.key(candidate)
                }
            }
            throw PresetError(tr("Couldn’t find a free name for the preset."))
        } finally {
            temporary.delete()
        }
    }

    fun rename(preset: PresetFile, name: String): PresetFile {
        PresetName.problem(name)?.let { throw PresetError(it) }
        verify(preset)
        val target = File(folder, "$name.json")
        if (PresetName.key(name) != PresetName.key(preset.name) && list().any { PresetName.key(it.name) == PresetName.key(name) }) {
            throw PresetError(tr("A preset named “{name}” already exists.", "name" to name))
        }
        if (!preset.file.renameTo(target)) throw PresetError(tr("Couldn’t rename the preset."))
        val order = readOrder().toMutableList()
        val index = order.indexOfFirst { PresetName.key(it) == PresetName.key(preset.name) }
        if (index >= 0) {
            order[index] = name
            writeOrder(order)
        }
        return PresetFile(target, target.length(), target.lastModified())
    }

    fun delete(preset: PresetFile) {
        verify(preset)
        if (!preset.file.delete()) throw PresetError(tr("Couldn’t delete the preset."))
        writeOrder(readOrder().filter { PresetName.key(it) != PresetName.key(preset.name) })
    }

    fun read(preset: PresetFile): ByteArray {
        verify(preset)
        return read(preset.file)
    }

    /** Preset names in the user's order; none if the file is missing or unreadable. */
    private fun readOrder(): List<String> = try {
        val order = Json.parseToJsonElement(orderFile.readText()).jsonObject["order"] as? JsonArray
        order.orEmpty().mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content?.removeSuffix(".json") }
    } catch (e: Exception) {
        emptyList()
    }

    private fun writeOrder(names: List<String>) {
        val text = buildJsonObject { put("order", JsonArray(names.map { JsonPrimitive("$it.json") })) }.toString()
        val temporary = File(folder, ".order.${UUID.randomUUID()}.tmp")
        temporary.writeText(text)
        if (!temporary.renameTo(orderFile)) {
            temporary.delete()
            throw PresetError(tr("Couldn’t save the presets’ order."))
        }
    }

    /** The file is still the one listed: not replaced, edited or removed since. */
    private fun verify(preset: PresetFile) {
        val f = preset.file
        if (!f.isFile || f.length() != preset.size || f.lastModified() != preset.modified) {
            throw PresetError(tr("“{name}” changed. The list has been updated; try again.", "name" to preset.name))
        }
    }

    companion object {
        /** Larger files aren't GUI states. */
        const val MAX_FILE_BYTES = 1_048_576

        fun read(file: File): ByteArray {
            if (file.length() > MAX_FILE_BYTES) throw PresetError(tr("The file is too large to be a preset."))
            return file.readBytes()
        }

        /** A file's bytes as the text the engine reads: UTF-8, without a byte order mark. */
        fun text(data: ByteArray): String {
            val text = try {
                Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(data)).toString()
            } catch (e: java.nio.charset.CharacterCodingException) {
                throw PresetError(tr("The file isn’t text."))
            }
            return text.removePrefix("﻿")
        }
    }
}

/**
 * Preset links (web/presets.js): `https://getnegs.com/try/#preset=` then
 * base64url of a version byte (1) and the raw DEFLATE of the UTF-8 JSON
 * {"name": …, "preset": the file's text}. The web app opens them; Negs for
 * Android reads them when they're shared to it.
 */
object PresetLink {
    const val BASE = "https://getnegs.com/try/"
    private const val KEY = "preset"
    private const val VERSION: Byte = 1
    const val MAX_CHARS = 16_384
    private const val MAX_ENVELOPE_BYTES = PresetLibrary.MAX_FILE_BYTES + 4096

    private const val FLAGS = Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP

    /** The link for preset [text] named [name], or null if it would be too long to share. */
    fun encode(name: String, text: String): String? {
        val json = buildJsonObject { put("name", name); put("preset", text) }.toString().toByteArray()
        val deflater = Deflater(Deflater.DEFAULT_COMPRESSION, true)
        deflater.setInput(json)
        deflater.finish()
        val out = ByteArrayOutputStream().apply { write(VERSION.toInt()) }
        val buffer = ByteArray(8192)
        while (!deflater.finished()) out.write(buffer, 0, deflater.deflate(buffer))
        deflater.end()
        val link = "$BASE#$KEY=" + Base64.encodeToString(out.toByteArray(), FLAGS)
        return if (link.length - BASE.length > MAX_CHARS) null else link
    }

    /** The preset link's value in [text] (a shared message), or null if it has none. */
    fun find(text: String): String? =
        Regex("""[#&]$KEY=([A-Za-z0-9_-]*)""").find(text)?.groupValues?.get(1)

    /** A link value's name and preset text; throws a [PresetError] saying why it can't be read. */
    fun decode(value: String): Pair<String, String> {
        if (value.isEmpty()) throw PresetError(tr("This preset link is empty."))
        if (value.length + KEY.length + 2 > MAX_CHARS) throw PresetError(tr("This preset link is too long."))
        val damaged = PresetError(tr("This preset link is damaged or incomplete."))
        if (value.length % 4 == 1) throw damaged
        val bytes = try { Base64.decode(value, FLAGS) } catch (e: IllegalArgumentException) { throw damaged }
        if (bytes.isEmpty()) throw damaged
        if (bytes[0] != VERSION) throw PresetError(tr("This preset link is from a newer Negs. Update the app, then open it again."))
        val inflater = Inflater(true)
        inflater.setInput(bytes, 1, bytes.size - 1)
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        try {
            while (!inflater.finished()) {
                val n = inflater.inflate(buffer)
                if (n == 0 && (inflater.needsInput() || inflater.needsDictionary())) break
                out.write(buffer, 0, n)
                if (out.size() > MAX_ENVELOPE_BYTES) throw PresetError(tr("The preset in this link is too large."))
            }
        } catch (e: java.util.zip.DataFormatException) {
            throw damaged
        } finally {
            inflater.end()
        }
        val envelope = try {
            Json.parseToJsonElement(PresetLibrary.text(out.toByteArray())) as? JsonObject
        } catch (e: Exception) {
            null
        } ?: throw damaged
        val name = (envelope["name"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: throw damaged
        val preset = (envelope["preset"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: throw damaged
        return PresetName.from(name) to preset
    }
}
