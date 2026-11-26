package dev.negs.app

import android.app.Application
import android.content.ComponentCallbacks2
import android.content.ContentValues
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.net.Uri
import android.os.Environment
import android.os.SystemClock
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.io.File
import java.nio.ByteBuffer
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sqrt

/** The photograph being edited: a copy of the picked file in the cache. */
data class Photo(val name: String, val file: File, val generation: Long)

/** What the film, paper and params make; rotation and the border are finishing. */
data class Edit(val film: String, val paper: String, val params: JsonObject)

/** A frame on screen: the developed photograph, or the original. */
data class Frame(val bitmap: Bitmap, val edit: Edit?, val interactive: Boolean)

/**
 * The editor (web/app.js, one photograph at a time): the edit as desktop
 * keeps it (the film, the paper and a complete RuntimeParams, from the
 * engine's defaults), renders it as it changes, newest first, and exports it.
 */
class Editor(app: Application) : AndroidViewModel(app) {
    private val context get() = getApplication<Application>()
    val memory = MemoryPolicy.measure(app)
    private lateinit var engine: Engine
    private var dataDir: File? = null
    private val prefs = app.getSharedPreferences("negs", android.content.Context.MODE_PRIVATE)

    var catalog by mutableStateOf<Catalog?>(null); private set
    var startupError by mutableStateOf<String?>(null); private set
    var photo by mutableStateOf<Photo?>(null); private set
    var edit by mutableStateOf(Edit("", "", JsonObject(emptyMap()))); private set
    var border by mutableStateOf(BorderSettings()); private set
    /** Clockwise quarter turns, applied when shown and exported. */
    var rotation by mutableStateOf(0); private set
    /** The crop, applied when shown and exported; the whole frame always develops. */
    var crop by mutableStateOf(CropSettings()); private set
    /** A crop under way: the editor shows the whole photograph and the crop's inspector. */
    var cropSession by mutableStateOf<CropSession?>(null); private set
    var frame by mutableStateOf<Frame?>(null); private set
    var original by mutableStateOf<Bitmap?>(null); private set
    var showingOriginal by mutableStateOf(false); private set
    var busy by mutableStateOf(false); private set
    var status by mutableStateOf(""); private set
    var message by mutableStateOf<Message?>(null); private set
    var exporting by mutableStateOf(false); private set
    var advanced by mutableStateOf(false)
    /** The Stock tab's search. */
    var stockQuery by mutableStateOf("")
    /** The photograph's size as decoded, once the engine has read it. */
    var photoSize by mutableStateOf<Pair<Int, Int>?>(null); private set
    /** A camera RAW: Prep's white balance and brightness match apply. */
    var raw by mutableStateOf(false); private set

    data class Message(val text: String, val error: Boolean = false, val share: Uri? = null)

    private var generation = 0L
    private var dragging = false
    private val dirty = Channel<Unit>(Channel.CONFLATED)
    private var lastKey: String? = null
    private var dimensions: Pair<Int, Int>? = null
    /** Adaptive interactive drag scale (0.15..1.0 of settled size), targeting [BUDGET_MS]. */
    private var interactiveScale = 0.5
    /** Measured photo canvas viewport in pixels and current pinch/double-tap zoom. */
    private var viewportPx: Pair<Int, Int>? = null
    private var viewportZoom = 1f
    private var settledRenderedLongSide = 0
    private var settleJob: Job? = null
    private var viewportJob: Job? = null
    /** The engine has started and the catalog is read. */
    private val started = kotlinx.coroutines.CompletableDeferred<Unit>()

    init {
        viewModelScope.launch {
            try {
                status = "Preparing the film data…"
                val data = withContext(Dispatchers.IO) { EngineData.prepare(context) }
                dataDir = data
                engine = Engine(context, data, memory)
                status = "Starting the engine…"
                val loaded = Catalog.parse(engine.call("catalog"))
                catalog = loaded
                edit = Edit("", "kodak_portra_endura", loaded.defaults)
                chooseFilm("kodak_portra_400")
                status = ""
                started.complete(Unit)
                launch { reloadPresets() }
            } catch (e: Exception) {
                Log.e(Engine.TAG, "startup failed", e)
                startupError = e.message ?: e.toString()
            }
            for (signal in dirty) {
                render()
                // A settled frame of this edit: the brightness match, if it's due.
                if (!dragging && frame?.edit == edit && cropSession == null) reconcileMatch()
            }
        }
    }

    // ---- The edit ---------------------------------------------------------

    fun get(path: String): JsonElement? = edit.params.at(path)
    fun defaultValue(path: String): JsonElement? = catalog?.defaults.at(path)
    fun isModified(path: String) = !sameValue(get(path), defaultValue(path))

    fun set(path: String, value: JsonElement, interactive: Boolean = false) {
        edit = edit.copy(params = edit.params.with(path, value))
        changed(interactive)
    }

    private fun changed(interactive: Boolean = false) {
        dragging = interactive
        settleJob?.cancel()
        if (interactive) {
            // Safeguard (web/app.js settleTimer): settle even if drag ends without onValueChangeFinished.
            settleJob = viewModelScope.launch {
                delay(250)
                if (dragging) settle()
            }
        }
        dirty.trySend(Unit)
    }

    /** A slider's drag ended: the settled frame follows. */
    fun settle() {
        settleJob?.cancel()
        dragging = false
        dirty.trySend(Unit)
    }

    /** Reports the measured photo canvas extent in pixels and its current zoom. */
    fun updateViewport(widthPx: Int, heightPx: Int, zoom: Float = 1f) {
        if (widthPx <= 0 || heightPx <= 0) return
        val clampedZoom = zoom.coerceIn(1f, 8f)
        val prev = viewportPx
        val prevZoom = viewportZoom
        if (prev?.first == widthPx && prev.second == heightPx && abs(prevZoom - clampedZoom) < 0.01f) return
        viewportPx = widthPx to heightPx
        viewportZoom = clampedZoom
        val dims = dimensions ?: return
        if (photo == null || dragging || cropSession != null) return
        val want = settledLongSide(dims.first, dims.second)
        val needsRefresh = want > settledRenderedLongSide + 8 ||
            (clampedZoom <= 1.01f && settledRenderedLongSide > 0 && abs(want - settledRenderedLongSide) > 64)
        if (!needsRefresh) return
        viewportJob?.cancel()
        viewportJob = viewModelScope.launch {
            delay(140)
            if (!dragging && cropSession == null) dirty.trySend(Unit)
        }
    }

