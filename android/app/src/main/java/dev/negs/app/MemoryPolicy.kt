package dev.negs.app

import android.app.ActivityManager
import android.content.Context
import android.util.DisplayMetrics
import kotlin.math.max
import kotlin.math.min

/**
 * How much the engine may use, from this device's measured memory rather
 * than a guess from its kind (the web app's `pointer: coarse`).
 *
 * - The GPU's admission budget: past it a render goes to the CPU engine
 *   instead of running the GPU out of memory. Phone GPUs share system RAM
 *   and Vulkan reports no separate heap, so it's a share of the RAM.
 * - The diffusion filters' FFT grid: on small phones the glow is made from a
 *   smaller copy of the frame, as the web app does on every phone. Larger
 *   phones render it exactly, as the desktop apps do.
 * - Bounded preview decode (`maxPreviewSourcePixels`): keeps a screen-sized
 *   working copy (~4.4 MP, ~50 MiB) while editing large RAWs so 60 MP photos
 *   don't hold a 690 MiB full `f32` frame in RAM just for previews.
 * - GPU RGBA8 preview readback (`SPEKTRAFILM_PREVIEW_RGBA8=1`): reads back
 *   4 B/px RGBA8 directly instead of 12 B/px `f32` + CPU tone-pack.
 */
data class MemoryPolicy(
    val totalBytes: Long,
    val lowRam: Boolean,
    val gpuBudgetMiB: Long,
    val diffusionGridBudgetBytes: Long,
    val previewLongSide: Int,
    val initialViewportLongSide: Int,
    val maxPreviewSourcePixels: Int,
) {
    /** The engine process's environment for this policy. */
    fun environment(): Map<String, String> = buildMap {
        put("SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB", gpuBudgetMiB.toString())
        if (diffusionGridBudgetBytes > 0) put("SPEKTRAFILM_DIFFUSION_GRID_BUDGET", diffusionGridBudgetBytes.toString())
        put("SPEKTRAFILM_MAX_PREVIEW_SOURCE_PIXELS", maxPreviewSourcePixels.toString())
        put("SPEKTRAFILM_PREVIEW_RGBA8", "1")
    }

    /** What's free now, for the status line and logs. */
    fun available(context: Context): Long {
        val info = ActivityManager.MemoryInfo()
        context.getSystemService(ActivityManager::class.java).getMemoryInfo(info)
        return info.availMem
    }

    companion object {
        private const val MIB = 1L shl 20
        private const val GIB = 1L shl 30
        /** Phone working-copy ceiling (matches `web/app.js` `MAX_PIXELS = 4.4e6`). */
        const val MAX_PREVIEW_PIXELS = 4_400_000

        fun measure(context: Context): MemoryPolicy {
            val manager = context.getSystemService(ActivityManager::class.java)
            val info = ActivityManager.MemoryInfo()
            manager.getMemoryInfo(info)
            val total = info.totalMem
            val metrics: DisplayMetrics = context.resources.displayMetrics
            val screenLong = max(metrics.widthPixels, metrics.heightPixels)
            val screenShort = min(metrics.widthPixels, metrics.heightPixels)
            return MemoryPolicy(
                totalBytes = total,
                lowRam = manager.isLowRamDevice,
                // Two fifths of the RAM: the photograph's working frames are
                // on the GPU, its source and the UI are not.
                gpuBudgetMiB = total * 2 / 5 / MIB,
                diffusionGridBudgetBytes = when {
                    total < 6 * GIB -> 160 * MIB
                    total < 10 * GIB -> 512 * MIB
                    else -> 0
                },
                // Maximum zoomed settled preview long side. Budget 4 GB phones
                // (e.g. Adreno 610) stay within 1600 px (~1.7 MP) so zoomed
                // frames stay responsive and safely under KGSL's ~2 s watchdog.
                previewLongSide = if (total < 5 * GIB) {
                    screenLong.coerceIn(1024, 1600)
                } else {
                    screenLong.coerceIn(1024, 2560)
                },
                // Fallback fitted viewport width before Compose reports the measured canvas size.
                initialViewportLongSide = screenShort.coerceIn(720, 1440),
                maxPreviewSourcePixels = if (total < 5 * GIB) 2_200_000 else MAX_PREVIEW_PIXELS,
            )
        }
    }
}
