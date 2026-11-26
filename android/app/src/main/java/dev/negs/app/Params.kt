package dev.negs.app

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlin.math.abs
import kotlin.math.max

/**
 * RuntimeParams as the engine's JSON, edited by dotted paths as the web app
 * does (`film_render.grain.active`, `scanner.unsharp_mask.1`): numbers after
 * a dot index arrays.
 */
fun JsonElement?.at(path: String): JsonElement? =
    path.split('.').fold(this) { node, key ->
        when (node) {
            is JsonObject -> node[key]
            is JsonArray -> key.toIntOrNull()?.let { node.getOrNull(it) }
            else -> null
        }
    }?.takeUnless { it is JsonNull }

fun JsonObject.with(path: String, value: JsonElement): JsonObject = set(this, path.split('.'), value) as JsonObject

private fun set(node: JsonElement?, keys: List<String>, value: JsonElement): JsonElement {
    if (keys.isEmpty()) return value
    val key = keys.first()
    val rest = keys.drop(1)
    return when {
        node is JsonArray && key.toIntOrNull() != null -> {
            val index = key.toInt()
            JsonArray(node.mapIndexed { i, child -> if (i == index) set(child, rest, value) else child })
        }
        else -> {
            val map = (node as? JsonObject)?.toMutableMap() ?: mutableMapOf()
            map[key] = set(map[key], rest, value)
            JsonObject(map)
        }
    }
}

fun JsonElement?.number(): Double? = (this as? JsonPrimitive)?.takeUnless { it.isString }?.doubleOrNull
fun JsonElement?.bool(): Boolean? = (this as? JsonPrimitive)?.booleanOrNull
fun JsonElement?.text(): String? = (this as? JsonPrimitive)?.takeIf { it.isString }?.content

fun Number.json() = JsonPrimitive(this)
fun Boolean.json() = JsonPrimitive(this)
fun String.json() = JsonPrimitive(this)

/** Rust defaults arrive as widened f32 values; slider edits are decimal. */
fun near(a: Double, b: Double) = abs(a - b) <= 1e-6 * max(1.0, abs(b))

fun sameValue(a: JsonElement?, b: JsonElement?): Boolean {
    val x = a.number()
    val y = b.number()
    if (x != null && y != null) return near(x, y)
    return (a ?: JsonNull) == (b ?: JsonNull)
}

fun JsonElement?.numbers(): List<Double> = (this as? JsonArray)?.mapNotNull { it.number() } ?: emptyList()

fun List<Double>.json() = JsonArray(map { JsonPrimitive(it) })

fun JsonElement.primitiveText(): String = (this as? JsonPrimitive)?.content ?: toString()

@Suppress("unused")
private fun JsonElement.debug() = jsonPrimitive.content