    /**
     * Target long side (in photograph pixels) for a settled preview covering
     * the fitted viewport at the current zoom (web/app.js `renderSide`),
     * bounded by `memory.maxPreviewSourcePixels` (~4.4 MP) and `previewLongSide`.
     */
    fun settledLongSide(photoW: Int, photoH: Int): Int {
        val long = max(photoW, photoH).toDouble().coerceAtLeast(1.0)
        val short = min(photoW, photoH).toDouble().coerceAtLeast(1.0)
        val (outW, outH) = Crop.outputSize(crop, rotation, photoW, photoH)
        val vp = viewportPx
        val fittedLong = if (vp != null && vp.first > 0 && vp.second > 0 && outW > 0 && outH > 0) {
            val fit = min(vp.first.toDouble() / outW, vp.second.toDouble() / outH)
            val displayLong = max(outW, outH) * fit * viewportZoom.toDouble().coerceIn(1.0, 8.0)
            val croppedLong = max(outW, outH).toDouble().coerceAtLeast(1.0)
            long * (displayLong / croppedLong)
        } else {
            memory.initialViewportLongSide.toDouble()
        }
        val maxByPixels = floor(sqrt(memory.maxPreviewSourcePixels.toDouble() * long / short))
        val cap = min(long, min(maxByPixels, memory.previewLongSide.toDouble()))
        return fittedLong.roundToInt().coerceIn(min(MIN_INTERACTIVE_SIDE, cap.toInt().coerceAtLeast(1)), cap.toInt().coerceAtLeast(1))
    }

    /** Every value a film's presets fill (macOS Stock.presetPaths): a film change resets them. */
    private val presetPaths = listOf("rms_granularity", "density_min", "uniformity", "particle_scale_sublayers")
        .map { "film_render.grain.$it" } + listOf(
        "film_render.dir_couplers.gamma_samelayer_rgb", "film_render.dir_couplers.gamma_interlayer_r_to_gb",
        "film_render.dir_couplers.gamma_interlayer_g_to_rb", "film_render.dir_couplers.gamma_interlayer_b_to_rg",
        "film_render.halation.halation_strength", "film_render.halation.halation_first_sigma_um",
    )

    /**
     * A new film brings its own grain, DIR and halation presets; a slide
     * scans directly, and a film with a target paper prints on it (macOS
     * EditorModel.chooseFilm).
     */
    fun chooseFilm(id: String) {
        edit = filmEdit(edit, id)
        changed()
    }

    private fun filmEdit(from: Edit, id: String): Edit {
        val catalog = catalog ?: return from
        var params = from.params.with("film_render.development_time", JsonNull)
        for (path in presetPaths) params = params.with(path, JsonNull)
        val film = catalog.stock(id) ?: return from.copy(film = id, params = params)
        params = params.with("io.scan_film", film.positive.json())
        var paper = from.paper
        if (film.targetPrint != null && catalog.papers.any { it.id == film.targetPrint } && paper != film.targetPrint) {
            paper = film.targetPrint
            params = params.with("print_render.development_time", JsonNull)
        }
        return Edit(id, paper, params)
    }

    fun choosePaper(id: String) {
        edit = paperEdit(edit, id)
        changed()
    }

    private fun paperEdit(from: Edit, id: String) =
        from.copy(paper = id, params = from.params.with("print_render.development_time", JsonNull))

    /** Restores the defaults, keeping the film and paper (macOS EditorModel.reset). */
    fun resetAdjustments() {
        val catalog = catalog ?: return
        val scan = catalog.stock(edit.film)?.positive ?: false
        edit = edit.copy(params = catalog.defaults.with("io.scan_film", scan.json()))
        match = newMatch()
        changed()
    }

    fun rotate(quarterTurns: Int) {
        val session = cropSession
        if (session != null) session.rotate(quarterTurns) else rotation = Crop.turns(rotation + quarterTurns)
    }

    // ---- Crop -----------------------------------------------------------------

    /** The photograph's size as decoded: what the crop and border measure. */
    fun sourceSize(): Pair<Int, Int>? = photoSize ?: frame?.bitmap?.let { it.width to it.height }

    /** The exported photograph's size before its border: cropped and turned. */
    fun outputSize(): Pair<Int, Int>? = sourceSize()?.let { (w, h) -> Crop.outputSize(crop, rotation, w, h) }

    fun beginCrop() {
        if (cropSession != null || exporting) return
        val (w, h) = sourceSize() ?: return
        if (frame == null) return
        showingOriginal = false
        cropSession = CropSession(crop, w, h, rotation)
    }

    fun endCrop(apply: Boolean) {
        val session = cropSession ?: return
        if (apply) {
            crop = session.committed()
            rotation = session.rotation
        }
        cropSession = null
        // A pending brightness match waited for the crop.
        dirty.trySend(Unit)
    }

    fun updateBorder(change: (BorderSettings) -> BorderSettings) {
        border = change(border)
    }

    // Grain lanes are null until edited: the engine then uses the film's
    // preset, which the catalog resolves (macOS EditorModel.stockNumber).
    fun filmIsBW() = catalog?.stock(edit.film)?.bw ?: false
    fun stockDefaults(path: String) = catalog?.stock(edit.film)?.grain?.get(path.substringAfterLast('.')) ?: emptyList()
    fun stockValues(path: String) = get(path).numbers().ifEmpty { stockDefaults(path) }

    fun setStock(path: String, index: Int, value: Double, interactive: Boolean) {
        val values = stockValues(path).toMutableList()
        if (index >= values.size) return
        values[index] = value
        val preset = stockDefaults(path)
        val atPreset = values.size == preset.size && values.indices.all { near(values[it], preset[it]) }
        set(path, if (atPreset) JsonNull else values.json(), interactive)
    }

    // ---- Opening ------------------------------------------------------------

    /**
     * Opens the photograph at `uri`: copied into the cache, where the engine
     * reads it. The edit stays, as the web app keeps it for the next photo.
     */
    fun open(uri: Uri) {
        viewModelScope.launch {
            busy = true
            try {
                val name = withContext(Dispatchers.IO) { displayName(uri) } ?: "photo.jpg"
                val file = withContext(Dispatchers.IO) {
                    val dir = File(context.cacheDir, "photo")
                    dir.deleteRecursively()
                    dir.mkdirs()
                    val target = File(dir, name.replace('/', '_'))
                    context.contentResolver.openInputStream(uri)!!.use { input ->
                        target.outputStream().use { input.copyTo(it, 1 shl 16) }
                    }
                    target
                }
                generation += 1
                photo = Photo(name, file, generation)
                frame = null
                original = null
                showingOriginal = false
                photoSize = null
                dimensions = null
                settledRenderedLongSide = 0
                viewportZoom = 1f
                rotation = 0
                crop = CropSettings()
                cropSession = null
                lastKey = null
                thumbnails.clear()
                presetThumbnails.clear()
                // The exposure above the last photograph's match carries
                // over; the match is solved anew (macOS EditorModel.open).
                val offset = (get("prep.exposure_ev").number() ?: 0.0) - (match?.ev ?: 0.0)
                raw = file.extension.lowercase() in RAW_EXTENSIONS
                match = newMatch()
                matchFailure = null
                referenceFor = null
                edit = edit.copy(params = edit.params.with("prep.exposure_ev", offset.coerceIn(-3.0, 3.0).json()))
                status = "Developing $name…"
                changed()
            } catch (e: Exception) {
                message = Message("Couldn't open the photo: ${e.message}", error = true)
            } finally {
                busy = false
            }
        }
    }

