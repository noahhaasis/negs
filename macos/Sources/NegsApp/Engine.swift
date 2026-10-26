import Foundation
import Darwin

struct EngineError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

struct EnginePaths {
    let helper: URL
    /// The reference exporter, `spektrafilm-f64`.
    let exporter: URL
    let data: URL
    /// The f32 command-line tool `spektrafilm`, for Fast (GPU) export. The
    /// app still starts without it; only a fast export then fails.
    var fastExporter: URL? = nil

    static func locate() throws -> EnginePaths {
        try locate(environment: ProcessInfo.processInfo.environment,
                   executableDir: Bundle.main.executableURL!.deletingLastPathComponent(),
                   resourceDir: Bundle.main.resourceURL!,
                   workingDir: URL(fileURLWithPath: FileManager.default.currentDirectoryPath))
    }

    /// Each tool comes from its environment override, else next to the app's
    /// executable, else `target/release` under `SPEKTRAFILM_ROOT` (default:
    /// the working directory).
    static func locate(environment env: [String: String], executableDir: URL, resourceDir: URL, workingDir: URL) throws -> EnginePaths {
        let fm = FileManager.default
        let root = env["SPEKTRAFILM_ROOT"].map { URL(fileURLWithPath: $0) } ?? workingDir
        func find(_ override: String, _ candidates: [URL], executable: Bool) throws -> URL {
            let urls = env[override].map { [URL(fileURLWithPath: $0)] } ?? candidates
            guard let url = urls.first(where: {
                executable ? fm.isExecutableFile(atPath: $0.path) : fm.fileExists(atPath: $0.path)
            }) else { throw EngineError(message: "Missing \(override). Build the app with scripts/package_macos_app.sh, or set SPEKTRAFILM_ROOT to your checkout.") }
            return url
        }
        func tool(_ override: String, _ name: String) throws -> URL {
            try find(override, [executableDir.appendingPathComponent(name), root.appendingPathComponent("target/release/\(name)")], executable: true)
        }
        return try EnginePaths(
            helper: tool("SPEKTRAFILM_NATIVE_HELPER", "spektrafilm-native"),
            exporter: tool("SPEKTRAFILM_F64_CLI", "spektrafilm-f64"),
            data: find("SPEKTRAFILM_DATA_DIR", [resourceDir.appendingPathComponent("data"), root.appendingPathComponent("data")], executable: false),
            fastExporter: try? tool("SPEKTRAFILM_F32_CLI", "spektrafilm")
        )
    }
}

/// The helper's last catalog, keyed by the helper binary and data folder, so a
/// launch can show the stocks before the helper answers. The helper's own
/// answer always replaces it.
enum CatalogCache {
    private static let defaultsKey = "catalogCache"

    private static func key(for paths: EnginePaths) -> String? {
        // Not URL.resourceValues, which caches per URL instance.
        guard let modified = (try? FileManager.default.attributesOfItem(atPath: paths.helper.path))?[.modificationDate] as? Date else { return nil }
        return "\(paths.helper.path)|\(modified.timeIntervalSince1970)|\(paths.data.path)"
    }

    static func load(for paths: EnginePaths, defaults: UserDefaults = .standard) -> JSONValue? {
        guard let key = key(for: paths), let data = defaults.data(forKey: defaultsKey),
              let entry = try? JSONDecoder().decode(JSONValue.self, from: data),
              entry["key"].string == key, entry["catalog"]["protocol"].double == 1 else { return nil }
        return entry["catalog"]
    }

    static func save(_ catalog: JSONValue, for paths: EnginePaths, defaults: UserDefaults = .standard) {
        guard let key = key(for: paths), let data = try? JSONEncoder().encode(JSONValue.object(["key": .string(key), "catalog": catalog])) else { return }
        defaults.set(data, forKey: defaultsKey)
    }
}

/// Which engine renders an export. Both use the same parameters, crop and
/// border. The GPU engine was measured against the reference on 6 photographs
/// x 7 stocks: worst pixel ΔE00 below 0.25, at most one 8-bit step, and grain
/// statistically equal but from different random numbers
/// (docs/gpu-vs-cpu-quality.md).
enum ExportEngine: String, CaseIterable {
    /// `spektrafilm-f64` on the CPU, the reference implementation.
    case reference
    /// `spektrafilm` (32-bit) on the GPU through wgpu/Metal, like the preview.
    case fast

