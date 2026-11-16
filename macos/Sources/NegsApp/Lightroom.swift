import AppKit
import ImageIO

// Open in Lightroom (ticket 48): photographs developed to 16-bit TIFFs by
// the batch exporter, then handed to Lightroom in one open call so it asks
// once ("Add 12 Photos").
//
// The cloud Lightroom copies what it adds into its own library, so its TIFFs
// wait in Negs' caches and are deleted once Lightroom has its copy, or after
// a week. Lightroom Classic adds files where they are, so its TIFFs go to a
// folder in Pictures and are never deleted by Negs.

/// Which Lightroom a send is for.
enum LightroomKind: String, Codable, CaseIterable, Sendable {
    case cloud, classic

    /// The App Store build first, then Adobe's installer's.
    var bundleIdentifiers: [String] {
        switch self {
        case .cloud: ["com.adobe.mas.lightroomCC", "com.adobe.lightroomCC"]
        case .classic: ["com.adobe.LightroomClassicCC7"]
        }
    }
    var title: String {
        switch self {
        case .cloud: "Lightroom"
        case .classic: "Lightroom Classic"
        }
    }
    /// Where its TIFFs are written.
    var folder: URL {
        switch self {
        case .cloud: Lightroom.cloudFolder
        case .classic: Lightroom.classicFolder
        }
    }
}

/// The Lightroom apps installed, as found after launch.
struct LightroomApps: Equatable, Sendable {
    var cloud: URL?
    var classic: URL?

    func app(_ kind: LightroomKind) -> URL? { kind == .cloud ? cloud : classic }
    var any: Bool { cloud != nil || classic != nil }
}

