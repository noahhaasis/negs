package dev.negs.app

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject

/** A film or paper, as the engine's `catalog` lists it. */
data class Stock(
    val id: String,
    val name: String,
    val paper: Boolean,
    val bw: Boolean,
    val positive: Boolean,
    val cinePrint: Boolean,
    val targetPrint: String?,
    val fotufilm: Boolean,
    val manufacturerData: Boolean,
    val negsBuilt: Boolean,
    /** The film's grain presets, by field (`rms_granularity`…), per layer. */
    val grain: Map<String, List<Double>>,
    val json: JsonObject,
)

data class ColorFilterGlass(val key: String, val name: String, val glass: String, val colour: Long)

/**
 * The engine's catalog: the stocks, the default params every edit starts
 * from, and the glasses for black-and-white film.
 */
class Catalog(
    val stocks: List<Stock>,
    val defaults: JsonObject,
    val colorFilters: List<ColorFilterGlass>,
    /** What a preset leaves as it is (presets::KEPT): a preset's tile changes only with these. */
    val presetKept: List<String> = emptyList(),
) {
    fun stock(id: String) = stocks.firstOrNull { it.id == id }
    val films get() = stocks.filter { !it.paper }
    val papers get() = stocks.filter { it.paper }

    private fun isMotionPicturePrint(p: Stock) =
        p.paper && (p.cinePrint || "Vision" in p.name || "Print Film" in p.name)

    /**
     * Films as web/app.js `filmGroups` groups them, without those Settings
     * hides (web `listsFilm`): Fotufilm ports by [fotufilm] ("all",
     * "datasheets" or "off"), Negs-built films unless [negsBuilt]. The film
     * in use stays listed.
     */
    fun filmGroups(current: String, fotufilm: String, negsBuilt: Boolean): List<Pair<String, List<Stock>>> {
        val cinemaPrints = papers.filter(::isMotionPicturePrint).map { it.id }.toSet()
        fun lists(s: Stock) = (!s.fotufilm || fotufilm == "all" || (fotufilm == "datasheets" && s.manufacturerData)) &&
            (negsBuilt || !s.negsBuilt)
        val listed = films.filter { lists(it) || it.id == current }
        val kind = { s: Stock ->
            when {
                s.bw -> "Black & White"
                s.positive -> "Color Slide"
                s.targetPrint in cinemaPrints -> "Motion Picture"
                else -> "Color Negative"
            }
        }
        return listOf("Color Negative", "Color Slide", "Motion Picture", "Black & White")
            .map { title -> title to listed.filter { kind(it) == title } }
            .filter { it.second.isNotEmpty() }
    }

    /** Papers as web/app.js `paperGroups`: Negs-built print films only if [negsBuilt]. */
    fun paperGroups(current: String, negsBuilt: Boolean): List<Pair<String, List<Stock>>> {
        val listed = papers.filter { negsBuilt || !it.negsBuilt || it.id == current }
        val kind = { s: Stock ->
            when {
                s.bw -> "Black & White"
                isMotionPicturePrint(s) -> "Motion Picture Print"
                else -> "Color Paper"
            }
        }
        return listOf("Color Paper", "Motion Picture Print", "Black & White")
            .map { title -> title to listed.filter { kind(it) == title } }
            .filter { it.second.isNotEmpty() }
    }

    companion object {
        fun parse(result: JsonObject): Catalog {
            val stocks = result["profiles"]!!.jsonArray.map { element ->
                val p = element.jsonObject
                Stock(
                    id = p["id"].text()!!,
                    name = p["name"].text() ?: p["id"].text()!!,
                    paper = p["paper"].bool() == true,
                    bw = p["bw"].bool() == true,
                    positive = p["positive"].bool() == true,
                    cinePrint = p["cine_print"].bool() == true,
                    targetPrint = p["target_print"].text(),
                    fotufilm = p["fotufilm"].bool() == true,
                    manufacturerData = p["manufacturer_data"].bool() != false,
                    negsBuilt = p["negs_built"].bool() == true,
                    grain = (p["grain"] as? JsonObject)?.mapValues { it.value.numbers() } ?: emptyMap(),
                    json = p,
                )
            }
            val filters = (result["color_filters"] as? JsonArray).orEmpty().map { element ->
                val f = element.jsonObject
                ColorFilterGlass(
                    key = f["key"].text()!!,
                    name = f["name"].text() ?: "",
                    glass = f["glass"].text() ?: "",
                    colour = 0xFF000000 or (f["colour"].text()?.removePrefix("#")?.toLongOrNull(16) ?: 0x808080),
                )
            }
            val kept = (result["preset_kept"] as? JsonArray).orEmpty().mapNotNull { it.text() }
            return Catalog(stocks, result["defaults"]!!.jsonObject, filters, kept)
        }
    }
}

@Suppress("unused")
private fun JsonElement.asObject() = jsonObject
