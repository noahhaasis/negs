import Foundation

/// The parts of a photograph's edits that Copy Edits… offers, and how a
/// parameter path falls into them.
enum EditGroup: String, CaseIterable, Codable, Sendable {
    case filmAndPaper, prep, lab, border, output, cropAndRotation, file

    var title: String {
        switch self {
        case .filmAndPaper: "Film and paper"
        case .prep: "Prep (white balance, exposure, Match camera brightness)"
        case .lab: "Lab"
        case .border: "Border"
        case .output: "Image size and output"
        case .cropAndRotation: "Crop and rotation"
        case .file: "Lens correction and input interpretation"
        }
    }

    /// What a plain Copy Edits copies: crop and rotation, and how the file
    /// was read, belong to each photograph.
    static let standard: Set<EditGroup> = [.filmAndPaper, .prep, .lab, .border, .output]

    private static let io: [String: EditGroup] = [
        "scan_film": .filmAndPaper,
        "raw_white_balance": .prep,
        "output_gamut_compress": .lab, "input_gamut_compress": .lab,
        "photo_long_side_px": .output, "upscale_factor": .output, "output_color_space": .output, "output_cctf_encoding": .output,
        "rotate_cw": .cropAndRotation, "crop": .cropAndRotation, "crop_center": .cropAndRotation, "crop_size": .cropAndRotation,
    ]

    /// The group of a parameter: `io` keys one by one (any not listed is
    /// the file's own), `prep` is Prep, and every other section is Lab.
    static func of(section: String, key: String) -> EditGroup {
        switch section {
        case "io": io[key] ?? .file
        case "prep": .prep
        default: .lab
        }
    }

    /// `target` with `groups` taken from `source`. Rating and flag aren't
    /// edits and stay.
    static func merge(_ groups: Set<EditGroup>, from source: EditState, into target: EditState) -> EditState {
        var merged = target
        if groups.contains(.filmAndPaper) { merged.film = source.film; merged.paper = source.paper }
        if groups.contains(.border) { merged.border = source.border }
        if groups.contains(.cropAndRotation) { merged.crop = source.crop }
        // Match camera brightness on or off goes with Prep; the match itself
        // is solved again for the photograph it lands on.
        if groups.contains(.prep) { merged.brightnessMatch = source.brightnessMatch.map { _ in BrightnessMatch() } }
        guard case .object(let sections) = source.params else { return merged }
        for (section, values) in sections {
            if case .object(let keys) = values, section == "io" || section == "prep" {
                for (key, value) in keys where groups.contains(of(section: section, key: key)) {
                    merged.params["\(section).\(key)"] = value
                }
            } else if groups.contains(of(section: section, key: "")) {
                merged.params[section] = values
            }
        }
        return merged
    }
}