enum Lightroom {
    /// `~/Library/Caches/<bundle id>/Lightroom`: removed with the app's data.
    static var cloudFolder: URL { AppDataLocations.cachesDirectory.appendingPathComponent("Lightroom", isDirectory: true) }
    /// Classic references what it adds in place, so this folder is the user's.
    static var classicFolder: URL {
        FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask)[0].appendingPathComponent("Negs for Lightroom", isDirectory: true)
    }
    /// A cloud TIFF Lightroom never copied (Cancel, or a library elsewhere) is deleted after this.
    static let keep: TimeInterval = 7 * 24 * 3600

    // MARK: Detection

    /// What the last detection found, so the menus and the button are
    /// right in the first frame without asking Launch Services then.
    static var remembered: LightroomApps {
        let defaults = UserDefaults.standard
        return LightroomApps(cloud: defaults.url(forKey: "lightroom.cloud"), classic: defaults.url(forKey: "lightroom.classic"))
    }
    static func remember(_ apps: LightroomApps) {
        let defaults = UserDefaults.standard
        if let cloud = apps.cloud { defaults.set(cloud, forKey: "lightroom.cloud") } else { defaults.removeObject(forKey: "lightroom.cloud") }
        if let classic = apps.classic { defaults.set(classic, forKey: "lightroom.classic") } else { defaults.removeObject(forKey: "lightroom.classic") }
    }

    /// Asks Launch Services (about 7 ms the first time): never on the main actor at launch.
    nonisolated static func detect() -> LightroomApps {
        func find(_ kind: LightroomKind) -> URL? {
            kind.bundleIdentifiers.lazy.compactMap { NSWorkspace.shared.urlsForApplications(withBundleIdentifier: $0).first }.first
        }
        return LightroomApps(cloud: find(.cloud), classic: find(.classic))
    }

    // MARK: Naming

    /// `IMG_0412-Negs.tif`, as the batch's custom naming makes it.
    static let naming = "{name}-Negs"

    /// A cloud TIFF gets a name its capture-date folders in Lightroom's
    /// library don't have yet, so finding that name there later means
    /// Lightroom copied this file (and a photograph sent again is a new one).
    nonisolated static func uniqueInLibrary(_ file: URL, libraries: [URL], fileManager: FileManager = .default) -> URL {
        let capture = captureDate(of: file)
        let folders = libraries.flatMap { library in dateFolders(capture: capture).map { library.appendingPathComponent($0) } }
        let directory = file.deletingLastPathComponent()
        let name = BatchNames.numbered(file.lastPathComponent) { candidate in
            (candidate != file.lastPathComponent && fileManager.fileExists(atPath: directory.appendingPathComponent(candidate).path))
                || folders.contains { fileManager.fileExists(atPath: $0.appendingPathComponent(candidate).path) }
        }
        guard name != file.lastPathComponent else { return file }
        let renamed = directory.appendingPathComponent(name)
        return (try? fileManager.moveItem(at: file, to: renamed)) != nil ? renamed : file
    }

    // MARK: Lightroom's library

    /// Each catalog's `originals` folder in every `*.lrlibrary` in Pictures:
    /// `Lightroom Library.lrlibrary/<catalog id>/originals`. Internal and
    /// undocumented, so only a hint for cleanup.
    nonisolated static func libraries(pictures: URL = FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask)[0],
                                      fileManager: FileManager = .default) -> [URL] {
        let bundles = (try? fileManager.contentsOfDirectory(at: pictures, includingPropertiesForKeys: nil)) ?? []
        return bundles.filter { $0.pathExtension == "lrlibrary" }.flatMap { bundle -> [URL] in
            let catalogs = (try? fileManager.contentsOfDirectory(at: bundle, includingPropertiesForKeys: nil)) ?? []
            return catalogs.map { $0.appendingPathComponent("originals", isDirectory: true) }.filter { fileManager.fileExists(atPath: $0.path) }
        }
    }

    /// The folders under `originals` a file captured on `capture` lands in:
    /// `2026/2026-11-15`, and the day either side for time zones. A file
    /// without a capture date goes to `0000/0000-00-00`.
    nonisolated static func dateFolders(capture: DateComponents?) -> [String] {
        guard let capture, let year = capture.year, let month = capture.month, let day = capture.day else { return ["0000/0000-00-00"] }
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        guard let date = calendar.date(from: DateComponents(year: year, month: month, day: day, hour: 12)) else { return ["0000/0000-00-00"] }
        return [0, -1, 1].compactMap { offset in
            calendar.date(byAdding: .day, value: offset, to: date).map { day in
                let parts = calendar.dateComponents([.year, .month, .day], from: day)
                return String(format: "%04d/%04d-%02d-%02d", parts.year!, parts.year!, parts.month!, parts.day!)
            }
        }
    }

    /// The EXIF capture date as written, without a time zone; nil without one.
    nonisolated static func captureDate(of file: URL) -> DateComponents? {
        guard let source = CGImageSourceCreateWithURL(file as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] else { return nil }
        let exif = properties[kCGImagePropertyExifDictionary] as? [CFString: Any]
        let tiff = properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
        let text = (exif?[kCGImagePropertyExifDateTimeOriginal] ?? exif?[kCGImagePropertyExifDateTimeDigitized] ?? tiff?[kCGImagePropertyTIFFDateTime]) as? String
        return text.flatMap(exifDate)
    }

    /// `2026:11:15 14:03:22` → 2026-11-15.
    nonisolated static func exifDate(_ text: String) -> DateComponents? {
        let parts = text.prefix(10).split(separator: ":").compactMap { Int($0) }
        guard parts.count == 3, parts[0] > 0, (1...12).contains(parts[1]), (1...31).contains(parts[2]) else { return nil }
        return DateComponents(year: parts[0], month: parts[1], day: parts[2])
    }

    /// Whether Lightroom has its copy: the same name and size in one of
    /// the capture date's folders of any library.
    nonisolated static func copied(name: String, size: Int, capture: DateComponents?, libraries: [URL], fileManager: FileManager = .default) -> Bool {
        for library in libraries {
            for folder in dateFolders(capture: capture) {
                let copy = library.appendingPathComponent(folder).appendingPathComponent(name)
                if let attributes = try? fileManager.attributesOfItem(atPath: copy.path),
                   (attributes[.size] as? NSNumber)?.intValue == size { return true }
            }
        }
        return false
    }

    // MARK: Cleanup

    /// What a cleanup pass found.
    struct Cleanup: Equatable, Sendable {
        /// Deleted because Lightroom has a copy, or because they're older than a week.
        var copied = 0, expired = 0
        /// Still waiting for Lightroom.
        var waiting = 0
    }

    /// Deletes the cloud folder's TIFFs that Lightroom has copied, and those
    /// older than a week. Off the main actor; cheap when nothing waits.
    nonisolated static func cleanUp(folder: URL = cloudFolder, libraries: [URL]? = nil, now: Date = Date(),
                                    fileManager: FileManager = .default) -> Cleanup {
        var result = Cleanup()
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey, .isRegularFileKey]
        guard let files = try? fileManager.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles]),
              !files.isEmpty else { return result }
        let libraries = libraries ?? Self.libraries(fileManager: fileManager)
        for file in files {
            guard let values = try? file.resourceValues(forKeys: Set(keys)), values.isRegularFile == true else { continue }
            if let modified = values.contentModificationDate, now.timeIntervalSince(modified) > keep {
                if (try? fileManager.removeItem(at: file)) != nil { result.expired += 1 }
            } else if let size = values.fileSize,
                      copied(name: file.lastPathComponent, size: size, capture: captureDate(of: file), libraries: libraries, fileManager: fileManager) {
                if (try? fileManager.removeItem(at: file)) != nil { result.copied += 1 }
            } else {
                result.waiting += 1
            }
        }
        return result
    }

    /// The bytes the files in `folder` take; nil if it's missing.
    nonisolated static func size(of folder: URL, fileManager: FileManager = .default) -> Int? {
        guard let files = try? fileManager.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.totalFileAllocatedSizeKey, .fileSizeKey],
                                                               options: [.skipsHiddenFiles]) else { return nil }
        return files.reduce(0) { total, file in
            let values = try? file.resourceValues(forKeys: [.totalFileAllocatedSizeKey, .fileSizeKey])
            return total + (values?.totalFileAllocatedSize ?? values?.fileSize ?? 0)
        }
    }

    /// Deletes the TIFFs in `folder` (only those Negs makes), keeping the folder.
    nonisolated static func deleteSent(in folder: URL, fileManager: FileManager = .default) {
        let files = (try? fileManager.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
        for file in files where ["tif", "tiff"].contains(file.pathExtension.lowercased()) { try? fileManager.removeItem(at: file) }
    }

    // MARK: Opening

    /// Hands `files` to Lightroom in one open call, so it asks once. A
    /// Lightroom that isn't running drops files opened while it starts
    /// (tested 2026-11-16: no sheet), so it's started first and given its
    /// window and 3 s more (1.5 s was enough in two tries out of two).
    @MainActor static func open(_ files: [URL], in app: URL, starting: @escaping @MainActor () -> Void) async throws {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        let identifier = Bundle(url: app)?.bundleIdentifier
        let running = identifier.flatMap { NSRunningApplication.runningApplications(withBundleIdentifier: $0).first }
        if running == nil || running?.isFinishedLaunching == false || Date().timeIntervalSince(running?.launchDate ?? .distantPast) < 15 {
            starting()
            let launched: NSRunningApplication
            if let running { launched = running } else { launched = try await NSWorkspace.shared.openApplication(at: app, configuration: configuration) }
            let start = Date()
            while !launched.isFinishedLaunching || !hasMainWindow(launched.processIdentifier), Date().timeIntervalSince(start) < 60 {
                if launched.isTerminated { break }
                try await Task.sleep(for: .milliseconds(200))
            }
            // Started by us: 3 s after its window. Started by someone a
            // moment ago: up to 3 s, until 15 s after its launch.
            let since = Date().timeIntervalSince(launched.launchDate ?? .distantPast)
            try await Task.sleep(for: .seconds(running == nil ? 3 : max(0, min(3, 15 - since))))
        }
        _ = try await NSWorkspace.shared.open(files, withApplicationAt: app, configuration: configuration)
    }

    /// An ordinary window of `pid`'s on screen, over 300 pt tall. The
    /// window list gives owners and sizes without Screen Recording.
    static func hasMainWindow(_ pid: pid_t) -> Bool {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return false }
        return list.contains { window in
            (window[kCGWindowOwnerPID as String] as? pid_t) == pid && (window[kCGWindowLayer as String] as? Int) == 0
                && (((window[kCGWindowBounds as String] as? [String: Any])?["Height"] as? Double) ?? 0) > 300
        }
    }
}

