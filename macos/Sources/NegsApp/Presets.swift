import Darwin
import Foundation

/// A preset: a file in the app's Presets folder holding Python spektrafilm's
/// saved GUI state, named after the file (docs/presets-design.md). The
/// engine reads and writes the format; this only handles the files.
struct PresetFile: Identifiable, Equatable, Hashable, Sendable {
    /// As found on disk, which may be decomposed Unicode.
    let url: URL
    let size: Int
    let modified: Date
    var name: String { url.deletingPathExtension().lastPathComponent }
    var id: URL { url }
}

/// One naming policy on every platform: names Windows, macOS and Linux all
/// accept as file names, compared without regard to case or Unicode form.
enum PresetName {
    static let maxLength = 120

    /// Why `name` can't name a preset, or nil.
    static func problem(_ name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return "Enter a name." }
        if name.count > maxLength { return "Use at most \(maxLength) characters." }
        if name.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) || name.contains(where: { "/\\:*?\"<>|".contains($0) }) {
            return "A name can’t contain / \\ : * ? \" < > or |."
        }
        if name.hasPrefix(".") { return "A name can’t start with a period." }
        if name.hasSuffix(".") || name.hasSuffix(" ") || name.hasPrefix(" ") { return "A name can’t start with a space or end with a space or period." }
        let stem = name.split(separator: ".", maxSplits: 1).first.map { String($0).uppercased() } ?? ""
        let reserved = ["CON", "PRN", "AUX", "NUL"] + (1...9).flatMap { ["COM\($0)", "LPT\($0)"] }
        if reserved.contains(stem.trimmingCharacters(in: .whitespaces)) { return "“\(name)” is reserved on Windows." }
        return nil
    }

    /// Names that are the same preset: Unicode-normalized and case-folded.
    static func key(_ name: String) -> String {
        name.precomposedStringWithCanonicalMapping.folding(options: [.caseInsensitive], locale: nil)
    }

    /// `name`, or "name 2", "name 3"… the first not in `taken` (keys).
    static func unique(_ name: String, taken: Set<String>) -> String {
        if !taken.contains(key(name)) { return name }
        var n = 2
        while taken.contains(key("\(name) \(n)")) { n += 1 }
        return "\(name) \(n)"
    }

    /// A file's name as a preset name: invalid characters become spaces.
    static func from(fileName: String) -> String {
        var name = String(fileName.map { "/\\:*?\"<>|".contains($0) || $0.isNewline ? " " : $0 })
        name = name.trimmingCharacters(in: CharacterSet(charactersIn: " .")).replacingOccurrences(of: "  ", with: " ")
        if name.count > maxLength { name = String(name.prefix(maxLength)).trimmingCharacters(in: CharacterSet(charactersIn: " .")) }
        return problem(name) == nil ? name : "Preset"
    }
}

