import XCTest
@testable import NegsApp

/// Ticket 39: times opening never-edited photographs, warm. NEGS_OPEN_BENCH
/// is a colon-separated list of photographs; NEGS_OPEN_BENCH_MATCH=1 matches
/// brightness; NEGS_OPEN_BENCH_SCALE the canvas's screen pixels per photo
/// pixel (default 0.25); NEGS_OPEN_BENCH_ROUNDS (default 5). Each open is of
/// a fresh APFS clone (warm in the file cache, but a path nothing has seen),
/// alternating the files; the first round warms up and isn't counted. Prints
/// time to the first developed frame and to the settled one, and the time to
/// settle when going back to a photograph opened a moment ago.
@MainActor final class OpenTimingTests: XCTestCase {
    func testOpenTimings() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let list = environment["NEGS_OPEN_BENCH"] else { throw XCTSkip("Set NEGS_OPEN_BENCH") }
        let files = list.split(separator: ":").map { URL(fileURLWithPath: String($0)) }
        let match = environment["NEGS_OPEN_BENCH_MATCH"] == "1"
        let scale = environment["NEGS_OPEN_BENCH_SCALE"].flatMap(Double.init).map { CGFloat($0) } ?? 0.25
        let rounds = environment["NEGS_OPEN_BENCH_ROUNDS"].flatMap(Int.init) ?? 5
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-open-bench-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: "autoPlaceNewImages")
        defer { defaults.set(saved, forKey: "autoPlaceNewImages") }
        let model = EditorModel()
        defer { model.shutdown() }
        model.autoPlaceNewImages = match
        model.canvasScale.fixed = scale
        await model.start(paths: paths, restoration: Task { nil })

        func settled() -> Bool { model.error != nil || model.previewCurrent && !model.matchingBrightness && !model.brightnessMatchPending }
        func open(_ photo: URL) async throws -> (first: Double, settled: Double) {
            let clock = ContinuousClock.now
            model.open(photo)
            var first: Duration?
            let deadline = Date().addingTimeInterval(60)
            while !settled(), Date() < deadline {
                if first == nil, model.image != nil { first = ContinuousClock.now - clock }
                try await Task.sleep(for: .milliseconds(1))
            }
            let done = ContinuousClock.now - clock
            XCTAssertNil(model.error)
            // Thumbnails and previews that follow don't overlap the next open.
            try await Task.sleep(for: .milliseconds(800))
            return ((first ?? done) / .milliseconds(1), done / .milliseconds(1))
        }
        var results: [String: [(first: Double, settled: Double)]] = [:], back: [String: [Double]] = [:]
        for round in 0..<rounds {
            var opened: [URL] = []
            for file in files {
                let clone = directory.appendingPathComponent("\(round)-\(file.lastPathComponent)")
                try FileManager.default.copyItem(at: file, to: clone)
                opened.append(clone)
                let timing = try await open(clone)
                if round > 0 { results[file.lastPathComponent, default: []].append(timing) }
                print("OPEN-BENCH match=\(match) round=\(round) \(file.lastPathComponent) first=\(Int(timing.first)) settled=\(Int(timing.settled)) renders=\(model.renderLog.suffix(2)) status=\(model.status)")
            }
            // Back to the first: A → B → A.
            if files.count > 1 {
                let timing = try await open(opened[0])
                if round > 0 { back[files[0].lastPathComponent, default: []].append(timing.settled) }
                print("OPEN-BENCH match=\(match) round=\(round) back to \(files[0].lastPathComponent) settled=\(Int(timing.settled))")
            }
        }
        let median = { (values: [Double]) in values.sorted()[values.count / 2] }
        for (name, runs) in results.sorted(by: { $0.key < $1.key }) {
            print("OPEN-BENCH-RESULT match=\(match) \(name): first \(Int(median(runs.map(\.first)))) ms, settled \(Int(median(runs.map(\.settled)))) ms; runs \(runs.map { "\(Int($0.first))/\(Int($0.settled))" })")
        }
        for (name, runs) in back {
            print("OPEN-BENCH-RESULT match=\(match) back to \(name): settled \(Int(median(runs))) ms; runs \(runs.map { Int($0) })")
        }
    }
}
