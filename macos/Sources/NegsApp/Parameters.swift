import Foundation

/// Keep the full Rust schema, including fields the inspector doesn't expose.
/// Defaults come from the engine, so adding a Rust parameter cannot reset it here.
enum JSONValue: Codable, Equatable, Sendable {
    case object([String: JSONValue]), array([JSONValue]), string(String), number(Double), bool(Bool), null

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([String: JSONValue].self) { self = .object(v) }
        else { self = .array(try c.decode([JSONValue].self)) }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .object(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .null: try c.encodeNil()
        }
    }

    subscript(_ path: String) -> JSONValue {
        get { value(at: path.split(separator: ".").map(String.init)[...]) }
        set { set(newValue, at: path.split(separator: ".").map(String.init)[...]) }
    }

    private func value(at path: ArraySlice<String>) -> JSONValue {
        guard let key = path.first else { return self }
        switch self {
        case .object(let values): return values[key]?.value(at: path.dropFirst()) ?? .null
        case .array(let values):
            guard let i = Int(key), values.indices.contains(i) else { return .null }
            return values[i].value(at: path.dropFirst())
        default: return .null
        }
    }

    private mutating func set(_ value: JSONValue, at path: ArraySlice<String>) {
        guard let key = path.first else { self = value; return }
        switch self {
        case .object(var values):
            var child = values[key] ?? .object([:])
            child.set(value, at: path.dropFirst()); values[key] = child; self = .object(values)
        case .array(var values):
            guard let i = Int(key), values.indices.contains(i) else { return }
            values[i].set(value, at: path.dropFirst()); self = .array(values)
        default: break
        }
    }

    var double: Double { if case .number(let v) = self { return v }; return 0 }
    var bool: Bool { if case .bool(let v) = self { return v }; return false }
    var string: String { if case .string(let v) = self { return v }; return "" }
    var array: [JSONValue] { if case .array(let v) = self { return v }; return [] }

    /// Keep new engine defaults, restore known fields, and discard removed fields.
    /// A saved `null` stays where `nullable` (the catalog's `nullable_paths`,
    /// `[*]` for every element) says it is a value, such as an infinite
    /// Langmuir knee, rather than falling back to the default.
    func restoring(_ saved: JSONValue, nullable: Set<String> = [], at path: String = "") -> JSONValue {
        switch (self, saved) {
        case (_, .null) where nullable.contains(path): return saved
        case (.object(let defaults), .object(let values)):
            var result = defaults
            for (key, value) in values {
                guard let fallback = defaults[key] else { continue }
                result[key] = fallback.restoring(value, nullable: nullable, at: path.isEmpty ? key : "\(path).\(key)")
            }
            return .object(result)
        case (.array(let defaults), .array(let values)) where defaults.count == values.count:
            return .array(zip(defaults, values).map { $0.restoring($1, nullable: nullable, at: "\(path)[*]") })
        case (.number, .number), (.bool, .bool), (.string, .string), (.null, _): return saved
        default: return self
        }
    }
}

struct Stock: Identifiable, Sendable {
    let id: String
    let name: String
    let paper: Bool
    let positive: Bool
    let bw: Bool
    let target: String
    let times: [Double]
    /// The resolved grain preset, per field of `Stock.grainFields`.
    let grain: JSONValue
    static let grainFields = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"]
    /// The DIR gammas and halation baseline the film's presets fill while the
    /// edit leaves them unset (`null`).
    static let dirHalationPaths = [
        "film_render.dir_couplers.gamma_samelayer_rgb", "film_render.dir_couplers.gamma_interlayer_r_to_gb",
        "film_render.dir_couplers.gamma_interlayer_g_to_rb", "film_render.dir_couplers.gamma_interlayer_b_to_rg",
        "film_render.halation.halation_strength", "film_render.halation.halation_first_sigma_um"
    ]
    /// Every value a film's presets fill. Choosing a film resets them, as
    /// upstream re-applies its presets on a stock change.
    static let presetPaths = grainFields.map { "film_render.grain.\($0)" } + dirHalationPaths