/// The Presets folder. File I/O runs on this actor, away from the main
/// actor; writes never replace an existing file, and renames and deletes
/// act only on the file as it was listed.
///
/// The user's order is `.order.json` in the folder: `{"order": [file names]}`.
/// Presets it doesn't name, such as files copied in by hand, follow in name
/// order; added presets go to the end.
actor PresetLibrary {
    let folder: URL
    var orderFile: URL { folder.appendingPathComponent(".order.json") }
    /// Larger files aren't GUI states.
    static let maxFileSize = 1_048_576

    init(folder: URL = AppDataLocations.supportDirectory.appendingPathComponent("Presets", isDirectory: true)) {
        self.folder = folder
    }

    func list() throws -> [PresetFile] {
        let fm = FileManager.default
        guard fm.fileExists(atPath: folder.path) else { return [] }
        let files = try fm.contentsOfDirectory(atPath: folder.path)
            .filter { !$0.hasPrefix(".") && ($0 as NSString).pathExtension.lowercased() == "json" }
            // In the folder's own spelling (not /private/var for /var), so a
            // file added or renamed here is the same URL when listed.
            .compactMap { try? file(at: folder.appendingPathComponent($0)) }
            .sorted { PresetName.key($0.name) < PresetName.key($1.name) }
        let position = Dictionary(readOrder().enumerated().map { (PresetName.key($1), $0) }) { first, _ in first }
        // Stable: unordered files keep their name order after the ordered ones.
        return files.enumerated().sorted { a, b in
            let (x, y) = (position[PresetName.key(a.element.name)] ?? Int.max, position[PresetName.key(b.element.name)] ?? Int.max)
            return x != y ? x < y : a.offset < b.offset
        }.map(\.element)
    }

    /// Remembers this order of the presets.
    func setOrder(_ presets: [PresetFile]) throws {
        try writeOrder(presets.map(\.name))
    }

    /// Writes `data` as a new preset named `name`, or "name 2"… if taken.
    func add(_ data: Data, name: String) throws -> PresetFile {
        if let problem = PresetName.problem(name) { throw EngineError(message: problem) }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let temporary = folder.appendingPathComponent(".\(UUID().uuidString).tmp")
        try data.write(to: temporary, options: .withoutOverwriting)
        defer { unlink(temporary.path) }
        var taken = Set(try list().map { PresetName.key($0.name) })
        for _ in 0..<1000 {
            let candidate = PresetName.unique(name, taken: taken)
            let url = folder.appendingPathComponent(candidate).appendingPathExtension("json")
            // A hard link never replaces: a file created meanwhile fails it.
            if link(temporary.path, url.path) == 0 {
                try writeOrder(try list().map(\.name).filter { PresetName.key($0) != PresetName.key(candidate) } + [candidate])
                return try file(at: url)
            }
            guard errno == EEXIST else { throw posixError("Couldn’t save the preset") }
            taken.insert(PresetName.key(candidate))
        }
        throw EngineError(message: "Couldn’t find a free name for the preset.")
    }

    func rename(_ preset: PresetFile, to name: String) throws -> PresetFile {
        if let problem = PresetName.problem(name) { throw EngineError(message: problem) }
        try verify(preset)
        let url = folder.appendingPathComponent(name).appendingPathExtension("json")
        if PresetName.key(name) == PresetName.key(preset.name) {
            // The same name in another case or form: the same file.
            guard Darwin.rename(preset.url.path, url.path) == 0 else { throw posixError("Couldn’t rename the preset") }
        } else {
            if try list().contains(where: { PresetName.key($0.name) == PresetName.key(name) }) {
                throw EngineError(message: "A preset named “\(name)” already exists.")
            }
            guard renamex_np(preset.url.path, url.path, UInt32(RENAME_EXCL)) == 0 else {
                if errno == EEXIST { throw EngineError(message: "A preset named “\(name)” already exists.") }
                throw posixError("Couldn’t rename the preset")
            }
        }
        let order = readOrder()
        if let index = order.firstIndex(where: { PresetName.key($0) == PresetName.key(preset.name) }) {
            var renamed = order; renamed[index] = name
            try writeOrder(renamed)
        }
        return try file(at: url)
    }

    func delete(_ preset: PresetFile) throws {
        try verify(preset)
        try FileManager.default.removeItem(at: preset.url)
        try writeOrder(readOrder().filter { PresetName.key($0) != PresetName.key(preset.name) })
    }

    /// Preset names in the user's order; none if the file is missing or unreadable.
    private func readOrder() -> [String] {
        guard let data = try? Data(contentsOf: orderFile),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let files = object["order"] as? [String] else { return [] }
        return files.map { ($0 as NSString).deletingPathExtension }
    }

    private func writeOrder(_ names: [String]) throws {
        let data = try JSONSerialization.data(withJSONObject: ["order": names.map { "\($0).json" }], options: [.prettyPrinted])
        try data.write(to: orderFile, options: .atomic)
    }

    func read(_ preset: PresetFile) throws -> Data {
        try verify(preset)
        return try Self.read(preset.url)
    }

    /// A file's bytes, within the size limit.
    static func read(_ url: URL) throws -> Data {
        let size = (try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int) ?? 0
        guard size <= maxFileSize else { throw EngineError(message: "The file is too large to be a preset.") }
        return try Data(contentsOf: url)
    }

    /// The file is still the one listed: not replaced, edited or removed since.
    private func verify(_ preset: PresetFile) throws {
        guard let current = try? file(at: preset.url), current.size == preset.size, current.modified == preset.modified else {
            throw EngineError(message: "“\(preset.name)” changed on disk. The list has been updated; try again.")
        }
    }

    /// A regular file's size and modification date, read now: not
    /// `URL.resourceValues`, which caches them per URL instance.
    private func file(at url: URL) throws -> PresetFile {
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular else { throw EngineError(message: "“\(url.lastPathComponent)” isn’t a file.") }
        return PresetFile(url: url, size: (attributes[.size] as? Int) ?? 0, modified: (attributes[.modificationDate] as? Date) ?? .distantPast)
    }

    private func posixError(_ what: String) -> EngineError {
        EngineError(message: "\(what): \(String(cString: strerror(errno))).")
    }
}

/// What the engine made of a preset file, cached by the file's identity.
enum PresetStatus: Equatable, Sendable {
    case valid(film: String, paper: String, scan: Bool)
    case invalid(String)
}
