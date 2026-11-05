import Foundation

/// Everything edited on a photograph, film to pins, as a JSON file beside it.
struct PhotoSettingsFile: Codable, Equatable {
    /// Marks the file as ours, and its format.
    var negsSettings = 1
    /// The photograph's own file name, which tells a RAW from its JPEG.
    var image: String
    var state: EditState

    /// Writes `DSC_0001.json` beside `DSC_0001.NEF`, replacing that photograph's
    /// earlier file. A RAW and its JPEG share a base name, and a JSON file there
    /// may not be ours, so if the name is taken this writes `DSC_0001.NEF.json`
    /// instead. It never overwrites anything else.
    func write(beside source: URL) throws -> URL {
        let names = Self.names(beside: source)
        guard let url = names.first(where: { isFree($0) }) else {
            throw EngineError(message: "Could not save the settings: \(names[1].lastPathComponent) already exists and isn't this photograph's.")
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(self).write(to: url, options: .atomic)
        return url
    }

    /// This photograph's saved settings, if it has any. Other photographs' files
    /// and other JSON are ignored; a file of its own that can't be read throws.
    static func read(beside source: URL) throws -> PhotoSettingsFile? {
        for url in names(beside: source) {
            guard let data = try? Data(contentsOf: url), isOurs(data, for: source.lastPathComponent) else { continue }
            guard data.count <= 1_048_576 else {
                throw EngineError(message: "“\(url.lastPathComponent)” is too large to be settings.")
            }
            do {
                return try JSONDecoder().decode(PhotoSettingsFile.self, from: data)
            } catch {
                throw EngineError(message: "The settings in “\(url.lastPathComponent)” can’t be read, so the photograph opened without them.")
            }
        }
        return nil
    }

    private static func names(beside source: URL) -> [URL] {
        [source.deletingPathExtension().appendingPathExtension("json"), source.appendingPathExtension("json")]
    }

    /// Whether a file is settings saved for the photograph named `image`.
    private static func isOurs(_ data: Data, for image: String) -> Bool {
        struct Header: Decodable { var negsSettings: Int; var image: String }
        return (try? JSONDecoder().decode(Header.self, from: data))?.image == image
    }

    /// Nothing is there, or this photograph's own settings are.
    private func isFree(_ url: URL) -> Bool {
        guard let data = try? Data(contentsOf: url) else { return !FileManager.default.fileExists(atPath: url.path) }
        return Self.isOurs(data, for: image)
    }
}
