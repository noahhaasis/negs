import CryptoKit
import Foundation
import IOSurface

/// One lossless display frame, separate from the editing session and the
/// helper's temporary buffers. Serial I/O keeps invalidation ahead of later
/// writes. A frame is published atomically only after a settled render.
final class RenderedPreviewCache: @unchecked Sendable {
    /// Records without the source and working sizes don't decode, and their
    /// keys predate schema 2, so they are never restored.
    struct Header: Codable {
        var key: String
        var width: Int
        var height: Int
        var photoSize: CGSize
        var sourceSize: CGSize
        var workingSize: CGSize
        var raw: Bool
        var checksum: String
    }
    struct Frame {
        var surface: IOSurface
        var photoSize: CGSize
        /// For Image Size readouts: the decoded source and the uncapped working frame.
        var sourceSize: CGSize
        var workingSize: CGSize
        var raw: Bool
    }

    let file: URL
    private let queue = DispatchQueue(label: "dev.negs.rendered-preview", qos: .userInitiated)
    private static let maxBytes = 8192 * 8192 * 4

    init(file: URL = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("dev.negs.app/rendered-preview.cache")) {
        self.file = file
    }

    /// Canvas-only edits (crop, rotation and border) are reapplied by the
    /// canvas, exactly as for a fresh render, and don't change these pixels.
    /// With an Image Size target the crop's extent is part of `state.rendered`.
    static func key(source: URL, state: EditState, paths: EnginePaths) -> String? {
        struct Identity: Encodable {
            var schema = 2
            var state: EditState
            var files: [String]
        }
        func stamp(_ url: URL) throws -> String {
            let a = try FileManager.default.attributesOfItem(atPath: url.path)
            guard let modified = a[.modificationDate] as? Date, let created = a[.creationDate] as? Date else {
                throw EngineError(message: "File identity unavailable")
            }
            return "\(url.standardizedFileURL.path)|\(a[.systemNumber] ?? "")|\(a[.systemFileNumber] ?? "")|\(a[.size] ?? "")|\(modified.timeIntervalSince1970)|\(created.timeIntervalSince1970)"
        }
        do {
            var files = try [stamp(source), stamp(paths.helper), stamp(paths.data)]
            guard let items = FileManager.default.enumerator(at: paths.data, includingPropertiesForKeys: [.isRegularFileKey]) else { return nil }
            for case let url as URL in items {
                if try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true { files.append(try stamp(url)) }
            }
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
            return digest(try encoder.encode(Identity(state: state.rendered, files: files.sorted())))
        } catch { return nil }
    }

    func invalidate() {
        queue.async { try? FileManager.default.removeItem(at: self.file) }
    }

    func flush() async {
        await withCheckedContinuation { continuation in queue.async { continuation.resume() } }
    }

    func load(source: URL, state: EditState, paths: EnginePaths) async -> Frame? {
        await withCheckedContinuation { continuation in
            queue.async {
                let frame = Self.key(source: source, state: state, paths: paths).flatMap { self.read(key: $0) }
                continuation.resume(returning: frame)
            }
        }
    }

    private func read(key: String) -> Frame? {
        guard let size = (try? FileManager.default.attributesOfItem(atPath: file.path))?[.size] as? Int,
              size <= Self.maxBytes + 65_536,
              let data = try? Data(contentsOf: file, options: .mappedIfSafe), let newline = data.prefix(65_536).firstIndex(of: 10),
              let header = try? JSONDecoder().decode(Header.self, from: data[..<newline]),
              header.key == key, (1...8192).contains(header.width), (1...8192).contains(header.height),
              header.photoSize.width.isFinite, header.photoSize.height.isFinite,
              header.photoSize.width > 0, header.photoSize.height > 0,
              [header.sourceSize, header.workingSize].allSatisfy({ $0.width.isFinite && $0.height.isFinite && $0.width >= 1 && $0.height >= 1 }) else { return nil }
        let pixels = Data(data[(newline + 1)...])
        guard pixels.count == header.width * header.height * 4, Self.digest(pixels) == header.checksum,
              let surface = try? EditorModel.previewSurface(pixels, width: header.width, height: header.height, bgra: true) else { return nil }
        return Frame(surface: surface, photoSize: header.photoSize, sourceSize: header.sourceSize, workingSize: header.workingSize, raw: header.raw)
    }

    /// Retains the finished surface until its bytes are safely on disk. It
    /// never reads the helper's preview file, which the next render can reuse.
    func save(_ surface: IOSurface, photoSize: CGSize, sourceSize: CGSize, workingSize: CGSize, raw: Bool, key: String) {
        queue.async {
            let width = surface.width, height = surface.height
            surface.lock(options: .readOnly, seed: nil)
            var pixels = Data(count: width * height * 4)
            pixels.withUnsafeMutableBytes { bytes in
                for y in 0..<height {
                    memcpy(bytes.baseAddress!.advanced(by: y * width * 4), surface.baseAddress.advanced(by: y * surface.bytesPerRow), width * 4)
                }
            }
            surface.unlock(options: .readOnly, seed: nil)
            let header = Header(key: key, width: width, height: height, photoSize: photoSize, sourceSize: sourceSize, workingSize: workingSize, raw: raw, checksum: Self.digest(pixels))
            do {
                var data = try JSONEncoder().encode(header); data.append(10)
                try FileManager.default.createDirectory(at: self.file.deletingLastPathComponent(), withIntermediateDirectories: true)
                let staging = self.file.appendingPathExtension("tmp")
                FileManager.default.createFile(atPath: staging.path, contents: nil)
                defer { try? FileManager.default.removeItem(at: staging) }
                let output = try FileHandle(forWritingTo: staging)
                defer { try? output.close() }
                try output.write(contentsOf: data); try output.write(contentsOf: pixels)
                try output.close()
                guard rename(staging.path, self.file.path) == 0 else { throw EngineError(message: "Could not cache preview") }
            } catch { /* A cache failure must never interrupt editing. */ }
        }
    }

    private static func digest(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
}
