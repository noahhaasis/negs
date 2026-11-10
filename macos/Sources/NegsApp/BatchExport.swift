import AppKit
import ImageIO
import Observation
import SwiftUI
import UniformTypeIdentifiers

/// A batch file's name: `{name}_{film}` as a single export suggests,
/// `{name}`, or the user's own text with those tokens.
enum BatchNaming: String, CaseIterable, Codable, Sendable {
    case nameFilm, name, custom

    var title: String {
        switch self {
        case .nameFilm: "Name_Film"
        case .name: "Name"
        case .custom: "Custom"
        }
    }
}

/// What a batch does when a file of its name is already there.
enum BatchExisting: String, CaseIterable, Codable, Sendable {
    case number, replace, skip

    var title: String {
        switch self {
        case .number: "Add a Number"
        case .replace: "Replace"
        case .skip: "Skip"
        }
    }
}

/// The batch dialog's choices, as the journal keeps them.
struct BatchOptions: Codable, Equatable, Sendable {
    var format = OutputFormat.jpeg.rawValue
    var engine = ExportEngine.reference.rawValue
    var edge = ExportSize.Edge.full.rawValue
    var pixels = ExportSize.full.pixels
    var bitDepth: Int?
    var colorSpace = 0
    var jpegQuality = 95
    var naming = BatchNaming.nameFilm
    var custom = "{name}_{film}"
    var existing = BatchExisting.number
    var lowMemory = false

    var outputFormat: OutputFormat { OutputFormat(rawValue: format) ?? .jpeg }
    var exportEngine: ExportEngine { ExportEngine(rawValue: engine) ?? .reference }
    var size: ExportSize { ExportSize(edge: ExportSize.Edge(rawValue: edge) ?? .full, pixels: pixels) }
    var save: SaveSettings { SaveSettings(bitDepth: bitDepth, colorSpace: colorSpace, jpegQuality: jpegQuality) }
    var fileExtension: String { outputFormat.type.preferredFilenameExtension ?? format }

    /// The file name for a photograph named `name` (without its extension,
    /// a duplicate's number included) developed with `film`.
    func fileName(name: String, film: String) -> String {
        let template = switch naming {
        case .nameFilm: "{name}_{film}"
        case .name: "{name}"
        case .custom: custom.isEmpty ? "{name}" : custom
        }
        var base = template.replacingOccurrences(of: "{name}", with: name).replacingOccurrences(of: "{film}", with: film)
        // A file name, never a path.
        base = base.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
        if base.trimmingCharacters(in: .whitespaces).isEmpty || base.hasPrefix(".") { base = name + base }
        return base + "." + fileExtension
    }
}

/// One photograph of a batch: its edits as they were when Export was
/// chosen, and how it went.
struct BatchItem: Codable, Equatable, Identifiable, Sendable {
    enum Status: String, Codable, Sendable { case waiting, done, failed, skipped }

    /// The library item: a photograph, or a duplicate of one.
    var item: URL
    /// As the strip names it: `IMG_0412.CR3 (2)`.
    var title: String
    var state: EditState
    /// Its Match camera brightness is solved just before it exports.
    var needsMatch: Bool
    /// The match, once solved, is stored in its sidecar (it had edits).
    var storesMatch: Bool
    /// `xmp:Rating`: 1–5, or −1 rejected; nil writes none.
    var rating: Int?
    /// The name it gets, before If a File Exists.
    var fileName: String
    /// The photograph's size before its crop, when known (the open one's):
    /// a 35mm border needs it. Otherwise read just before it exports.
    var size: CGSize?
    /// Its stored edits' look, which a match solved for them is stored with.
    var look: String?
    var status = Status.waiting
    var reason: String?
    /// Where it was written.
    var written: URL?

    var id: URL { item }
}

/// An unfinished batch, so the next launch can resume it. Written
/// atomically after each photograph; removed when the batch ends.
struct BatchJournal: Codable, Equatable, Sendable {
    var schema = 1
    var destination: URL
    var bookmark: Data?
    var options: BatchOptions
    var items: [BatchItem]

    /// In app data; under XCTest a temporary folder's.
    static let url = NSClassFromString("XCTestCase") != nil
        ? FileManager.default.temporaryDirectory.appendingPathComponent("negs-batch-\(UUID().uuidString)/active-export.json")
        : AppDataLocations.supportDirectory.appendingPathComponent("active-export.json")