    var title: String {
        switch self {
        case .reference: "Reference (CPU, 64-bit)"
        case .fast: "Fast (GPU)"
        }
    }

    /// `SPEKTRAFILM_BACKEND` for the exporter process.
    var backend: String {
        switch self {
        case .reference: "cpu"
        case .fast: "wgpu"
        }
    }

    func executable(in paths: EnginePaths) throws -> URL {
        switch self {
        case .reference: return paths.exporter
        case .fast:
            guard let url = paths.fastExporter else {
                throw EngineError(message: "Fast (GPU) export needs the spektrafilm command-line tool next to the app (or SPEKTRAFILM_F32_CLI). Export with Reference (CPU, 64-bit) instead, or rebuild the app with scripts/package_macos_app.sh.")
            }
            return url
        }
    }

    /// The exporter prints the backend it chose. A fast export without a GPU
    /// adapter falls back to the CPU (`Backend: CPU (rayon)`).
    static func ranOnGPU(log: String) -> Bool { log.contains("Backend: wgpu") }
    /// Whether the helper's reported backend name is the wgpu GPU backend.
    static func ranOnGPU(backend: String) -> Bool { backend.hasPrefix("wgpu") }

    /// The last choice, offered first next time. Fast until changed.
    static var selected: ExportEngine {
        get { UserDefaults.standard.string(forKey: "exportEngine").flatMap(ExportEngine.init) ?? .fast }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "exportEngine") }
    }
}

/// What an export did beyond writing the file.
struct ExportOutcome: Equatable {
    /// A fast export ran on the CPU because no GPU was available.
    var fellBackToCPU = false
}

/// Temporary files live in per-launch folders under $TMPDIR/Negs, each
/// beside a lock file held open while the folder is in use. A folder whose lock
/// is free was left by a crash or force quit. Making a new folder removes those,
/// along with the export staging files and exporter processes they recorded.
final class ScratchSpace: @unchecked Sendable {
    static let defaultRoot = FileManager.default.temporaryDirectory.appendingPathComponent("Negs", isDirectory: true)
    let directory: URL
    private let lockPath: String
    private let lock = NSLock()
    private var descriptor: Int32

    init(root: URL = ScratchSpace.defaultRoot) throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        Self.sweep(root)
        while true {
            let id = UUID().uuidString, path = root.appendingPathComponent("\(id).lock").path
            let fd = open(path, O_CREAT | O_EXCL | O_RDWR | O_CLOEXEC | O_EXLOCK, 0o600)
            guard fd >= 0 else { throw EngineError(message: "Could not create temporary files: \(String(cString: strerror(errno)))") }
            // A concurrent sweep can claim and unlink a new lock file before it is locked.
            var opened = stat(), linked = stat()
            if fstat(fd, &opened) == 0, stat(path, &linked) == 0, opened.st_ino == linked.st_ino {
                descriptor = fd; lockPath = path
                directory = root.appendingPathComponent(id, isDirectory: true)
                break
            }
            close(fd)
        }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    }

    deinit { remove() }

    /// Clear abandoned launches while respecting another running app's lock.
    static func cleanForRemoval(root: URL = defaultRoot) throws {
        var info = stat()
        if lstat(root.path, &info) == 0, info.st_mode & S_IFMT == S_IFLNK {
            guard unlink(root.path) == 0 else { throw EngineError(message: "Could not remove the temporary-data link.") }
            return
        }
        sweep(root)
        guard rmdir(root.path) != 0, errno != ENOENT else { return }
        throw EngineError(message: "Could not remove temporary data: \(String(cString: strerror(errno))). Close any other running copy of Negs and retry.")
    }

    /// Deletes the folder. The engine calls this when it stops.
    func remove() {
        lock.lock(); defer { lock.unlock() }
        guard descriptor >= 0 else { return }
        try? FileManager.default.removeItem(at: directory)
        unlink(lockPath)
        close(descriptor); descriptor = -1
    }

    private static func sweep(_ root: URL) {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: root.path)) ?? []
        for name in names where name.hasSuffix(".lock") {
            let path = root.appendingPathComponent(name).path
            let fd = open(path, O_RDWR | O_CLOEXEC | O_EXLOCK | O_NONBLOCK)
            guard fd >= 0 else { continue } // in use, or already removed
            let folder = root.appendingPathComponent(String(name.dropLast(5)), isDirectory: true)
            ExportRecord.abandon(in: folder)
            try? FileManager.default.removeItem(at: folder)
            unlink(path)
            close(fd)
        }
    }
}