    init(_ value: JSONValue) {
        id = value["id"].string; name = value["name"].string
        paper = value["paper"].bool; positive = value["positive"].bool; bw = value["bw"].bool
        target = value["target_print"].string
        times = value["development_times"].array.map(\.double)
        grain = value["grain"]
    }
}

/// Stocks of one kind, in catalog order, for sectioned stock menus.
struct StockGroup: Identifiable {
    let title: String
    let stocks: [Stock]
    var id: String { title }

    static func films(_ films: [Stock], papers: [Stock]) -> [StockGroup] {
        let cinemaPrints = Set(papers.filter(\.isMotionPicturePrint).map(\.id))
        return group(films, order: ["Color Negative", "Color Slide", "Motion Picture", "Black & White"]) {
            $0.bw ? "Black & White" : $0.positive ? "Color Slide" : cinemaPrints.contains($0.target) ? "Motion Picture" : "Color Negative"
        }
    }

    static func papers(_ papers: [Stock]) -> [StockGroup] {
        group(papers, order: ["Color Paper", "Motion Picture Print", "Black & White"]) {
            $0.bw ? "Black & White" : $0.isMotionPicturePrint ? "Motion Picture Print" : "Color Paper"
        }
    }

    private static func group(_ stocks: [Stock], order: [String], kind: (Stock) -> String) -> [StockGroup] {
        let grouped = Dictionary(grouping: stocks, by: kind)
        return order.compactMap { title in grouped[title].map { StockGroup(title: title, stocks: $0) } }
    }
}

extension Stock {
    var isMotionPicturePrint: Bool { paper && (name.contains("Vision") || name.contains("Print Film")) }
}

/// Human-readable names for engine option identifiers.
func optionTitle(_ option: String) -> String {
    let titles = [
        "average": "Average", "median": "Median", "center_weighted": "Center-Weighted", "partial": "Partial",
        "matrix": "Matrix", "multi_zone": "Multi-Zone", "highlight_weighted": "Highlight-Weighted",
        "as_shot": "As Shot", "daylight": "Daylight", "tungsten": "Tungsten", "custom": "Custom",
        "black_pro_mist": "Black Pro-Mist", "glimmerglass": "Glimmerglass", "pro_mist": "Pro-Mist", "cinebloom": "CineBloom",
        "off": "Off", "oklch": "OKLCh", "oklrab": "OKLrab", "cam16ucs": "CAM16-UCS", "aces_rgc": "ACES RGC", "xy": "xy Chromaticity",
        "hanatos2025": "Hanatos 2025", "arctic2026beta04": "Arctic 2026 (Beta 04)", "mallett2019": "Mallett 2019"
    ]
    return titles[option] ?? option.replacingOccurrences(of: "_", with: " ").capitalized
}

struct EditState: Codable, Equatable, Sendable {
    var params: JSONValue = .object([:])
    var film = "kodak_gold_200"
    var paper = "kodak_portra_endura"
    var limitPreview = false
    /// Drawn around the developed photograph; not part of the render.
    var border = BorderSettings()
    /// Trims the developed photograph; not part of the render.
    var crop = CropSettings()

    /// The state without presentation-only settings, to decide whether an
    /// edit needs a new render. The preview is never rotated: the canvas
    /// turns it, and saving and exporting rotate the result. With an Image
    /// Size target the crop's extent sets the working size, so it stays; its
    /// position, angle and ratio never change the render.
    var rendered: EditState {
        var state = self; state.border = BorderSettings(); state.crop = CropSettings()
        if ImageSize.mode(of: params) == .longestSide, crop.cropped {
            state.crop.rect = CGRect(origin: .zero, size: crop.rect.size)
        }
        state.params["io.rotate_cw"] = .null
        return state
    }
}
