package dev.negs.app

import android.app.ActivityManager
import android.content.Context
import android.os.Build
import android.os.Debug
import android.os.SystemClock
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * Real-device benchmark driving [Engine] directly (avoiding UI / onTrimMemory):
 * - Unpacks engine data once
 * - Measures 24 MP bundled sample (DSC_6377.dng) and 60 MP RAW (leica60mp.dng)
 * - Steps per photo: catalog -> quick preview -> settled screen-size preview ->
 *   brightness_reference + place_exposure -> full-size export render -> save JPEG
 * - Records backend, timings, Vulkan adapter/limits, engine VmHWM/VmRSS,
 *   app PSS, available RAM, and whether the engine was killed.
 */
@RunWith(AndroidJUnit4::class)
class EngineBenchmarkTest {

    private val tag = "NegsBench"
    private val events = Collections.synchronizedList(mutableListOf<JsonObject>())
    private var resultsFileRef: File? = null

    private fun logEvent(event: JsonObject) {
        events.add(event)
        Log.i(tag, event.toString())
        flushResults()
    }

    private fun flushResults() {
        val f = resultsFileRef ?: return
        runCatching {
            val snapshot = synchronized(events) { events.toList() }
            val payload = buildJsonObject { put("events", JsonArray(snapshot)) }
            f.writeText(payload.toString())
        }
    }

    private fun readProcStatus(pid: Int): JsonObject? {
        if (pid <= 0) return null
        val file = File("/proc/$pid/status")
        if (!file.canRead()) return null
        var vmHwm: Long? = null
        var vmRss: Long? = null
        var vmSwap: Long? = null
        var rssAnon: Long? = null
        runCatching {
            file.forEachLine { line ->
                val parts = line.trim().split(Regex("\\s+"))
                if (parts.size >= 2) {
                    val v = parts[1].toLongOrNull()
                    when (parts[0]) {
                        "VmHWM:" -> vmHwm = v
                        "VmRSS:" -> vmRss = v
                        "VmSwap:" -> vmSwap = v
                        "RssAnon:" -> rssAnon = v
                    }
                }
            }
        }.onFailure { return null }
        return buildJsonObject {
            put("pid", pid)
            vmHwm?.let { put("vm_hwm_kib", it) }
            vmRss?.let { put("vm_rss_kib", it) }
            vmSwap?.let { put("vm_swap_kib", it) }
            rssAnon?.let { put("rss_anon_kib", it) }
        }
    }

    private fun systemMemory(context: Context): JsonObject {
        val am = context.getSystemService(ActivityManager::class.java)
        val mi = ActivityManager.MemoryInfo()
        am.getMemoryInfo(mi)
        val dmi = Debug.MemoryInfo()
        Debug.getMemoryInfo(dmi)
        val rt = Runtime.getRuntime()
        return buildJsonObject {
            put("avail_mib", mi.availMem / (1024L * 1024L))
            put("total_mib", mi.totalMem / (1024L * 1024L))
            put("threshold_mib", mi.threshold / (1024L * 1024L))
            put("low_memory", mi.lowMemory)
            put("app_total_pss_kib", dmi.totalPss)
            put("app_native_pss_kib", dmi.nativePss)
            put("app_dalvik_pss_kib", dmi.dalvikPss)
            put("app_heap_used_mib", (rt.totalMemory() - rt.freeMemory()) / (1024L * 1024L))
        }
    }

    @Test
    fun benchmarkEngineOnDevice() {
        runBlocking {
            runBenchmark()
        }
    }

