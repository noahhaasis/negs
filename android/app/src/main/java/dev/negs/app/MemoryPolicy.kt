package dev.negs.app

import android.app.ActivityManager
import android.content.Context
import android.util.DisplayMetrics
import kotlin.math.max

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
 * - Preview sizes: the screen's, never the photograph's 24 or 40 MP.
 */
data class MemoryPolicy(
    val totalBytes: Long,
    val lowRam: Boolean,
    val gpuBudgetMiB: Long,
    val diffusionGridBudgetBytes: Long,
    val previewLongSide: Int,
) {
    /** The engine process's environment for this policy. */
    fun environment(): Map<String, String> = buildMap {
        put("SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB", gpuBudgetMiB.toString())
        if (diffusionGridBudgetBytes > 0) put("SPEKTRAFILM_DIFFUSION_GRID_BUDGET", diffusionGridBudgetBytes.toString())
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

        fun measure(context: Context): MemoryPolicy {
            val manager = context.getSystemService(ActivityManager::class.java)
            val info = ActivityManager.MemoryInfo()
            manager.getMemoryInfo(info)
            val total = info.totalMem
            val metrics: DisplayMetrics = context.resources.displayMetrics
            val screen = max(metrics.widthPixels, metrics.heightPixels)
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
                // The settled preview fills the screen's longer side; zoom
                // shows the full photograph's pixels only past that.
                previewLongSide = screen.coerceIn(1024, 3072),
            )
        }
    }
}
