import AppKit
import XCTest
@testable import NegsApp

/// The window can't be narrower than its columns' minimums: the canvas and
/// inspector together, or the canvas alone while the inspector is hidden.
@MainActor final class WindowMinimumTests: XCTestCase {
    private let frameKey = "NSWindow Frame editor"

    func testWindowKeepsRoomForBothColumns() async throws {
        _ = NSApplication.shared
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: frameKey)
        defer { defaults.set(saved, forKey: frameKey) }
        // A frame saved before the minimum existed, narrower than both columns.
        defaults.set("100 100 700 400 0 0 1920 1080 ", forKey: frameKey)

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-window-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        let split = try XCTUnwrap(window.contentViewController as? NSSplitViewController)
        let columns = split.splitViewItems.map(\.minimumThickness).reduce(0, +) + split.splitView.dividerThickness
        func content() -> NSSize { window.contentRect(forFrameRect: window.frame).size }

        XCTAssertEqual(window.contentMinSize, NSSize(width: columns, height: 560))
        XCTAssertGreaterThanOrEqual(content().width, columns, "A too-small saved frame grows to fit both columns")
        XCTAssertGreaterThanOrEqual(content().height, 560)

        model.inspectorVisible = false
        try await waitUntil { window.contentMinSize.width == split.splitViewItems[0].minimumThickness }
        window.setContentSize(NSSize(width: 700, height: 600))

        model.inspectorVisible = true
        try await waitUntil { window.contentMinSize.width == columns }
        XCTAssertGreaterThanOrEqual(content().width, columns, "Showing the inspector makes room for it")
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        for _ in 0..<100 where !predicate() { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate())
    }
}