    private suspend fun runBenchmark() {
        val inst = InstrumentationRegistry.getInstrumentation()
        val appContext = inst.targetContext
        val testContext = inst.context
        val policy = MemoryPolicy.measure(appContext)
        val outDir = appContext.getExternalFilesDir(null) ?: appContext.filesDir
        outDir.mkdirs()
        val resultsFile = File(outDir, "bench_results.json")
        resultsFileRef = resultsFile

        val deviceInfo = buildJsonObject {
            put("event", "device_info")
            put("manufacturer", Build.MANUFACTURER)
            put("model", Build.MODEL)
            put("device", Build.DEVICE)
            put("hardware", Build.HARDWARE)
            put("board", Build.BOARD)
            if (Build.VERSION.SDK_INT >= 31) {
                put("soc_model", Build.SOC_MODEL)
                put("soc_manufacturer", Build.SOC_MANUFACTURER)
            }
            put("sdk_int", Build.VERSION.SDK_INT)
            put("abis", buildJsonArray { Build.SUPPORTED_ABIS.forEach { add(JsonPrimitive(it)) } })
            put("total_ram_mib", policy.totalBytes / (1024L * 1024L))
            put("low_ram", policy.lowRam)
            put("gpu_budget_mib", policy.gpuBudgetMiB)
            put("diffusion_grid_budget_mib", policy.diffusionGridBudgetBytes / (1024L * 1024L))
            put("preview_long_side", policy.previewLongSide)
            put("initial_viewport_long_side", policy.initialViewportLongSide)
            put("max_preview_source_pixels", policy.maxPreviewSourcePixels)
            put("initial_memory", systemMemory(appContext))
        }
        logEvent(deviceInfo)

        try {
            val tData = SystemClock.elapsedRealtime()
            val dataDir = EngineData.prepare(appContext)
            logEvent(buildJsonObject {
                put("event", "data_prepared")
                put("elapsed_ms", SystemClock.elapsedRealtime() - tData)
                put("data_dir", dataDir.path)
            })

            // 1. Bundled 24 MP sample
            val sample24 = File(appContext.cacheDir, "bench_24mp.dng")
            appContext.assets.open("samples/DSC_6377.dng").use { input ->
                sample24.outputStream().use { input.copyTo(it, 1 shl 16) }
            }
            runPhotoBenchmark(appContext, dataDir, policy, "sample_24mp", sample24)
            sample24.delete()

            // 2. 60 MP Leica Q3 RAW from androidTest assets (if packaged)
            val testSamples = testContext.assets.list("samples").orEmpty()
            if ("leica60mp.dng" in testSamples) {
                val sample60 = File(appContext.cacheDir, "bench_60mp.dng")
                testContext.assets.open("samples/leica60mp.dng").use { input ->
                    sample60.outputStream().use { input.copyTo(it, 1 shl 16) }
                }
                runPhotoBenchmark(appContext, dataDir, policy, "leica_60mp", sample60)
                sample60.delete()
            } else {
                logEvent(buildJsonObject {
                    put("event", "skipped")
                    put("photo", "leica_60mp")
                    put("reason", "samples/leica60mp.dng not present in androidTest assets")
                })
            }
        } finally {
            val payload = buildJsonObject {
                put("events", JsonArray(events))
            }
            resultsFile.writeText(payload.toString())
            Log.i(tag, "WROTE_RESULTS ${resultsFile.path} (${resultsFile.length()} bytes)")
        }
    }