// MARK: - The editor's part

extension EditorModel {
    /// The Lightroom items to offer, in menu order: those installed.
    var lightroomKinds: [LightroomKind] {
        LightroomKind.allCases.filter { lightroomApps.app($0) != nil }
    }

    /// "Open in Lightroom", or "Open 12 Photographs in Lightroom Classic".
    nonisolated static func lightroomTitle(_ kind: LightroomKind, count: Int) -> String {
        count > 1 ? "Open \(count) Photographs in \(kind.title)" : "Open in \(kind.title)"
    }

    /// Whether `targets` can go to Lightroom now: as a batch export could start.
    func canOpenInLightroom(_ kind: LightroomKind, targets: [URL]) -> Bool {
        lightroomApps.app(kind) != nil && !targets.isEmpty && ready && !saving && !cropping && canStartBatch
    }

    /// Develops `targets` (what Export acts on unless given) to full-size
    /// 16-bit TIFFs in the export settings' colour space, with their EXIF,
    /// profile and rating, then opens them all in Lightroom at once.
    func openInLightroom(_ kind: LightroomKind, targets: [URL]? = nil) {
        let targets = targets ?? exportTargets
        guard canOpenInLightroom(kind, targets: targets) else { return }
        let folder = kind.folder
        do { try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true) } catch {
            self.error = "Couldn’t create the folder for \(kind.title): \(error.localizedDescription)"; return
        }
        let options = lightroomOptions(kind)
        Task { await exportBatch(targets, to: folder, options: options) }
    }

    /// Full size, TIFF 16-bit in the export settings' colour space, the
    /// export engine last chosen, `IMG_0412-Negs.tif` numbered if taken.
    func lightroomOptions(_ kind: LightroomKind) -> BatchOptions {
        var options = BatchOptions(format: OutputFormat.tiff.rawValue, engine: ExportEngine.selected.rawValue, edge: ExportSize.Edge.full.rawValue,
                                   pixels: ExportSize.full.pixels, bitDepth: 16,
                                   colorSpace: SaveSettings.effectiveSpace(SaveSettings.chosenSpace, bitDepth: 16), lowMemory: lowMemoryExport)
        options.naming = .custom
        options.custom = Lightroom.naming
        options.existing = .number
        options.lightroom = kind
        return options
    }

    /// The batch has ended: what it wrote goes to Lightroom in one call.
    func lightroomBatchFinished(_ run: BatchRun, kind: LightroomKind) {
        let written = run.exported.compactMap(\.written)
        let failed = run.failures.count
        let photographs = written.count == 1 ? "1 photograph" : "\(written.count) photographs"
        let failures = failed > 0 ? "; \(failed) failed" : ""
        guard !written.isEmpty, !run.cancelled else {
            lightroomWords = nil
            status = run.cancelled ? "Sending to \(kind.title) cancelled" : "Nothing sent to \(kind.title)\(failures)"
            if kind == .cloud { scheduleLightroomCleanup() }
            return
        }
        guard let app = lightroomApps.app(kind) else {
            status = "\(kind.title) isn’t installed any more"
            return
        }
        status = "Sending \(photographs) to \(kind.title)…"
        lightroomWords = status
        lightroomHandingOver = kind == .cloud
        Task { [weak self] in
            // Named so Lightroom's library can't already have them, which
            // is what tells cleanup that Lightroom took its copy.
            let files = kind == .cloud
                ? await Task.detached(priority: .userInitiated) {
                    let libraries = Lightroom.libraries()
                    return written.map { Lightroom.uniqueInLibrary($0, libraries: libraries) }
                }.value
                : written
            self?.lightroomHandingOver = false
            do {
                try await Lightroom.open(files, in: app) { self?.lightroomWords = "Starting \(kind.title)…" }
                let sent = "Sent \(photographs) to \(kind.title)\(failures)"
                self?.status = sent
                self?.lightroomWords = sent
                try? await Task.sleep(for: .seconds(4))
                if self?.lightroomWords == sent { self?.lightroomWords = nil }
            } catch {
                self?.lightroomWords = nil
                self?.status = ""
                self?.error = "Couldn’t open \(kind.title): \(error.localizedDescription)"
            }
            if kind == .cloud { self?.scheduleLightroomCleanup() }
        }
    }

    // MARK: Detection and cleanup, never on the launch path

    /// Cloud TIFFs are being written or renamed. A photograph sent again
    /// keeps the name and size of Lightroom's earlier copy until it's
    /// renamed, so cleanup would take it for copied: it waits.
    var lightroomSending: Bool {
        lightroomHandingOver || (batch?.running == true && batch?.journal.options.lightroom == .cloud)
    }

    /// After the first window: which Lightroom is installed, and the cloud
    /// TIFFs Lightroom has copied since. Again when Negs becomes active
    /// (Lightroom installed meanwhile, or photos added there), at most
    /// every 10 s.
    func refreshLightroom(delay: Duration = .zero) {
        if let last = lightroomDetected, Date().timeIntervalSince(last) < 10 { return }
        lightroomDetected = Date()
        Task { [weak self] in
            if delay > .zero { try? await Task.sleep(for: delay) }
            let sending = self?.lightroomSending ?? true
            let (apps, cleanup) = await Task.detached(priority: .utility) { (Lightroom.detect(), sending ? Lightroom.Cleanup() : Lightroom.cleanUp()) }.value
            guard let self else { return }
            if apps != self.lightroomApps { self.lightroomApps = apps }
            Lightroom.remember(apps)
            if cleanup.waiting > 0, self.lightroomCleanup == nil { self.scheduleLightroomCleanup() }
        }
    }

    /// Looks for Lightroom's copies a few times after a send, as long as
    /// TIFFs wait: Add is clicked, then the copy takes a moment.
    func scheduleLightroomCleanup() {
        lightroomCleanup?.cancel()
        lightroomCleanup = Task { [weak self] in
            for seconds in [20, 60, 120, 300, 600, 1800] {
                try? await Task.sleep(for: .seconds(seconds))
                if Task.isCancelled { return }
                if self?.lightroomSending ?? true { continue }
                let cleanup = await Task.detached(priority: .utility) { Lightroom.cleanUp() }.value
                if cleanup.waiting == 0 { break }
            }
            self?.lightroomCleanup = nil
        }
    }
}