    /** The web app's sample photograph (web/sample), shipped in the app. */
    fun openSample() {
        viewModelScope.launch {
            val file = withContext(Dispatchers.IO) {
                File(context.cacheDir, "DSC_6377.dng").also { target ->
                    if (!target.exists()) context.assets.open("samples/DSC_6377.dng").use { input -> target.outputStream().use { input.copyTo(it) } }
                }
            }
            open(Uri.fromFile(file))
        }
    }

    private fun displayName(uri: Uri): String? = if (uri.scheme == "file") uri.lastPathSegment else
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }

    // ---- Rendering ----------------------------------------------------------

    private fun renderFields(photo: Photo, edit: Edit, previewPath: File, scale: Double?): JsonObject = buildJsonObject {
        put("input", photo.file.path)
        put("source_generation", photo.generation)
        put("film", edit.film)
        put("paper", edit.paper)
        put("params", edit.params)
        put("limit_preview", true)
        put("preview_path", previewPath.path)
        if (scale != null && scale < 1.0) put("interactive_scale", scale)
    }

    /**
     * The newest edit, once: a settled frame sized to the fitted viewport
     * (growing on zoom up to ~4.4 MP), or while a slider moves an adaptive
     * smaller frame targeting [BUDGET_MS] (web/app.js). Edits made meanwhile
     * wait for it, and only the last of them renders next.
     */
    private suspend fun render() {
        val photo = photo ?: return
        val edit = edit
        val interactive = dragging
        val scale = dimensions?.let { (w, h) ->
            val photoLong = max(w, h).toDouble()
            val settledSide = settledLongSide(w, h)
            val targetSide = if (interactive) {
                max(MIN_INTERACTIVE_SIDE, (settledSide * interactiveScale).roundToInt()).coerceAtMost(settledSide)
            } else {
                settledSide
            }
            (targetSide.toDouble() / photoLong).coerceAtMost(1.0)
        } ?: 0.25
        val key = listOf(photo.generation, edit, (scale * 1000.0).roundToInt()).toString()
        if (key == lastKey) return
        busy = true
        val preview = File(context.cacheDir, "preview-${photo.generation % 2}.rgba")
        val t0 = SystemClock.elapsedRealtime()
        try {
            val result = engine.call("render", renderFields(photo, edit, preview, scale), timeoutMs = 45_000L)
            val width = result["width"].number()!!.toInt()
            val height = result["height"].number()!!.toInt()
            if (dimensions == null) {
                val pw = result["photo_width"].number()?.toInt()
                val ph = result["photo_height"].number()?.toInt()
                if (pw != null && ph != null) {
                    dimensions = pw to ph
                    // The first frame was a quick one; the settled frame follows.
                    dirty.trySend(Unit)
                }
            }
            val sw = result["source_width"].number()?.toInt()
            val sh = result["source_height"].number()?.toInt()
            if (sw != null && sh != null) photoSize = sw to sh
            val bitmap = withContext(Dispatchers.IO) { readRgba(preview, width, height) }
            val frameWallMs = (SystemClock.elapsedRealtime() - t0).toDouble()
            if (interactive) {
                val elapsed = result["elapsed_ms"].number() ?: frameWallMs
                interactiveScale = nextInteractiveScale(interactiveScale, max(elapsed, frameWallMs))
            } else {
                settledRenderedLongSide = max(width, height)
            }
            if (photo.generation == this.photo?.generation) {
                frame = Frame(bitmap, edit, interactive)
                lastKey = key
                val ms = result["elapsed_ms"].number()?.roundToInt()
                status = "${width}×${height} · ${result["backend"].text() ?: ""} · $ms ms"
            }
        } catch (e: EngineDied) {
            message = Message("${e.message}. Rendering again…", error = true)
            delay(300)
            dirty.trySend(Unit)
        } catch (e: Exception) {
            lastKey = key
            message = Message(e.message ?: "The render failed", error = true)
        } finally {
            busy = false
        }
        if (this.edit != edit || dragging != interactive) dirty.trySend(Unit)
    }

    private fun readRgba(file: File, width: Int, height: Int): Bitmap {
        val bytes = file.readBytes()
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(bytes, 0, width * height * 4))
        return bitmap
    }

    /** The photograph turned as it's shown and exported. */
    fun turned(bitmap: Bitmap): Bitmap {
        if (rotation == 0) return bitmap
        val matrix = Matrix().apply { postRotate(90f * rotation) }
        return Bitmap.createBitmap(bitmap, 0, 0, bitmap.width, bitmap.height, matrix, true)
    }

    /** Show Original: the photograph before any film, while held. */
    fun holdOriginal(hold: Boolean) {
        showingOriginal = hold
        if (!hold || original != null) return
        val photo = photo ?: return
        viewModelScope.launch {
            val file = File(context.cacheDir, "original.rgba")
            try {
                val result = engine.call("original", buildJsonObject {
                    put("input", photo.file.path)
                    put("params", edit.params)
                    put("limit_preview", true)
                    put("preview_path", file.path)
                })
                val w = result["width"].number()!!.toInt()
                val h = result["height"].number()!!.toInt()
                val bitmap = withContext(Dispatchers.IO) { readRgba(file, w, h) }
                if (photo.generation == this@Editor.photo?.generation) original = bitmap
            } catch (e: Exception) {
                message = Message(e.message ?: "Couldn't show the original", error = true)
            }
        }
    }

    // ---- Stock previews -----------------------------------------------------

    /** Stock previews by stock id, for the edit they were made for. */
    val thumbnails = mutableStateMapOf<String, Pair<String, Bitmap>>()
    private val thumbnailJobs = mutableMapOf<String, Job>()

    /** The edit a film's or paper's tile shows: this one with that stock. */
    fun tileEdit(stock: Stock): Edit = if (stock.paper) paperEdit(edit, stock.id) else filmEdit(edit, stock.id)

    /** What a tile shows: the photograph, its crop and the tile's edit. */
    fun tileKey(stock: Stock): String? = photo?.let { listOf(it.generation, crop.rect, crop.angle, tileEdit(stock)).toString() }

    /** Asks for `stock`'s preview, behind the photograph's own renders. */
    fun wantThumbnail(stock: Stock, longSide: Int) {
        val photo = photo ?: return
        val tile = tileEdit(stock)
        val cut = crop
        val key = tileKey(stock) ?: return
        if (thumbnails[stock.id]?.first == key || thumbnailJobs[stock.id]?.isActive == true) return
        thumbnailJobs[stock.id] = viewModelScope.launch {
            val file = File(context.cacheDir, "thumbnails/${stock.id}.rgba").apply { parentFile?.mkdirs() }
            try {
                val result = engine.call("thumbnail", buildJsonObject {
                    put("input", photo.file.path)
                    put("source_generation", photo.generation)
                    put("film", tile.film)
                    put("paper", tile.paper)
                    put("params", tile.params)
                    put("long_side", longSide.coerceIn(16, 1024))
                    put("preview_path", file.path)
                    if (!cut.isFull) put("crop", cut.json())
                }, urgent = false)
                val w = result["width"].number()!!.toInt()
                val h = result["height"].number()!!.toInt()
                val bitmap = withContext(Dispatchers.IO) { readRgba(file, w, h) }
                if (photo.generation == this@Editor.photo?.generation) thumbnails[stock.id] = key to bitmap
            } catch (e: Exception) {
                Log.w(Engine.TAG, "thumbnail ${stock.id}: ${e.message}")
            }
        }
    }

    // ---- Actions ------------------------------------------------------------

    var balancing by mutableStateOf(false); private set

    /** The M/Y shifts that print a mid-grey neutral (macOS EditorModel.balancePrint). */
    fun balancePrint() {
        if (balancing) return
        val asked = edit
        viewModelScope.launch {
            balancing = true
            try {
                val r = engine.call("balance_print", buildJsonObject {
                    put("film", asked.film)
                    put("paper", asked.paper)
                    put("params", asked.params)
                })
                val m = r["m_filter_shift"].number() ?: 0.0
                val y = r["y_filter_shift"].number() ?: 0.0
                edit = edit.copy(params = edit.params.with("enlarger.m_filter_shift", m.json()).with("enlarger.y_filter_shift", y.json()))
                changed()
                message = Message("Print balanced: magenta %.2f, yellow %.2f".format(m, y))
            } catch (e: Exception) {
                message = Message("Balance Print failed: ${e.message}", error = true)
            } finally {
                balancing = false
            }
        }
    }

    fun dismissMessage() {
        message = null
    }

    fun notify(text: String, error: Boolean = true) {
        message = Message(text, error)
    }

    /**
     * The edit at full size as a JPEG in Pictures/Negs: rendered again
     * without the preview's scale, then saved by the engine with the
     * rotation and border, as the desktop apps export.
     */
    fun export() {
        val photo = photo ?: return
        if (exporting) return
        if (cropSession != null) return
        val turns = rotation
        val cut = crop
        val border = border
        viewModelScope.launch {
            exporting = true
            // A pending brightness match first: the export is what it shows.
            awaitMatch()
            val asked = edit
            message = Message(photoSize?.let { (w, h) -> "Rendering ${w}×${h} for export…" } ?: "Rendering for export…")
            val dir = File(context.cacheDir, "export").apply { deleteRecursively(); mkdirs() }
            val name = "${photo.name.substringBeforeLast('.')}-${asked.film}.jpg"
            val out = File(dir, name)
            try {
                engine.exclusive {
                    val rendered = send("render", buildJsonObject {
                        put("input", photo.file.path)
                        put("source_generation", photo.generation)
                        put("film", asked.film)
                        put("paper", asked.paper)
                        put("params", asked.params)
                        // The whole photograph: the preview's cap is for the screen.
                        // Omit preview_path to skip writing an unused 90-170 MiB RGBA file.
                        put("limit_preview", false)
                    })
                    val renderId = rendered["request_id"].number()!!.toLong()
                    val (w, h) = Crop.outputSize(
                        cut, turns, rendered["output_width"].number()?.toInt() ?: 0, rendered["output_height"].number()?.toInt() ?: 0,
                    )
                    send("save", buildJsonObject {
                        // The engine keeps its last render for Save; its
                        // request id names it.
                        put("render_id", renderId)
                        put("output", out.path)
                        put("save_options", buildJsonObject {
                            put("jpeg_quality", 92)
                            put("copy_metadata", true)
                        })
                        put("crop_after_post", true)
                        if (!cut.isFull) put("crop", cut.json())
                        put("rotate_cw", turns)
                        if (border.exported(w, h)) {
                            put("border", border.style.id)
                            put("border_options", border.options())
                            put("instagram", border.instagram)
                        }
                    })
                }
                val uri = withContext(Dispatchers.IO) { publish(out) }
                message = Message("Saved to Pictures/Negs", share = uri)
            } catch (e: Exception) {
                message = Message("Export failed: ${e.message}", error = true)
            } finally {
                exporting = false
                // The preview's frame comes back at its own size.
                lastKey = null
                dirty.trySend(Unit)
            }
        }
    }

    /** Copies the exported file into the photo library (Pictures/Negs). */
    private fun publish(file: File): Uri {
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, file.name)
            put(MediaStore.Images.Media.MIME_TYPE, "image/jpeg")
            put(MediaStore.Images.Media.RELATIVE_PATH, "${Environment.DIRECTORY_PICTURES}/Negs")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)!!
        resolver.openOutputStream(uri)!!.use { out -> file.inputStream().use { it.copyTo(out) } }
        values.clear()
        values.put(MediaStore.Images.Media.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
        return uri
    }

    fun shareIntent(uri: Uri): Intent = Intent.createChooser(
        Intent(Intent.ACTION_SEND).apply {
            type = "image/jpeg"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        },
        null,
    )

    // ---- Prep › Match camera brightness ---------------------------------------

    /**
     * The match while it's on (web/app.js `state.match`, macOS EditorModel
     * §Camera brightness): the EV it solved, the inputs it solved for, and
     * whether highlights held it back.
     */
    data class Match(val ev: Double? = null, val inputs: String? = null, val limited: Boolean = false)
    /** A failed match: for the photograph ([inputs] null) or until these inputs change. */
    data class MatchFailure(val generation: Long, val inputs: String?, val message: String)

    var match by mutableStateOf<Match?>(null); private set
    var matchFailure by mutableStateOf<MatchFailure?>(null); private set
    var matching by mutableStateOf(false); private set
    /** The photograph generation the camera reference file was made for. */
    private var referenceFor: Long? = null

    private val matchNewRaws get() = prefs.getBoolean("matchNewRaws", true)
    private fun newMatch() = if (raw && matchNewRaws) Match() else null

    /** What the match depends on: all that sets the developed brightness except the user's own. */
    private fun matchInputs(): String {
        var p = edit.params
        for (path in MATCH_IGNORES) p = p.with(path, JsonNull)
        return listOf(edit.film, edit.paper, p).toString()
    }

    private fun matchUnavailable() = matchFailure?.takeIf { it.generation == photo?.generation && it.inputs == null }?.message
    private fun matchFailedHere() = matchFailure?.let { it.generation == photo?.generation && it.inputs != null && it.inputs == matchInputs() } ?: false
    /** On, possible, and not failed for these settings. */
    fun matchActive() = match != null && raw && matchUnavailable() == null && !matchFailedHere()
    /** On and not solved for these settings: an export waits. */
    fun matchPending() = matchActive() && match!!.let { it.ev == null || it.inputs != matchInputs() }
    fun matchedEv() = if (matchActive()) match?.ev ?: 0.0 else 0.0

    /** The note under the Exposure slider. */
    fun matchNote(): String? {
        val m = match ?: return null
        if (!raw) return null
        matchUnavailable()?.let { return it }
        if (matchFailedHere()) return matchFailure?.message
        if (matchPending() || matching) return "Matching…"
        val ev = m.ev ?: return null
        val value = "${if (ev >= 0) "+" else "−"}${"%.2f".format(kotlin.math.abs(ev))}"
        return if (m.limited) "Starts at $value EV to match the camera, held back to protect highlights."
        else "Starts at $value EV to match the camera."
    }

    /** On: from the match, Exposure back to 0. Off: the brightness stays, all of it Exposure's. */
    fun setMatch(on: Boolean) {
        prefs.edit().putBoolean("matchNewRaws", on).apply()
        if (on == (match != null)) return
        if (on) {
            match = Match()
            edit = edit.copy(params = edit.params.with("prep.exposure_ev", 0.0.json()))
        } else {
            match = null
        }
        changed()
    }

    /** Prep's Exposure: the user's exposure above the match, within the engine's ±3 EV in all. */
    fun exposureOffset() = (get("prep.exposure_ev").number() ?: 0.0) - matchedEv()

    fun setExposureOffset(value: Double, interactive: Boolean) {
        val m = matchedEv()
        val offset = ((value / 0.05).roundToInt() * 0.05).coerceIn(-3 - m, 3 - m)
        set("prep.exposure_ev", (if (kotlin.math.abs(offset) < 1e-9) m else m + offset).coerceIn(-3.0, 3.0).json(), interactive)
    }

    /**
     * Solves the match when it's on and stale: the camera's own preview as
     * the reference (`brightness_reference`, once a photograph), then the
     * exposure that develops the RAW as bright (`place_exposure`). A result
     * applies only if the inputs it was solved for are still current.
     */
    private suspend fun reconcileMatch() {
        if (matching || !matchPending()) return
        val photo = photo ?: return
        val (w, h) = photoSize ?: return
        val inputs = matchInputs()
        val asked = edit
        // The solve's own exposure: Prep and the print exposure are the user's.
        val params = asked.params
            .with("prep", defaultValue("prep") ?: JsonNull)
            .with("enlarger.print_exposure", defaultValue("enlarger.print_exposure") ?: JsonNull)
        val reference = File(context.cacheDir, "camera-brightness.png")
        matching = true
        try {
            if (referenceFor != photo.generation) {
                val r = engine.call("brightness_reference", buildJsonObject {
                    put("input", photo.file.path)
                    put("reference_path", reference.path)
                    put("expected_width", w)
                    put("expected_height", h)
                })
                if (r["usable"].bool() == false) {
                    if (photo.generation == this.photo?.generation) {
                        matchFailure = MatchFailure(photo.generation, null, r["reason"].text() ?: "The camera preview couldn’t be read.")
                    }
                    return
                }
                referenceFor = photo.generation
            }
            val result = engine.call("place_exposure", buildJsonObject {
                put("input", photo.file.path)
                put("source_generation", photo.generation)
                put("film", asked.film)
                put("paper", asked.paper)
                put("params", params)
                put("reference_path", reference.path)
            }, timeoutMs = 45_000L)
            if (photo.generation != this.photo?.generation || match == null || matchInputs() != inputs) return
            val ev = result["exposure_ev"].number()
            Log.i(Engine.TAG, "matched camera brightness: $ev EV in ${result["elapsed_ms"].number()?.roundToInt()} ms")
            if (ev == null || !ev.isFinite() || kotlin.math.abs(ev) > 3) {
                matchFailure = MatchFailure(photo.generation, inputs, "No reliable brightness match with these settings.")
            } else {
                applyMatch(ev, result["highlight_limited"].bool() == true, inputs)
            }
        } catch (e: EngineDied) {
            // The render that follows starts it again; the match comes after.
            referenceFor = null
        } catch (e: Exception) {
            if (photo.generation != this.photo?.generation || matchInputs() != inputs) return
            Log.w(Engine.TAG, "no camera brightness match: ${e.message}")
            val (message, photograph) = failureReason(e.message ?: "")
            matchFailure = MatchFailure(photo.generation, if (photograph) null else inputs, message)
        } finally {
            matching = false
        }
    }

    /** Moves the total with the match, keeping the user's exposure above it; under 0.05 EV it only marks it current. */
    private fun applyMatch(ev: Double, limited: Boolean, inputs: String) {
        val m = match ?: return
        val offset = (get("prep.exposure_ev").number() ?: 0.0) - (m.ev ?: 0.0)
        var next = m
        if (m.ev == null || kotlin.math.abs(ev - m.ev) >= 0.05) {
            next = next.copy(ev = ev)
            edit = edit.copy(params = edit.params.with("prep.exposure_ev", (if (kotlin.math.abs(offset) < 1e-9) ev else ev + offset).coerceIn(-3.0, 3.0).json()))
            changed()
        }
        match = next.copy(inputs = inputs, limited = limited)
    }

    /** Until a pending match is solved, for an export. */
    private suspend fun awaitMatch() {
        repeat(1200) {
            if (!matchPending() && !matching) return
            if (!matching) reconcileMatch() else delay(25)
        }
    }

    // ---- Border textures -------------------------------------------------------

    /** The texture the border draws over the photograph, by its path under the data. */
    var borderTexture by mutableStateOf<Pair<String, Bitmap>?>(null); private set
    private var textureLoading: String? = null

    /** The texture border `b` needs around a `w` × `h` photograph, or null. */
    fun textureKey(w: Int, h: Int): String? {
        if (!border.shown(w, h)) return null
        return when (border.style) {
            BorderStyle.Film -> BorderLayout.FILM_TEXTURE
            BorderStyle.Strip35 -> BorderLayout.filmFormat(w, h)?.let { BorderLayout.stripPath(it, edit.film) }
            else -> null
        }
    }

    /**
     * Loads a border's texture at about the screen's size (the strip is
     * 3800 × 3500, 53 MB whole). Films without an edge print get the
     * generic strip.
     */
    fun wantTexture(key: String) {
        if (borderTexture?.first == key || textureLoading == key) return
        val data = dataDir ?: return
        textureLoading = key
        viewModelScope.launch {
            val bitmap = withContext(Dispatchers.IO) {
                var file = File(data, key)
                if (!file.exists() && key.startsWith("borders/35mm/")) file = File(file.parentFile, "generic.png")
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeFile(file.path, bounds)
                var sample = 1
                val long = max(bounds.outWidth, bounds.outHeight)
                while (long / (sample * 2) >= memory.previewLongSide * 3 / 4) sample *= 2
                BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
            }
            if (textureLoading == key) textureLoading = null
            if (bitmap != null) borderTexture = key to bitmap
        }
    }

    // ---- LUT ---------------------------------------------------------------------

    /** The `.cube` name desktop suggests (File › Export LUT…). */
    fun lutName() = "${edit.film}_${edit.paper}.cube"

    /** A 33³ `.cube` of the film and paper as set, written to `uri`. */
    fun exportLut(uri: Uri) {
        val asked = edit
        viewModelScope.launch {
            val file = File(context.cacheDir, "export.cube")
            message = Message("Exporting LUT…")
            try {
                engine.call("export_lut", buildJsonObject {
                    put("film", asked.film)
                    put("paper", asked.paper)
                    put("params", asked.params)
                    put("size", 33)
                    put("output", file.path)
                })
                withContext(Dispatchers.IO) {
                    context.contentResolver.openOutputStream(uri, "wt")!!.use { out -> file.inputStream().use { it.copyTo(out) } }
                }
                message = Message("LUT exported")
            } catch (e: Exception) {
                message = Message("LUT export failed: ${e.message}", error = true)
            } finally {
                file.delete()
            }
        }
    }

    // ---- Favourites ----------------------------------------------------------------

    /** Starred films and papers, in the order starred (web/app.js favorites). */
    var favoriteFilms by mutableStateOf(readFavorites("favoriteFilms", DEFAULT_FAVORITE_FILMS)); private set
    var favoritePapers by mutableStateOf(readFavorites("favoritePapers", emptyList())); private set

    private fun readFavorites(key: String, defaults: List<String>): List<String> =
        prefs.getString(key, null)?.split(',')?.filter { it.isNotEmpty() } ?: defaults

    fun isFavorite(stock: Stock) = stock.id in (if (stock.paper) favoritePapers else favoriteFilms)

    fun toggleFavorite(stock: Stock) {
        val list = if (stock.paper) favoritePapers else favoriteFilms
        val next = if (stock.id in list) list - stock.id else list + stock.id
        if (stock.paper) favoritePapers = next else favoriteFilms = next
        prefs.edit().putString(if (stock.paper) "favoritePapers" else "favoriteFilms", next.joinToString(",")).apply()
    }

    // ---- Presets -------------------------------------------------------------------

    /*
     * The presets in the app's folder (macOS Presets pane, docs/presets-design.md):
     * Python spektrafilm GUI-state files, in desktop's format and the web's
     * links. Applying one sets the film, paper and Lab settings; Prep, the
     * crop, rotation, border and Post stay (presets::KEPT).
     */
    private val presetLibrary by lazy { PresetLibrary(File(context.filesDir, "presets")) }
    /** One change to the folder at a time. */
    private val presetLock = Mutex()
    var presets by mutableStateOf<List<PresetFile>>(emptyList()); private set
    /** What the engine made of each file, as listed. */
    val presetStatus = mutableStateMapOf<PresetFile, PresetStatus>()
    /** The name key of the preset applied or saved last: its tile is ringed. */
    var appliedPreset by mutableStateOf<String?>(null); private set
    /** Counts requests to show the Presets tab (a preset arrived from another app). */
    var presetsShown by mutableStateOf(0); private set

    val canChangePresets get() = catalog != null && cropSession == null && !exporting

    /** Lists the folder again and asks the engine about files it hasn't read. */
    private suspend fun reloadPresets() {
        val listed = try {
            withContext(Dispatchers.IO) { presetLibrary.list() }
        } catch (e: Exception) {
            message = Message("Couldn’t list the presets: ${e.message}", error = true)
            emptyList()
        }
        presets = listed
        (presetStatus.keys - listed.toSet()).forEach { presetStatus.remove(it) }
        val paths = listed.map { it.file.path }.toSet()
        (presetThumbnails.keys - paths).forEach { presetThumbnails.remove(it) }
        for (preset in listed) {
            if (presetStatus[preset] != null) continue
            presetStatus[preset] = try {
                val text = withContext(Dispatchers.IO) { PresetLibrary.text(presetLibrary.read(preset)) }
                validate(text)
            } catch (e: Exception) {
                PresetStatus.Invalid(e.message ?: e.toString())
            }
        }
    }

    /** What the engine reads in preset [text]; throws if it can't be used here. */
    private suspend fun validate(text: String): PresetStatus.Valid {
        val r = engine.call("preset_import", buildJsonObject { put("text", text) }, urgent = false)
        val film = r["film"].text() ?: throw PresetError("The engine didn’t read a film.")
        val paper = r["paper"].text() ?: throw PresetError("The engine didn’t read a paper.")
        if (catalog?.stock(film) == null || catalog?.stock(paper) == null) {
            throw PresetError("This version of Negs doesn’t have the preset’s film or paper.")
        }
        return PresetStatus.Valid(film, paper, r["params"].at("io.scan_film").bool() == true, text)
    }

    /**
     * The film, paper and Lab settings of [preset] (macOS EditorModel.applyPreset).
     * The engine keeps what a preset doesn't carry from the params it's
     * given; asked again if they changed while it answered.
     */
    fun applyPreset(preset: PresetFile) {
        if (!canChangePresets) return
        val status = presetStatus[preset] as? PresetStatus.Valid ?: return
        viewModelScope.launch {
            try {
                repeat(3) {
                    val snapshot = edit
                    val r = engine.call("preset_import", buildJsonObject {
                        put("text", status.text)
                        put("params", snapshot.params)
                    })
                    if (edit != snapshot) return@repeat
                    val film = r["film"].text()!!
                    val paper = r["paper"].text()!!
                    if (catalog?.stock(film) == null || catalog?.stock(paper) == null) {
                        throw PresetError("This version of Negs doesn’t have the preset’s film or paper.")
                    }
                    edit = Edit(film, paper, r["params"] as JsonObject)
                    changed()
                    appliedPreset = PresetName.key(preset.name)
                    message = Message("Applied “${preset.name}”")
                    return@launch
                }
            } catch (e: Exception) {
                message = Message("Couldn’t apply “${preset.name}”: ${e.message}", error = true)
                reloadPresets()
            }
        }
    }

    /** The name a new preset starts with: the film's, made unique. */
    fun suggestedPresetName(): String =
        PresetName.unique(PresetName.from(catalog?.stock(edit.film)?.name ?: edit.film), presets.map { PresetName.key(it.name) }.toSet())

    /** Why [name] can't name a new preset (or [renaming] renamed), or null. */
    fun presetNameProblem(name: String, renaming: PresetFile? = null): String? =
        PresetName.problem(name) ?: name.takeIf { n ->
            presets.any { it != renaming && PresetName.key(it.name) == PresetName.key(n) }
        }?.let { "A preset named “$it” already exists." }

    /** Saves the film, paper and Lab settings as a new preset named [name]. */
    fun newPreset(name: String) {
        if (!canChangePresets) return
        val snapshot = edit
        viewModelScope.launch {
            try {
                val r = engine.call("preset_export", buildJsonObject {
                    put("film", snapshot.film)
                    put("paper", snapshot.paper)
                    put("params", snapshot.params)
                })
                val text = r["text"].text()!!
                val file = presetLock.withLock { withContext(Dispatchers.IO) { presetLibrary.add(text.toByteArray(), name) } }
                // The edit's settings are this preset's: it's the one ringed.
                appliedPreset = PresetName.key(file.name)
                reloadPresets()
                message = Message("Saved “${file.name}”")
            } catch (e: Exception) {
                message = Message("Couldn’t save the preset: ${e.message}", error = true)
            }
        }
    }

    fun renamePreset(preset: PresetFile, name: String) {
        viewModelScope.launch {
            try {
                val renamed = presetLock.withLock { withContext(Dispatchers.IO) { presetLibrary.rename(preset, name) } }
                // The same file: what's known about it moves with it.
                presetStatus.remove(preset)?.let { presetStatus[renamed] = it }
                presetThumbnails.remove(preset.file.path)?.let { presetThumbnails[renamed.file.path] = it }
                if (appliedPreset == PresetName.key(preset.name)) appliedPreset = PresetName.key(renamed.name)
            } catch (e: Exception) {
                message = Message("Couldn’t rename “${preset.name}”: ${e.message}", error = true)
            }
            reloadPresets()
        }
    }

    fun deletePreset(preset: PresetFile) {
        viewModelScope.launch {
            try {
                presetLock.withLock { withContext(Dispatchers.IO) { presetLibrary.delete(preset) } }
                if (appliedPreset == PresetName.key(preset.name)) appliedPreset = null
                message = Message("Deleted “${preset.name}”")
            } catch (e: Exception) {
                message = Message("Couldn’t delete “${preset.name}”: ${e.message}", error = true)
            }
            reloadPresets()
        }
    }

    /** Moves [preset] [by] places in the list, and remembers the order. */
    fun movePreset(preset: PresetFile, by: Int) {
        val list = presets.toMutableList()
        val from = list.indexOf(preset)
        if (from < 0) return
        val to = (from + by).coerceIn(0, list.lastIndex)
        if (to == from) return
        list.add(to, list.removeAt(from))
        presets = list
        viewModelScope.launch {
            try {
                presetLock.withLock { withContext(Dispatchers.IO) { presetLibrary.setOrder(list) } }
            } catch (e: Exception) {
                message = Message("Couldn’t save the order: ${e.message}", error = true)
                reloadPresets()
            }
        }
    }

    /**
     * Copies each file the engine reads into the presets, under its own
     * name ("name 2"… if taken), and says which it refused.
     */
    fun importPresets(uris: List<Uri>) {
        if (uris.isEmpty()) return
        presetsShown++
        viewModelScope.launch {
            started.await()
            val failures = mutableListOf<String>()
            val added = mutableListOf<String>()
            for (uri in uris) {
                val fileName = withContext(Dispatchers.IO) { runCatching { displayName(uri) }.getOrNull() } ?: "Preset.json"
                try {
                    val data = withContext(Dispatchers.IO) { readLimited(uri) }
                    validate(PresetLibrary.text(data))
                    val file = presetLock.withLock {
                        withContext(Dispatchers.IO) { presetLibrary.add(data, PresetName.from(PresetName.stem(fileName))) }
                    }
                    added += file.name
                } catch (e: Exception) {
                    failures += "$fileName: ${e.message}"
                }
            }
            reloadPresets()
            message = when {
                failures.isEmpty() -> Message(if (added.size == 1) "Added “${added[0]}” to the presets" else "Added ${added.size} presets")
                added.isEmpty() -> Message((if (failures.size == 1) "Couldn’t import " else "No presets were imported. ") + failures.joinToString("\n"), error = true)
                else -> Message("Added ${added.size} presets. Not imported: " + failures.joinToString("\n"), error = true)
            }
        }
    }

    /** A preset link shared to Negs (web/presets.js links); false if [text] has none. */
    fun importPresetLink(text: String): Boolean {
        val value = PresetLink.find(text) ?: return false
        presetsShown++
        viewModelScope.launch {
            started.await()
            try {
                val (name, preset) = PresetLink.decode(value)
                validate(preset)
                val file = presetLock.withLock { withContext(Dispatchers.IO) { presetLibrary.add(preset.toByteArray(), name) } }
                reloadPresets()
                message = Message("Added “${file.name}” to the presets")
            } catch (e: Exception) {
                message = Message("Couldn’t add the preset: ${e.message}", error = true)
            }
        }
        return true
    }

    /** A file's bytes, if it's small enough to be a preset. */
    private fun readLimited(uri: Uri): ByteArray {
        context.contentResolver.openInputStream(uri)!!.use { input ->
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(1 shl 16)
            while (true) {
                val n = input.read(buffer)
                if (n < 0) break
                out.write(buffer, 0, n)
                if (out.size() > PresetLibrary.MAX_FILE_BYTES) throw PresetError("The file is too large to be a preset.")
            }
            return out.toByteArray()
        }
    }

    /**
     * Shares [preset] as its file, with a link that opens it in the web app,
     * through [launch] (the activity's startActivity).
     */
    fun sharePreset(preset: PresetFile, launch: (Intent) -> Unit) {
        viewModelScope.launch {
            try {
                val (file, link) = withContext(Dispatchers.IO) {
                    val data = presetLibrary.read(preset)
                    val dir = File(context.cacheDir, "shared").apply { deleteRecursively(); mkdirs() }
                    val file = File(dir, "${preset.name}.json").apply { writeBytes(data) }
                    file to PresetLink.encode(preset.name, PresetLibrary.text(data))
                }
                val uri = androidx.core.content.FileProvider.getUriForFile(context, "${context.packageName}.files", file)
                val send = Intent(Intent.ACTION_SEND).apply {
                    type = "application/json"
                    putExtra(Intent.EXTRA_STREAM, uri)
                    putExtra(Intent.EXTRA_SUBJECT, preset.name)
                    if (link != null) putExtra(Intent.EXTRA_TEXT, link)
                    clipData = android.content.ClipData.newRawUri(preset.name, uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                launch(Intent.createChooser(send, "Share “${preset.name}”"))
            } catch (e: Exception) {
                message = Message("Couldn’t share “${preset.name}”: ${e.message}", error = true)
                reloadPresets()
            }
        }
    }

    /** Writes [preset]'s file to [uri] (Save as file…). */
    fun savePresetFile(preset: PresetFile, uri: Uri) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    val data = presetLibrary.read(preset)
                    context.contentResolver.openOutputStream(uri, "wt")!!.use { it.write(data) }
                }
                message = Message("Saved “${preset.name}.json”")
            } catch (e: Exception) {
                message = Message("Couldn’t save “${preset.name}”: ${e.message}", error = true)
            }
        }
    }

    /** Preset previews by file path, for the key they were made for. */
    val presetThumbnails = mutableStateMapOf<String, Pair<String, Bitmap>>()
    private val presetThumbnailJobs = mutableMapOf<String, Job>()

    /** What a preset's tile shows: the photograph, its crop, what the preset keeps, and the preset. */
    fun presetTileKey(preset: PresetFile): String? {
        val photo = photo ?: return null
        val status = presetStatus[preset] as? PresetStatus.Valid ?: return null
        val kept = catalog?.presetKept.orEmpty().map { edit.params.at(it) }
        return listOf(photo.generation, crop.rect, crop.angle, kept, status.text.length, status.text.hashCode()).toString()
    }

    /** Asks for [preset]'s preview: the photograph developed through it, behind the photograph's own renders. */
    fun wantPresetThumbnail(preset: PresetFile, longSide: Int) {
        val photo = photo ?: return
        val status = presetStatus[preset] as? PresetStatus.Valid ?: return
        val key = presetTileKey(preset) ?: return
        val path = preset.file.path
        if (presetThumbnails[path]?.first == key || presetThumbnailJobs[path]?.isActive == true) return
        val base = edit
        val cut = crop
        presetThumbnailJobs[path] = viewModelScope.launch {
            val file = File(context.cacheDir, "thumbnails/preset-${Integer.toHexString(path.hashCode())}.rgba").apply { parentFile?.mkdirs() }
            try {
                val result = engine.call("thumbnail", buildJsonObject {
                    put("input", photo.file.path)
                    put("source_generation", photo.generation)
                    put("film", base.film)
                    put("paper", base.paper)
                    put("params", base.params)
                    put("preset", status.text)
                    put("long_side", longSide.coerceIn(16, 1024))
                    put("preview_path", file.path)
                    if (!cut.isFull) put("crop", cut.json())
                }, urgent = false)
                val w = result["width"].number()!!.toInt()
                val h = result["height"].number()!!.toInt()
                val bitmap = withContext(Dispatchers.IO) { readRgba(file, w, h) }
                if (photo.generation == this@Editor.photo?.generation) presetThumbnails[path] = key to bitmap
            } catch (e: Exception) {
                Log.w(Engine.TAG, "preset thumbnail ${preset.name}: ${e.message}")
            }
        }
    }

    // ---- Memory -------------------------------------------------------------

    /**
     * Android asks for memory back: the stock previews' caches go first, the
     * kept film sides next; out of sight for good, the engine ends and the
     * next render starts it again.
     */
    fun trimMemory(level: Int) {
        if (!::engine.isInitialized) return
        viewModelScope.launch {
            try {
                if (level >= ComponentCallbacks2.TRIM_MEMORY_BACKGROUND) {
                    Log.i(Engine.TAG, "trim $level: stopping the engine")
                    engine.stop()
                    lastKey = null
                    return@launch
                }
                // Running low (before Android 14) or out of sight: the
                // engine's caches go; the tiles' bitmaps are small and stay,
                // unless memory is short while the editor is in use.
                if (level < ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN) {
                    thumbnails.clear()
                    presetThumbnails.clear()
                }
                // The border's texture loads again when it shows.
                borderTexture = null
                engine.call("release_thumbnails", urgent = false)
                engine.call("release_film_cache", urgent = false)
            } catch (e: Exception) {
                Log.w(Engine.TAG, "trim: ${e.message}")
            }
        }
    }

    /** Back in sight after the engine ended: render the photograph again. */
    fun resumed() {
        if (photo != null && frame != null && lastKey == null) dirty.trySend(Unit)
    }

    override fun onCleared() {
        if (::engine.isInitialized) engine.stop()
    }

    companion object {
        const val BUDGET_MS = 90.0
        const val MIN_INTERACTIVE_SCALE = 0.15
        const val MIN_INTERACTIVE_SIDE = 256

        /** Adaptive interactive slider scale step (web/app.js). */
        fun nextInteractiveScale(current: Double, elapsedMs: Double): Double =
            (current * sqrt(BUDGET_MS / max(elapsedMs, 1.0))).coerceIn(MIN_INTERACTIVE_SCALE, 1.0)

        val DEFAULT_FAVORITE_FILMS = listOf("kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200")

        /** Settings that can't change how bright the photograph develops, or are the user's own. */
        val MATCH_IGNORES = listOf(
            "prep", "enlarger.print_exposure", "post", "film_render.grain", "io.rotate_cw", "io.flip", "io.crop",
            "io.crop_center", "io.crop_size", "io.output_color_space", "io.output_cctf_encoding", "settings.serial_grain",
        )

        /**
         * The note for a failed solve, and whether it holds for the
         * photograph (spektrafilm-io brightness::failure_reason): the
         * engine's reasons are worded for logs.
         */
        fun failureReason(reason: String): Pair<String, Boolean> = when {
            "midtone quantiles" in reason -> "The camera preview is too dark or too bright to match." to true
            "±3 EV" in reason -> "Matching would take more than ±3 EV." to false
            else -> "No reliable brightness match with these settings." to false
        }

        val RAW_EXTENSIONS = setOf("dng", "raf", "arw", "nef", "cr2", "cr3", "rw2", "orf", "pef", "srw", "nrw", "raw", "rwl", "3fr", "iiq", "erf", "srf", "sr2", "x3f", "mrw", "kdc", "dcr")
    }
}