    /// Photographs still to export.
    var remaining: Int { items.filter { $0.status == .waiting }.count }

    func write(to url: URL = BatchJournal.url) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try JSONEncoder().encode(self).write(to: url, options: .atomic)
    }

    static func read(from url: URL = BatchJournal.url) -> BatchJournal? {
        guard let data = try? Data(contentsOf: url), var journal = try? JSONDecoder().decode(BatchJournal.self, from: data), journal.schema == 1 else { return nil }
        // A folder that moved is found through its bookmark.
        var stale = false
        if !FileManager.default.fileExists(atPath: journal.destination.path), let bookmark = journal.bookmark,
           let moved = try? URL(resolvingBookmarkData: bookmark, bookmarkDataIsStale: &stale) { journal.destination = moved }
        return journal
    }

    static func remove(at url: URL = BatchJournal.url) { try? FileManager.default.removeItem(at: url) }
}

/// Names in a batch: those that two photographs would share (a RAW+JPEG
/// pair) are numbered, as are those an existing file has, unless replaced
/// or skipped.
enum BatchNames {
    /// `name.ext`, then `name 2.ext`, `name 3.ext`… until `taken` says no.
    static func numbered(_ fileName: String, taken: (String) -> Bool) -> String {
        guard taken(fileName) else { return fileName }
        let url = URL(fileURLWithPath: fileName)
        let base = url.deletingPathExtension().lastPathComponent, ext = url.pathExtension
        var n = 2
        while taken("\(base) \(n).\(ext)") { n += 1 }
        return "\(base) \(n).\(ext)"
    }

    /// The batch's own names made unique among themselves, in order.
    static func unique(_ names: [String]) -> [String] {
        var used = Set<String>()
        return names.map { name in
            let unique = numbered(name) { used.contains($0.lowercased()) }
            used.insert(unique.lowercased())
            return unique
        }
    }
}

/// The rating an export embeds, as Lightroom writes it: the stars, or −1
/// for a reject. Nil (no packet at all) for an unrated, unrejected photo.
enum ExportRating {
    static func value(_ summary: LibrarySummary) -> Int? {
        if summary.flag == .reject { return -1 }
        return summary.rating > 0 ? summary.rating : nil
    }

    /// Adds `xmp:Rating` to the finished file without decoding its image:
    /// ImageIO copies the image data as it is and merges the metadata.
    /// Formats it can't do that for are left alone.
    static func embed(_ rating: Int, in file: URL) throws {
        guard let source = CGImageSourceCreateWithURL(file as CFURL, nil), let type = CGImageSourceGetType(source) else { return }
        // ImageIO can't add XMP to a PNG without encoding it again.
        if type as String == UTType.png.identifier { return try embedInPNG(rating, file: file) }
        let metadata = CGImageMetadataCreateMutable()
        guard CGImageMetadataSetValueWithPath(metadata, nil, "xmp:Rating" as CFString, "\(rating)" as CFString) else { return }
        let staging = file.deletingLastPathComponent().appendingPathComponent(".negs-rating-\(UUID().uuidString).\(file.pathExtension)")
        defer { try? FileManager.default.removeItem(at: staging) }
        guard let destination = CGImageDestinationCreateWithURL(staging as CFURL, type, 1, nil) else { return }
        let options: [CFString: Any] = [kCGImageDestinationMetadata: metadata, kCGImageDestinationMergeMetadata: true]
        var error: Unmanaged<CFError>?
        guard CGImageDestinationCopyImageSource(destination, source, options as CFDictionary, &error) else { return }
        guard rename(staging.path, file.path) == 0 else { throw EngineError(message: "Could not add the rating: \(String(cString: strerror(errno)))") }
    }

