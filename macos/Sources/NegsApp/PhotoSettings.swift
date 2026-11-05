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
        let names = [source.deletingPathExtension().appendingPathExtension("json"), source.appendingPathExtension("json")]
        guard let url = names.first(where: { isFree($0) }) else {
            throw EngineError(message: "Could not save the settings: \(names[1].lastPathComponent) already exists and isn't this photograph's.")
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(self).write(to: url, options: .atomic)
        return url
    }

    /// Nothing is there, or this photograph's own settings are.
    private func isFree(_ url: URL) -> Bool {
        guard let data = try? Data(contentsOf: url) else { return !FileManager.default.fileExists(atPath: url.path) }
        struct Header: Decodable { var negsSettings: Int; var image: String }
        return (try? JSONDecoder().decode(Header.self, from: data))?.image == image
    }
}