    private suspend fun runPhotoBenchmark(
        context: Context,
        dataDir: File,
        policy: MemoryPolicy,
        label: String,
        photoFile: File,
    ) {
        val stderrLines = Collections.synchronizedList(mutableListOf<String>())
        val importantStderr = Collections.synchronizedList(mutableListOf<String>())
        val engine = Engine(context, dataDir, policy) { line ->
            synchronized(stderrLines) {
                if (stderrLines.size >= 80) stderrLines.removeAt(0)
                stderrLines.add(line)
            }
            if (line.contains("wgpu backend initialized") ||
                line.contains("GPU capacity exceeded") ||
                line.contains("software Vulkan") ||
                line.contains("using CPU backend") ||
                line.contains("WARN") ||
                line.contains("ERROR")
            ) {
                importantStderr.add(line)
            }
        }

        val workDir = File(context.cacheDir, "bench_$label").apply { deleteRecursively(); mkdirs() }
        val previewQuick = File(workDir, "preview_quick.rgba")
        val previewSettled = File(workDir, "preview_settled.rgba")
        val refPng = File(workDir, "camera_brightness.png")
        val exportPreview = File(workDir, "export_preview.rgba")
        val exportJpg = File(workDir, "export.jpg")

        suspend fun queryStats(): JsonElement = try {
            engine.call("process_stats")
        } catch (e: Exception) {
            JsonPrimitive("unavailable: ${e.message}")
        }

        val film = "kodak_portra_400"
        val paper = "kodak_portra_endura"

        try {
            // Step 1: catalog
            val tCat = SystemClock.elapsedRealtime()
            val catRaw = engine.call("catalog")
            val catMs = SystemClock.elapsedRealtime() - tCat
            val defaults = catRaw["defaults"]?.jsonObject ?: JsonObject(emptyMap())
            val catStats = queryStats()
            val enginePid = (catStats as? JsonObject)?.get("pid")?.number()?.toInt() ?: -1
            logEvent(buildJsonObject {
                put("event", "step")
                put("photo", label)
                put("step", "catalog")
                put("wall_ms", catMs)
                put("engine_stats", catStats)
                put("memory", systemMemory(context))
            })

            // Step 2: quick preview (0.25x, decodes source as Editor does)
            val tQuick = SystemClock.elapsedRealtime()
            val quickRes = engine.call("render", buildJsonObject {
                put("input", photoFile.path)
                put("source_generation", 1)
                put("film", film)
                put("paper", paper)
                put("params", defaults)
                put("limit_preview", true)
                put("preview_path", previewQuick.path)
                put("interactive_scale", 0.25)
            })
            val quickMs = SystemClock.elapsedRealtime() - tQuick
            val pw = quickRes["photo_width"].number()?.toInt() ?: 0
            val ph = quickRes["photo_height"].number()?.toInt() ?: 0
            val sw = quickRes["source_width"].number()?.toInt() ?: 0
            val sh = quickRes["source_height"].number()?.toInt() ?: 0
            logEvent(buildJsonObject {
                put("event", "step")
                put("photo", label)
                put("step", "preview_quick")
                put("wall_ms", quickMs)
                put("engine_ms", quickRes["elapsed_ms"] ?: JsonNull)
                put("backend", quickRes["backend"] ?: JsonNull)
                put("gpu_rgba8", quickRes["gpu_rgba8"] ?: JsonNull)
                put("width", quickRes["width"] ?: JsonNull)
                put("height", quickRes["height"] ?: JsonNull)
                put("source_width", sw)
                put("source_height", sh)
                put("photo_width", pw)
                put("photo_height", ph)
                put("engine_stats", queryStats())
                put("memory", systemMemory(context))
                put("stderr_notes", buildJsonArray { importantStderr.forEach { add(JsonPrimitive(it)) } })
            })
            previewQuick.delete()

            // Step 3a: settled fitted-viewport preview (matches Editor.kt default 1x view)
            val fittedSide = policy.initialViewportLongSide
            val settledScale = if (pw > 0 && ph > 0) {
                (fittedSide.toDouble() / max(pw, ph)).coerceAtMost(1.0)
            } else {
                0.5
            }
            val tPrev = SystemClock.elapsedRealtime()
            val prevRes = engine.call("render", buildJsonObject {
                put("input", photoFile.path)
                put("source_generation", 1)
                put("film", film)
                put("paper", paper)
                put("params", defaults)
                put("limit_preview", true)
                put("preview_path", previewSettled.path)
                if (settledScale < 1.0) put("interactive_scale", settledScale)
            })
            val prevMs = SystemClock.elapsedRealtime() - tPrev
            logEvent(buildJsonObject {
                put("event", "step")
                put("photo", label)
                put("step", "preview_settled")
                put("wall_ms", prevMs)
                put("engine_ms", prevRes["elapsed_ms"] ?: JsonNull)
                put("backend", prevRes["backend"] ?: JsonNull)
                put("gpu_rgba8", prevRes["gpu_rgba8"] ?: JsonNull)
                put("width", prevRes["width"] ?: JsonNull)
                put("height", prevRes["height"] ?: JsonNull)
                put("scale", settledScale)
                put("engine_stats", queryStats())
                put("memory", systemMemory(context))
            })
            previewSettled.delete()

            // Step 3b: interactive slider drag (8 consecutive frames adapting toward BUDGET_MS = 90)
            var adaptive = 0.5
            val dragFrames = mutableListOf<JsonObject>()
            val dragWallTimes = mutableListOf<Long>()
            val dragEvs = listOf(-0.6, -0.4, -0.2, 0.0, 0.2, 0.4, 0.6, 0.8)
            val previewDrag = File(workDir, "preview_drag.rgba")
            for ((idx, ev) in dragEvs.withIndex()) {
                val targetSide = max(Editor.MIN_INTERACTIVE_SIDE, (fittedSide * adaptive).roundToInt()).coerceAtMost(fittedSide)
                val dragScale = if (pw > 0 && ph > 0) {
                    (targetSide.toDouble() / max(pw, ph)).coerceAtMost(1.0)
                } else {
                    0.25
                }
                val dragParams = defaults.with("prep.exposure_ev", ev.json())
                val t0 = SystemClock.elapsedRealtime()
                val res = engine.call("render", buildJsonObject {
                    put("input", photoFile.path)
                    put("source_generation", 1)
                    put("film", film)
                    put("paper", paper)
                    put("params", dragParams)
                    put("limit_preview", true)
                    put("preview_path", previewDrag.path)
                    if (dragScale < 1.0) put("interactive_scale", dragScale)
                })
                val readBytes = previewDrag.readBytes().size
                val wallMs = SystemClock.elapsedRealtime() - t0
                val engineMs = res["elapsed_ms"].number() ?: wallMs.toDouble()
                dragWallTimes.add(wallMs)
                dragFrames.add(buildJsonObject {
                    put("frame", idx)
                    put("ev", ev)
                    put("width", res["width"] ?: JsonNull)
                    put("height", res["height"] ?: JsonNull)
                    put("wall_ms", wallMs)
                    put("engine_ms", engineMs)
                    put("bytes", readBytes)
                    put("gpu_rgba8", res["gpu_rgba8"] ?: JsonNull)
                    put("adaptive_scale", adaptive)
                })
                adaptive = Editor.nextInteractiveScale(adaptive, max(engineMs, wallMs.toDouble()))
            }
            previewDrag.delete()
            val last4Avg = dragWallTimes.takeLast(4).average()
            logEvent(buildJsonObject {
                put("event", "step")
                put("photo", label)
                put("step", "interactive_drag")
                put("frame_count", dragFrames.size)
                put("last4_avg_wall_ms", last4Avg)
                put("min_wall_ms", dragWallTimes.minOrNull() ?: 0L)
                put("max_wall_ms", dragWallTimes.maxOrNull() ?: 0L)
                put("final_adaptive_scale", adaptive)
                put("frames", JsonArray(dragFrames))
                put("engine_stats", queryStats())
                put("memory", systemMemory(context))
            })

            // Step 3c: zoomed settled preview (policy.previewLongSide, matching baseline 2340-2400 px
            // so we also verify Adreno 610 doesn't hang before place_exposure).
            val zoomedScale = if (pw > 0 && ph > 0) {
                (policy.previewLongSide.toDouble() / max(pw, ph)).coerceAtMost(1.0)
            } else {
                0.5
            }
            val tZoom = SystemClock.elapsedRealtime()
            val zoomRes = engine.call("render", buildJsonObject {
                put("input", photoFile.path)
                put("source_generation", 1)
                put("film", film)
                put("paper", paper)
                put("params", defaults)
                put("limit_preview", true)
                put("preview_path", previewSettled.path)
                if (zoomedScale < 1.0) put("interactive_scale", zoomedScale)
            })
            val zoomMs = SystemClock.elapsedRealtime() - tZoom
            logEvent(buildJsonObject {
                put("event", "step")
                put("photo", label)
                put("step", "preview_zoomed")
                put("wall_ms", zoomMs)
                put("engine_ms", zoomRes["elapsed_ms"] ?: JsonNull)
                put("backend", zoomRes["backend"] ?: JsonNull)
                put("gpu_rgba8", zoomRes["gpu_rgba8"] ?: JsonNull)
                put("width", zoomRes["width"] ?: JsonNull)
                put("height", zoomRes["height"] ?: JsonNull)
                put("scale", zoomedScale)
                put("engine_stats", queryStats())
                put("memory", systemMemory(context))
            })
            previewSettled.delete()

            // Step 4: brightness_reference + place_exposure (with 90s timeout in case GPU hangs)
            val tRef = SystemClock.elapsedRealtime()
            val refRes = engine.call("brightness_reference", buildJsonObject {
                put("input", photoFile.path)
                put("reference_path", refPng.path)
                put("expected_width", sw)
                put("expected_height", sh)
            })
            val refMs = SystemClock.elapsedRealtime() - tRef
            var matchEv: Double? = null
            var placeMs: Long = -1
            var placeEngineMs: JsonElement = JsonNull
            var placeError: String? = null
            var placeHung = false
            if (refRes["usable"].bool() == true) {
                val tPlace = SystemClock.elapsedRealtime()
                try {
                    val placeRes = withTimeout(90_000L) {
                        engine.call("place_exposure", buildJsonObject {
                            put("input", photoFile.path)
                            put("source_generation", 1)
                            put("film", film)
                            put("paper", paper)
                            put("params", defaults)
                            put("reference_path", refPng.path)
                        })
                    }
                    placeMs = SystemClock.elapsedRealtime() - tPlace
                    placeEngineMs = placeRes["elapsed_ms"] ?: JsonNull
                    matchEv = placeRes["exposure_ev"].number()
                } catch (e: TimeoutCancellationException) {
                    placeMs = SystemClock.elapsedRealtime() - tPlace
                    placeHung = true
                    placeError = "Timed out after ${placeMs}ms (likely GPU hang/deadlock)"
                    engine.kill()
                } catch (e: EngineError) {
                    placeMs = SystemClock.elapsedRealtime() - tPlace
                    placeError = e.message
                }
            }
            logEvent(buildJsonObject {
                put("event", "step")
                put("photo", label)
                put("step", "place_exposure")
                put("ref_ms", refMs)
                put("ref_usable", refRes["usable"] ?: JsonNull)
                put("place_wall_ms", placeMs)
                put("place_engine_ms", placeEngineMs)
                put("exposure_ev", matchEv?.let { JsonPrimitive(it) } ?: JsonNull)
                put("hung", placeHung)
                if (placeError != null) put("error", placeError)
                if (!placeHung) put("engine_stats", queryStats())
                put("memory", systemMemory(context))
            })
            refPng.delete()

            // Step 5: full-size export (render limit_preview:false + save JPEG)
            // Sample available system RAM and /proc/<enginePid>/status during export
            // so even if lmkd kills dev.negs.app mid-export, peak RSS and fallback reason are recorded.
            val sampling = AtomicBoolean(true)
            val minAvailMib = AtomicLong(Long.MAX_VALUE)
            val maxAppPssKib = AtomicLong(0L)
            val peakEngineHwmKib = AtomicLong(0L)
            val peakEngineSwapKib = AtomicLong(0L)
            val tExp = SystemClock.elapsedRealtime()
            val sampler = Thread {
                val am = context.getSystemService(ActivityManager::class.java)
                val mi = ActivityManager.MemoryInfo()
                val dmi = Debug.MemoryInfo()
                var lastLoggedAt = 0L
                while (sampling.get()) {
                    am.getMemoryInfo(mi)
                    Debug.getMemoryInfo(dmi)
                    val avail = mi.availMem / (1024L * 1024L)
                    minAvailMib.accumulateAndGet(avail, ::minOf)
                    maxAppPssKib.accumulateAndGet(dmi.totalPss.toLong(), ::maxOf)
                    val proc = readProcStatus(enginePid)
                    val hwm = proc?.get("vm_hwm_kib")?.number()?.toLong() ?: 0L
                    val swap = proc?.get("vm_swap_kib")?.number()?.toLong() ?: 0L
                    if (hwm > 0) peakEngineHwmKib.accumulateAndGet(hwm, ::maxOf)
                    if (swap > 0) peakEngineSwapKib.accumulateAndGet(swap, ::maxOf)
                    val now = SystemClock.elapsedRealtime()
                    if (now - lastLoggedAt >= 2000L) {
                        lastLoggedAt = now
                        Log.i(tag, buildJsonObject {
                            put("event", "export_sample")
                            put("photo", label)
                            put("elapsed_ms", now - tExp)
                            put("avail_mib", avail)
                            put("app_pss_kib", dmi.totalPss)
                            if (proc != null) put("engine_proc", proc)
                        }.toString())
                    }
                    try {
                        Thread.sleep(250)
                    } catch (_: InterruptedException) {
                        break
                    }
                }
            }.apply { start() }

            val exportParams = if (matchEv != null) {
                defaults.with("prep.exposure_ev", matchEv.json())
            } else {
                defaults
            }

            try {
                var renderMs = 0L
                var saveMs = 0L
                lateinit var expRes: JsonObject
                lateinit var saveRes: JsonObject
                withTimeout(300_000L) {
                    engine.exclusive {
                        val t0 = SystemClock.elapsedRealtime()
                        expRes = send("render", buildJsonObject {
                            put("input", photoFile.path)
                            put("source_generation", 1)
                            put("film", film)
                            put("paper", paper)
                            put("params", exportParams)
                            put("limit_preview", false)
                        })
                        renderMs = SystemClock.elapsedRealtime() - t0
                        val renderId = expRes["request_id"].number()!!.toLong()
                        val t1 = SystemClock.elapsedRealtime()
                        saveRes = send("save", buildJsonObject {
                            put("render_id", renderId)
                            put("output", exportJpg.path)
                            put("save_options", buildJsonObject {
                                put("jpeg_quality", 92)
                                put("copy_metadata", true)
                            })
                            put("crop_after_post", true)
                            put("rotate_cw", 0)
                        })
                        saveMs = SystemClock.elapsedRealtime() - t1
                    }
                }
                sampling.set(false)
                sampler.join(2000)

                val statsAfterExport = queryStats()
                logEvent(buildJsonObject {
                    put("event", "step")
                    put("photo", label)
                    put("step", "export_full")
                    put("killed", false)
                    put("render_wall_ms", renderMs)
                    put("render_engine_ms", expRes["elapsed_ms"] ?: JsonNull)
                    put("save_wall_ms", saveMs)
                    put("total_wall_ms", SystemClock.elapsedRealtime() - tExp)
                    put("backend", expRes["backend"] ?: JsonNull)
                    put("output_width", expRes["output_width"] ?: JsonNull)
                    put("output_height", expRes["output_height"] ?: JsonNull)
                    put("preview_file_bytes", exportPreview.length())
                    put("jpeg_bytes", exportJpg.length())
                    put("saved", saveRes["saved"] ?: JsonNull)
                    put("min_avail_mib_during_export", minAvailMib.get())
                    put("max_app_pss_kib_during_export", maxAppPssKib.get())
                    put("sampled_peak_engine_hwm_kib", peakEngineHwmKib.get())
                    put("sampled_peak_engine_swap_kib", peakEngineSwapKib.get())
                    put("engine_stats", statsAfterExport)
                    put("memory", systemMemory(context))
                    put("engine_starts", engine.starts.value)
                    put("stderr_notes", buildJsonArray { importantStderr.forEach { add(JsonPrimitive(it)) } })
                })
            } catch (e: TimeoutCancellationException) {
                sampling.set(false)
                sampler.join(2000)
                engine.kill()
                logEvent(buildJsonObject {
                    put("event", "step")
                    put("photo", label)
                    put("step", "export_full")
                    put("killed", false)
                    put("hung", true)
                    put("error", "Timed out after ${SystemClock.elapsedRealtime() - tExp}ms")
                    put("elapsed_before_timeout_ms", SystemClock.elapsedRealtime() - tExp)
                    put("min_avail_mib_during_export", minAvailMib.get())
                    put("max_app_pss_kib_during_export", maxAppPssKib.get())
                    put("sampled_peak_engine_hwm_kib", peakEngineHwmKib.get())
                    put("sampled_peak_engine_swap_kib", peakEngineSwapKib.get())
                    put("memory", systemMemory(context))
                    put("engine_starts", engine.starts.value)
                    put("stderr_notes", buildJsonArray { importantStderr.forEach { add(JsonPrimitive(it)) } })
                    put("stderr_tail", buildJsonArray { stderrLines.takeLast(25).forEach { add(JsonPrimitive(it)) } })
                })
            } catch (e: EngineDied) {
                sampling.set(false)
                sampler.join(2000)
                logEvent(buildJsonObject {
                    put("event", "step")
                    put("photo", label)
                    put("step", "export_full")
                    put("killed", true)
                    put("error", e.message ?: "EngineDied")
                    put("elapsed_before_death_ms", SystemClock.elapsedRealtime() - tExp)
                    put("min_avail_mib_during_export", minAvailMib.get())
                    put("max_app_pss_kib_during_export", maxAppPssKib.get())
                    put("sampled_peak_engine_hwm_kib", peakEngineHwmKib.get())
                    put("sampled_peak_engine_swap_kib", peakEngineSwapKib.get())
                    put("memory", systemMemory(context))
                    put("engine_starts", engine.starts.value)
                    put("stderr_notes", buildJsonArray { importantStderr.forEach { add(JsonPrimitive(it)) } })
                    put("stderr_tail", buildJsonArray { stderrLines.takeLast(25).forEach { add(JsonPrimitive(it)) } })
                })
            }
        } catch (e: Exception) {
            logEvent(buildJsonObject {
                put("event", "photo_error")
                put("photo", label)
                put("killed", e is EngineDied)
                put("error_type", e.javaClass.simpleName)
                put("error", e.message ?: e.toString())
                put("memory", systemMemory(context))
                put("engine_starts", engine.starts.value)
                put("stderr_notes", buildJsonArray { importantStderr.forEach { add(JsonPrimitive(it)) } })
                put("stderr_tail", buildJsonArray { stderrLines.takeLast(25).forEach { add(JsonPrimitive(it)) } })
            })
        } finally {
            engine.stop()
            workDir.deleteRecursively()
        }
    }
}
