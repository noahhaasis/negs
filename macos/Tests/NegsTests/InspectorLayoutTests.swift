import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

@MainActor final class InspectorLayoutTests: XCTestCase {
    func testInspectorDoesNotMoveWhenInteractivePreviewFinishes() async throws {
        try await checkInspectorLayout(limited: false)
    }

    func testLimitedPreviewNoticeStaysStableDuringSliderInteraction() async throws {
        try await checkInspectorLayout(limited: true)
    }

    /// Exercise the real form and helper through a reduced drag frame and
    /// the settled render. The Image Size row is above the visible controls.
    private func checkInspectorLayout(limited: Bool) async throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = ["expandedAdjustments", "inspectorTab", "imageSizeLongestSide"].map { ($0, defaults.object(forKey: $0)) }
        defaults.set("Enlarger", forKey: "expandedAdjustments")
        defaults.set("Lab", forKey: "inspectorTab")
        defer { for (key, value) in saved { defaults.set(value, forKey: key) } }

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-layout-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        await model.start(paths: paths)
        XCTAssertTrue(model.ready, model.error ?? "No engine")
        model.set("film_render.grain.active", .bool(false))
        model.set("film_render.halation.active", .bool(false))
        model.limitPreview(limited)
        model.setLongestSide(limited ? 4800 : 2400)
        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64,
            bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24))
        memset(bitmap.bitmapData!, 120, bitmap.bytesPerRow * bitmap.pixelsHigh)
        let fixture = directory.appendingPathComponent("fixture.png")
        try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: fixture)
        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        let settledScale = model.previewScale
        if limited { XCTAssertLessThan(settledScale, 1) } else { XCTAssertEqual(settledScale, 1) }

        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        window.setContentSize(NSSize(width: 1100, height: 860))
        let split = try XCTUnwrap(window.contentViewController as? NSSplitViewController)
        let panel = try XCTUnwrap(split.splitViewItems[1].viewController as? NSHostingController<InspectorPanel>)
        // Populate the inspector even when the test window is occluded.
        panel.rootView = InspectorPanel(model: model)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        try await Task.sleep(for: .milliseconds(700))
        let inspector = split.splitViewItems[1].viewController.view
        func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
        let scroll = try XCTUnwrap(descendants(inspector).compactMap { $0 as? NSScrollView }.first)
        let document = try XCTUnwrap(scroll.documentView)
        // Only Enlarger is expanded; its last slider is Preflash yellow.
        let slider = try XCTUnwrap(descendants(inspector).compactMap { $0 as? NSSlider }.last)
        func sliderY() -> CGFloat { slider.convert(slider.bounds, to: inspector).midY }
        func capture(_ name: String) throws {
            guard let folder = ProcessInfo.processInfo.environment["NEGS_INSPECTOR_LAYOUT_SNAPSHOTS"] else { return }
            try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
            let bitmap = try XCTUnwrap(inspector.bitmapImageRepForCachingDisplay(in: inspector.bounds))
            inspector.cacheDisplay(in: inspector.bounds, to: bitmap)
            try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
                .write(to: URL(fileURLWithPath: folder).appendingPathComponent("\(limited ? "limited" : "full")-\(name).png"))
        }
        let origin = max(0, document.frame.height - scroll.contentSize.height - 150)
        scroll.contentView.scroll(to: NSPoint(x: 0, y: origin))
        scroll.reflectScrolledClipView(scroll.contentView)
        try await Task.sleep(for: .milliseconds(100))
        let initialHeight = document.frame.height
        let initialOrigin = scroll.contentView.bounds.origin.y
        let initialY = sliderY()
        try capture("before")

        model.beginInteraction()
        model.set("enlarger.m_filter_shift", .number(-5))
        try await waitUntil { model.status.hasPrefix("Preview ·") || model.error != nil }
        XCTAssertNil(model.error)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(model.previewScale, settledScale, "The notice describes the settled preview, independent of drag resolution")
        let interactiveY = sliderY()
        try capture("drag")
        XCTAssertEqual(document.frame.height, initialHeight, accuracy: 1, "A preview update must not resize the inspector form")
        XCTAssertEqual(interactiveY, initialY, accuracy: 1, "A preview update must keep sliders under the pointer")

        model.endInteraction()
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        try await Task.sleep(for: .milliseconds(300))
        try capture("release")
        XCTAssertEqual(model.previewScale, settledScale)
        XCTAssertEqual(document.frame.height, initialHeight, accuracy: 1)
        XCTAssertEqual(scroll.contentView.bounds.origin.y, initialOrigin, accuracy: 1, "Releasing a slider must keep the inspector's scroll position")
        XCTAssertEqual(sliderY(), interactiveY, accuracy: 1, "Releasing a slider must keep the slider in place")
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(predicate(), "Timed out waiting for image engine")
    }
}