/// What an export puts outside its scratch folder: the staging file beside the
/// destination and the exporter process. Recorded so a crash can be cleaned up.
struct ExportRecord: Codable {
    static let fileName = "export.json"
    var staging: String
    var exporter: String
    var pid: Int32?

    func write(to folder: URL) throws {
        try JSONEncoder().encode(self).write(to: folder.appendingPathComponent(Self.fileName), options: .atomic)
    }

    static func abandon(in folder: URL) {
        let jobs = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
        for job in jobs {
            guard let data = try? Data(contentsOf: job.appendingPathComponent(fileName)),
                  let record = try? JSONDecoder().decode(ExportRecord.self, from: data) else { continue }
            // Check the executable so a reused process ID is never killed.
            if let pid = record.pid, executablePath(of: pid) == record.exporter { kill(pid, SIGKILL) }
            if URL(fileURLWithPath: record.staging).lastPathComponent.hasPrefix(".negs-") { unlink(record.staging) }
        }
    }

    static func resolvedPath(_ url: URL) -> String {
        guard let resolved = realpath(url.path, nil) else { return url.path }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    private static func executablePath(of pid: Int32) -> String? {
        var buffer = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
        guard proc_pidpath(pid, &buffer, UInt32(buffer.count)) > 0 else { return nil }
        return String(cString: buffer)
    }
}

/// A serial worker owns the pipe protocol. No pipe reads or image work on the UI thread.
final class EngineClient: @unchecked Sendable {
    let paths: EnginePaths
    let scratch: ScratchSpace
    var directory: URL { scratch.directory }
    private let queue = DispatchQueue(label: "dev.negs.engine", qos: .userInitiated)
    private let lock = NSLock()
    private var process: Process?
    private var stopped = false
    private var input: FileHandle?
    private var output: FileHandle?
    private var log: FileHandle?
    private var pending = Data()
    private var sequence = 0

    init(paths: EnginePaths) throws {
        self.paths = paths
        scratch = try ScratchSpace()
    }

    func request(_ command: [String: JSONValue]) async throws -> (Int, JSONValue) {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                do { continuation.resume(returning: try self.exchange(command)) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }

    private func exchange(_ command: [String: JSONValue]) throws -> (Int, JSONValue) {
        try startIfNeeded()
        sequence += 1
        let id = sequence
        var body = command; body["id"] = .number(Double(id))
        var data = try JSONEncoder().encode(JSONValue.object(body)); data.append(10)
        do {
            try input!.write(contentsOf: data)
            while !pending.contains(10) {
                let bytes = output!.availableData
                guard !bytes.isEmpty else { throw EngineError(message: "The image engine stopped unexpectedly. Open the image again to retry.") }
                pending.append(bytes)
            }
            let newline = pending.firstIndex(of: 10)!
            let line = Data(pending[..<newline]); pending.removeSubrange(...newline)
            let reply = try JSONDecoder().decode(JSONValue.self, from: line)
            guard reply["id"].double == Double(id) else { throw EngineError(message: "Image engine response is out of sequence.") }
            if !reply["error"].string.isEmpty { throw EngineError(message: reply["error"].string) }
            return (id, reply["result"])
        } catch {
            if process?.isRunning != true { closeHandles() }
            throw error
        }
    }

    private func startIfNeeded() throws {
        lock.lock(); defer { lock.unlock() }
        guard !stopped else { throw CancellationError() }
        if process?.isRunning == true { return }
        closeHandles()
        let child = Process(), stdin = Pipe(), stdout = Pipe()
        let logURL = directory.appendingPathComponent("engine.log")
        FileManager.default.createFile(atPath: logURL.path, contents: nil)
        log = try FileHandle(forWritingTo: logURL)
        child.executableURL = paths.helper
        child.arguments = ["--data-dir", paths.data.path]
        child.standardInput = stdin; child.standardOutput = stdout; child.standardError = log
        try child.run()
        process = child; input = stdin.fileHandleForWriting; output = stdout.fileHandleForReading
    }

    private func closeHandles() {
        try? input?.close(); try? output?.close(); try? log?.close()
        input = nil; output = nil; log = nil; pending.removeAll()
    }

    func stop() {
        lock.lock(); stopped = true
        if let child = process, child.isRunning { kill(child.processIdentifier, SIGKILL) }
        lock.unlock()
        queue.sync {
            // Its stdout ends when the helper exits. Process.waitUntilExit would
            // spin the run loop instead, which can wait forever on the main thread.
            _ = try? self.output?.readToEnd()
            self.closeHandles()
            self.scratch.remove()
        }
    }
}

/// Export writes to a sibling temporary file. Cancel/failure leaves an existing
/// destination untouched; a successful render is committed with an atomic rename.
@MainActor final class ExportJob {
    private var process: Process?
    private var cancelled = false
    private var temporaryFiles: [URL] = []