    /// The XMP packet with only the rating.
    static func packet(_ rating: Int) -> String {
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">"
            + "<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"\(rating)\"/></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    }

    /// A PNG gets the packet as an `iTXt` chunk (`XML:com.adobe.xmp`) before
    /// its image data, replacing one already there; every other byte stays.
    private static func embedInPNG(_ rating: Int, file: URL) throws {
        let data = try Data(contentsOf: file)
        let signature = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        guard data.count > 8, data.prefix(8) == signature else { return }
        let keyword = Data("XML:com.adobe.xmp".utf8)
        var chunks: [(type: Data, range: Range<Int>)] = []
        var at = 8
        while at + 12 <= data.count {
            let length = data[at..<at + 4].reduce(0) { $0 << 8 | Int($1) }
            guard at + 12 + length <= data.count else { return }
            chunks.append((data[at + 4..<at + 8], at..<at + 12 + length))
            at += 12 + length
        }
        guard let image = chunks.firstIndex(where: { $0.type == Data("IDAT".utf8) }) else { return }
        var body = Data("iTXt".utf8)
        body += keyword + Data([0, 0, 0, 0, 0]) + Data(packet(rating).utf8)
        var chunk = Data()
        let length = UInt32(body.count - 4)
        chunk += withUnsafeBytes(of: length.bigEndian) { Data($0) }
        chunk += body
        chunk += withUnsafeBytes(of: crc32(body).bigEndian) { Data($0) }
        var out = Data(signature)
        for (index, piece) in chunks.enumerated() {
            if index == image { out += chunk }
            let isXMP = piece.type == Data("iTXt".utf8) && data[piece.range.lowerBound + 8..<min(piece.range.upperBound, piece.range.lowerBound + 8 + keyword.count)] == keyword
            if !isXMP { out += data[piece.range] }
        }
        let staging = file.deletingLastPathComponent().appendingPathComponent(".negs-rating-\(UUID().uuidString).png")
        defer { try? FileManager.default.removeItem(at: staging) }
        try out.write(to: staging)
        guard rename(staging.path, file.path) == 0 else { throw EngineError(message: "Could not add the rating: \(String(cString: strerror(errno)))") }
    }

    private static let crcTable: [UInt32] = (0..<256).map { n -> UInt32 in
        var c = UInt32(n)
        for _ in 0..<8 { c = c & 1 != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        return c
    }

    static func crc32(_ data: Data) -> UInt32 {
        var c: UInt32 = 0xFFFF_FFFF
        for byte in data { c = crcTable[Int((c ^ UInt32(byte)) & 0xFF)] ^ (c >> 8) }
        return c ^ 0xFFFF_FFFF
    }

    /// The rating a file carries, for tests and checks.
    static func read(_ file: URL) -> Int? {
        guard let source = CGImageSourceCreateWithURL(file as CFURL, nil),
              let metadata = CGImageSourceCopyMetadataAtIndex(source, 0, nil),
              let tag = CGImageMetadataCopyTagWithPath(metadata, nil, "xmp:Rating" as CFString),
              let value = CGImageMetadataTagCopyValue(tag) else { return nil }
        return Int("\(value)")
    }
}

/// A batch as the status line and its popover show it: running, or
/// finished with failures until they've been seen.
@MainActor @Observable final class BatchRun {
    var journal: BatchJournal
    /// The photograph exporting now.
    var current: Int?
    var running = false
    var cancelled = false
    /// Where the journal is kept.
    @ObservationIgnored let journalURL: URL
    @ObservationIgnored var job: ExportJob?
    @ObservationIgnored var task: Task<Void, Never>?

    init(journal: BatchJournal, journalURL: URL) {
        self.journal = journal
        self.journalURL = journalURL
    }

    var items: [BatchItem] { journal.items }
    var total: Int { items.count }
    /// Exported, failed or skipped.
    var finished: Int { items.count - journal.remaining }
    var exported: [BatchItem] { items.filter { $0.status == .done } }
    var failures: [BatchItem] { items.filter { $0.status == .failed } }
    var skipped: [BatchItem] { items.filter { $0.status == .skipped } }
    var fraction: Double { total == 0 ? 1 : Double(finished) / Double(total) }
    /// Ended with something to look at: failures, or a cancel.
    var needsAttention: Bool { !running && (!failures.isEmpty || cancelled) }

    /// "7 of 12", as the popover's title.
    var title: String {
        if running { return "Exporting \(min(finished + 1, total)) of \(total)" }
        let exported = exported.count
        if cancelled { return "Cancelled after \(exported) of \(total)" }
        return total == 1 && exported == 1 ? "Exported 1 photograph" : "Exported \(exported) of \(total)"
    }

    func cancel() {
        guard running else { return }
        cancelled = true
        job?.cancel()
    }
}

/// The batch dialog's File Name and If a File Exists rows, remembered.
@MainActor final class BatchRows: NSObject, NSTextFieldDelegate {
    private let namingPopup = NSPopUpButton()
    private let customField = NSTextField()
    private let existingPopup = NSPopUpButton()
    private let example: (BatchNaming, String) -> String
    private let exampleLabel = NSTextField(labelWithString: "")
    var changed: () -> Void = {}
    let views: [NSView]

    var naming: BatchNaming { BatchNaming.allCases[max(namingPopup.indexOfSelectedItem, 0)] }
    var custom: String { customField.stringValue }
    var existing: BatchExisting { BatchExisting.allCases[max(existingPopup.indexOfSelectedItem, 0)] }

    /// `example` names the first photograph with a naming and custom text.
    init(example: @escaping (BatchNaming, String) -> String) {
        self.example = example
        let defaults = UserDefaults.standard
        namingPopup.addItems(withTitles: BatchNaming.allCases.map(\.title))
        namingPopup.selectItem(at: BatchNaming.allCases.firstIndex(of: defaults.string(forKey: "batchNaming").flatMap(BatchNaming.init) ?? .nameFilm) ?? 0)
        customField.stringValue = defaults.string(forKey: "batchCustomName") ?? "{name}_{film}"
        customField.placeholderString = "{name}_{film}"
        customField.toolTip = "{name} is the photograph’s name, {film} the film’s."
        customField.setAccessibilityLabel("Custom file name")
        customField.widthAnchor.constraint(equalToConstant: 180).isActive = true
        existingPopup.addItems(withTitles: BatchExisting.allCases.map(\.title))
        existingPopup.selectItem(at: BatchExisting.allCases.firstIndex(of: defaults.string(forKey: "batchExisting").flatMap(BatchExisting.init) ?? .number) ?? 0)
        exampleLabel.textColor = .secondaryLabelColor
        exampleLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        exampleLabel.lineBreakMode = .byTruncatingMiddle
        views = [NSStackView(views: [NSTextField(labelWithString: "File Name:"), namingPopup, customField]),
                 exampleLabel,
                 NSStackView(views: [NSTextField(labelWithString: "If a File Exists:"), existingPopup])]
        super.init()
        namingPopup.target = self; namingPopup.action = #selector(namingChanged)
        customField.delegate = self
        sync()
    }

    func remember() {
        let defaults = UserDefaults.standard
        defaults.set(naming.rawValue, forKey: "batchNaming")
        defaults.set(custom, forKey: "batchCustomName")
        defaults.set(existing.rawValue, forKey: "batchExisting")
    }

    /// The example again, after the format changed.
    func refresh() { sync() }

    private func sync() {
        customField.isHidden = naming != .custom
        exampleLabel.stringValue = "For example: " + example(naming, custom)
    }
    @objc private func namingChanged() {
        sync(); changed()
        if naming == .custom { customField.window?.makeFirstResponder(customField) }
    }
    func controlTextDidChange(_ notification: Notification) { sync() }
}

/// The popover over the status line's Show: how far the batch has come, what failed and
/// why, and Cancel, Retry Failed and Show in Finder.
struct BatchProgressView: View {
    let run: BatchRun
    let retry: () -> Void
    let showInFinder: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(run.title).font(.headline)
            ProgressView(value: run.fraction)
            if run.running, let index = run.current, run.items.indices.contains(index) {
                Text(run.items[index].title).font(.callout).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            } else {
                Text("To “\(run.journal.destination.lastPathComponent)”").font(.callout).foregroundStyle(.secondary).lineLimit(1)
            }
            if !run.failures.isEmpty {
                Divider()
                Text(run.failures.count == 1 ? "1 failed" : "\(run.failures.count) failed").font(.callout.weight(.semibold))
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(run.failures) { item in
                            VStack(alignment: .leading, spacing: 1) {
                                Text(item.title).lineLimit(1).truncationMode(.middle)
                                Text(item.reason ?? "Unknown error").foregroundStyle(.secondary).lineLimit(3)
                            }
                            .font(.callout)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .frame(maxHeight: 160)
                .fixedSize(horizontal: false, vertical: true)
            }
            if !run.skipped.isEmpty {
                Text(run.skipped.count == 1 ? "1 skipped: a file of its name exists." : "\(run.skipped.count) skipped: files of their names exist.")
                    .font(.callout).foregroundStyle(.secondary)
            }
            HStack {
                if run.running {
                    Button("Cancel") { run.cancel() }
                } else if !run.failures.isEmpty {
                    Button("Retry Failed", action: retry)
                }
                Spacer()
                Button("Show in Finder", action: showInFinder)
            }
        }
        .padding(14)
        .frame(width: 300)
    }
}
