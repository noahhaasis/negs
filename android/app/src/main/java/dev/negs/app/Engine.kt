package dev.negs.app

import android.content.Context
import android.util.Log
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.io.BufferedWriter
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/** The engine answered with an error, in words for the user. */
class EngineError(message: String) : Exception(message)

/** The engine process ended while it worked on a request (often: memory). */
class EngineDied(message: String) : Exception(message)

/**
 * The film engine, as a separate process: the desktop apps' helper
 * (crates/spektrafilm-native), built for Android and shipped as a native
 * library, spoken to over its JSON-lines protocol on stdin/stdout.
 *
 * Its own process keeps the photograph's gigabyte or so out of the UI's
 * heap, and when Android ends it for memory the editor stays up: the next
 * request starts it again, and every request names the photograph, which it
 * loads again by itself.
 */
class Engine(
    private val context: Context,
    private val dataDir: File,
    private val policy: MemoryPolicy,
    private val onStderr: ((String) -> Unit)? = null,
) {
    private var process: Process? = null
    private var stdin: BufferedWriter? = null
    private val pending = ConcurrentHashMap<Long, CompletableDeferred<JsonObject>>()
    private val ids = AtomicLong(1)

    /** One request at a time, as the engine works: the editor's go first. */
    private val gate = Mutex()
    private val urgentWaiting = AtomicInteger(0)

    private val _starts = MutableStateFlow(0)
    /** How often the engine process started; above 1 it was restarted. */
    val starts: StateFlow<Int> = _starts

    private val _lastExit = MutableStateFlow<String?>(null)
    /** Why the last engine process ended unasked, for the status line. */
    val lastExit: StateFlow<String?> = _lastExit

    /**
     * Sends `command` with `fields` and returns its result. `urgent` requests
     * (the photograph's renders, export) go before waiting background work
     * (stock previews).
     */
    suspend fun call(command: String, fields: JsonObject = JsonObject(emptyMap()), urgent: Boolean = true): JsonObject =
        exclusive(urgent) { send(command, fields) }

    /**
     * Runs `block` with the engine to itself: requests that must follow each
     * other with nothing between (an export's render and save).
     */
    suspend fun <T> exclusive(urgent: Boolean = true, block: suspend Session.() -> T): T {
        if (urgent) urgentWaiting.incrementAndGet()
        try {
            if (!urgent) while (urgentWaiting.get() > 0) delay(8)
            return gate.withLock { Session().block() }
        } finally {
            if (urgent) urgentWaiting.decrementAndGet()
        }
    }

    inner class Session {
        suspend fun send(command: String, fields: JsonObject = JsonObject(emptyMap())): JsonObject =
            this@Engine.send(command, fields)
    }

    private suspend fun send(command: String, fields: JsonObject): JsonObject {
        val id = ids.getAndIncrement()
        val answer = CompletableDeferred<JsonObject>()
        pending[id] = answer
        val line = buildJsonObject {
            put("id", JsonPrimitive(id))
            put("command", JsonPrimitive(command))
            fields.forEach { (k, v) -> put(k, v) }
        }.toString()
        try {
            withContext(Dispatchers.IO) {
                val writer = ensureStarted()
                writer.write(line)
                writer.newLine()
                writer.flush()
            }
        } catch (e: Exception) {
            pending.remove(id)
            throw EngineDied("The engine couldn't be reached: ${e.message}")
        }
        return answer.await()
    }

    @Synchronized
    private fun ensureStarted(): BufferedWriter {
        stdin?.let { if (process?.isAlive == true) return it }
        val exe = File(context.applicationInfo.nativeLibraryDir, "libspektrafilm_native.so")
        val cache = File(context.cacheDir, "engine").apply { mkdirs() }
        val builder = ProcessBuilder(exe.path, "--data-dir", dataDir.path)
        builder.environment().apply {
            putAll(policy.environment())
            // The Vulkan pipeline cache (spektrafilm-gpu pipeline_cache.rs).
            put("XDG_CACHE_HOME", cache.path)
            put("HOME", context.filesDir.path)
            put("RUST_LOG", "info")
        }
        val started = builder.start()
        process = started
        val writer = started.outputStream.bufferedWriter()
        stdin = writer
        _starts.value += 1
        Log.i(TAG, "engine started (${_starts.value}): ${policy.environment()}")
        Thread({ readAnswers(started) }, "engine-stdout").start()
        Thread({
            runCatching {
                started.errorStream.bufferedReader().forEachLine { line ->
                    Log.i(TAG, line)
                    onStderr?.invoke(line)
                }
            }
        }, "engine-stderr").start()
        return writer
    }

    private fun readAnswers(proc: Process) {
        try {
            proc.inputStream.bufferedReader().forEachLine { line ->
                val answer = runCatching { Json.parseToJsonElement(line).jsonObject }.getOrNull() ?: return@forEachLine
                val id = answer["id"]?.jsonPrimitive?.longOrNull ?: return@forEachLine
                val waiting = pending.remove(id) ?: return@forEachLine
                val error = answer["error"]
                if (error != null) waiting.completeExceptionally(EngineError(error.jsonPrimitive.content))
                // The request's id goes with its result: Save names a render by it.
                else waiting.complete(JsonObject((answer["result"]?.jsonObject ?: emptyMap()) + ("request_id" to JsonPrimitive(id))))
            }
        } catch (_: Exception) {
        }
        val code = runCatching { proc.waitFor() }.getOrDefault(-1)
        synchronized(this) {
            if (process === proc) {
                process = null
                stdin = null
            }
        }
        // 137 and 9: killed, which on a phone means memory.
        val reason = if (code == 137 || code == 9) "Android stopped the engine to free memory" else "The engine stopped (exit $code)"
        if (pending.isNotEmpty()) {
            _lastExit.value = reason
            Log.w(TAG, reason)
        }
        val died = EngineDied(reason)
        pending.keys.toList().forEach { id -> pending.remove(id)?.completeExceptionally(died) }
    }

    /**
     * Ends the engine, which frees everything it holds; the next request
     * starts it again. For when the app is out of sight for a while.
     */
    @Synchronized
    fun stop() {
        runCatching { stdin?.close() }
        stdin = null
        process = null
    }

    /** Force-kills a hung engine process (used by benchmark timeouts). */
    @Synchronized
    fun kill() {
        runCatching { stdin?.close() }
        runCatching { process?.destroyForcibly() }
        stdin = null
        process = null
    }

    companion object {
        const val TAG = "NegsEngine"
    }
}
