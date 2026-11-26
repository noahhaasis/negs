package dev.negs.app

import android.content.Context
import android.content.res.AssetManager
import java.io.File

/**
 * The engine's data directory (stock profiles, LUTs, borders…), unpacked
 * from the APK's assets once per install or update: the engine reads files.
 */
object EngineData {
    /** Unpacks the data if this build's isn't there yet; returns its directory. */
    fun prepare(context: Context): File {
        val dir = File(context.filesDir, "data")
        val stamp = File(context.filesDir, "data.stamp")
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        val wanted = "${info.longVersionCode}:${info.lastUpdateTime}"
        if (dir.isDirectory && stamp.exists() && stamp.readText() == wanted) return dir
        // A half-unpacked directory from an interrupted first launch goes too.
        val staging = File(context.filesDir, "data.unpacking")
        staging.deleteRecursively()
        copy(context.assets, "data", staging)
        dir.deleteRecursively()
        check(staging.renameTo(dir)) { "Couldn't move the engine's data into place" }
        stamp.writeText(wanted)
        return dir
    }

    private fun copy(assets: AssetManager, path: String, to: File) {
        val children = assets.list(path).orEmpty()
        if (children.isEmpty()) {
            to.parentFile?.mkdirs()
            assets.open(path).use { input -> to.outputStream().use { input.copyTo(it, 1 shl 16) } }
            return
        }
        to.mkdirs()
        for (child in children) copy(assets, "$path/$child", File(to, child))
    }
}