    /// Develops `source` at full resolution with `state`, cropping and
    /// straightening to `crop`, adding `border`, fitting to `size`, and
    /// encoding with `save`, with `engine`.
    @discardableResult
    func run(paths: EnginePaths, scratch: ScratchSpace, source: URL, destination: URL, state: EditState, crop: CropSettings? = nil, border: BorderSettings?, size: ExportSize = .full, save: SaveSettings = SaveSettings(), engine: ExportEngine = .reference) async throws -> ExportOutcome {
        let fm = FileManager.default
        let executable = try engine.executable(in: paths)
        let work = scratch.directory.appendingPathComponent("export-\(UUID().uuidString)")
        try fm.createDirectory(at: work, withIntermediateDirectories: true)
        let staging = destination.deletingLastPathComponent().appendingPathComponent(".negs-\(UUID().uuidString).\(destination.pathExtension)")
        temporaryFiles = [work, staging]
        var record = ExportRecord(staging: staging.path, exporter: ExportRecord.resolvedPath(executable))
        try record.write(to: work)
        defer { try? fm.removeItem(at: work); try? fm.removeItem(at: staging); process = nil; temporaryFiles = [] }
        let params = work.appendingPathComponent("params.json")
        try JSONEncoder().encode(state.params).write(to: params)
        let logURL = work.appendingPathComponent("export.log")
        fm.createFile(atPath: logURL.path, contents: nil)
        let log = try FileHandle(forWritingTo: logURL)
        defer { try? log.close() }
        let child = Process()
        child.executableURL = executable
        child.arguments = ["process", source.path, "-o", staging.path, "--film", state.film, "--paper", state.paper, "--params", params.path, "--data-dir", paths.data.path]
        if state.params["io.scan_film"].bool { child.arguments!.append("--scan-film") }
        if let crop {
            let r = crop.rect
            child.arguments! += ["--crop", [r.minX, r.minY, r.width, r.height, crop.angle].map { "\(Double($0))" }.joined(separator: ",")]
        }
        if let border { child.arguments! += border.arguments }
        child.arguments! += size.arguments
        child.arguments! += save.arguments
        var environment = ProcessInfo.processInfo.environment
        environment["SPEKTRAFILM_BACKEND"] = engine.backend; child.environment = environment
        child.standardOutput = log; child.standardError = log
        process = child
        let status: Int32 = try await withCheckedThrowingContinuation { continuation in
            child.terminationHandler = { process in continuation.resume(returning: process.terminationStatus) }
            do {
                try child.run()
                record.pid = child.processIdentifier; try? record.write(to: work)
            } catch { child.terminationHandler = nil; continuation.resume(throwing: error) }
        }
        if cancelled { throw CancellationError() }
        let diagnostic = (try? String(contentsOf: logURL, encoding: .utf8)) ?? ""
        guard status == 0 else {
            let hint = engine == .fast ? " If the GPU ran out of memory, export with Reference (CPU, 64-bit) instead." : ""
            throw EngineError(message: "Export failed.\(hint)\n\(diagnostic.isEmpty ? "Unknown exporter error" : String(diagnostic.suffix(4000)))")
        }
        guard rename(staging.path, destination.path) == 0 else { throw EngineError(message: "Could not move the completed export to \(destination.path): \(String(cString: strerror(errno)))") }
        return ExportOutcome(fellBackToCPU: engine == .fast && !ExportEngine.ranOnGPU(log: diagnostic))
    }

    func cancel() {
        cancelled = true
        // No waitUntilExit: it spins the run loop and can hang the main thread.
        // run() removes the files again once the termination handler fires.
        if let process, process.isRunning { kill(process.processIdentifier, SIGKILL) }
        temporaryFiles.forEach { try? FileManager.default.removeItem(at: $0) }
    }
}
