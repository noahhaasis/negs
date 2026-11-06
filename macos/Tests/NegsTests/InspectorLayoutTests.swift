import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

@MainActor final class InspectorLayoutTests: XCTestCase {
    /// The sidebar is black, with a 1-point amber edge on its left.
    func testTheSidebarMatchesTheTitleBarAndHasALeftEdge() throws {
        _ = NSApplication.shared
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: ViewerBackground.customKey)
        defaults.set(false, forKey: ViewerBackground.customKey)
        defer { defaults.set(saved, forKey: ViewerBackground.customKey) }
        let model = EditorModel()
        defer { model.shutdown() }
        let host = NSHostingView(rootView: InspectorPanel(model: model, built: false))
        host.appearance = NSAppearance(named: .darkAqua)
        host.frame = NSRect(x: 0, y: 0, width: 100, height: 100)
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        func grey(_ x: Int) throws -> Int {
            let colour = try XCTUnwrap(bitmap.colorAt(x: x * bitmap.pixelsWide / 100, y: bitmap.pixelsHigh / 2)?.usingColorSpace(.sRGB))
            XCTAssertEqual(colour.alphaComponent, 1, accuracy: 0.01, "at \(x)")
            return Int((colour.redComponent * 255).rounded())
        }
        XCTAssertEqual(try grey(50), 0, accuracy: 2, "Black")
        XCTAssertGreaterThan(try grey(0), try grey(50) + 8, "The left edge is lighter")
        XCTAssertEqual(try grey(2), try grey(50), accuracy: 1, "It's only one point wide")
    }

    /// In a proof, the viewer's background is white, and the
    /// saved colour comes back when it ends.
    func testTheViewerIsWhiteInAProof() throws {
        _ = NSApplication.shared
        let defaults = UserDefaults.standard
        let saved = (defaults.object(forKey: "proofMode"), defaults.object(forKey: ViewerBackground.customKey))
        defaults.set(false, forKey: ViewerBackground.customKey)
        defer {
            defaults.set(saved.0, forKey: "proofMode")
            defaults.set(saved.1, forKey: ViewerBackground.customKey)
        }
        let model = EditorModel()
        defer { model.shutdown() }
        func background() throws -> Int {
            let host = NSHostingView(rootView: CanvasArea(model: model))
            host.appearance = NSAppearance(named: .darkAqua)
            host.frame = NSRect(x: 0, y: 0, width: 800, height: 800)
            host.layoutSubtreeIfNeeded()
            let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
            host.cacheDisplay(in: host.bounds, to: bitmap)
            let colour = try XCTUnwrap(bitmap.colorAt(x: 8, y: bitmap.pixelsHigh - 8)?.usingColorSpace(.sRGB))
            return Int((colour.redComponent * 255).rounded())
        }
        model.proofing = false
        XCTAssertEqual(try background(), 0, accuracy: 2)
        model.proofing = true
        XCTAssertEqual(try background(), 255)
        model.proofing = false
        XCTAssertEqual(try background(), 0, accuracy: 2)
    }

    /// A custom viewer colour is for the viewer: the sidebar's light text would
    /// vanish on a white one.
    func testACustomViewerColourLeavesTheSidebarAlone() throws {
        _ = NSApplication.shared
        let defaults = UserDefaults.standard
        let saved = (defaults.object(forKey: ViewerBackground.customKey), defaults.object(forKey: ViewerBackground.colorKey))
        defaults.set(true, forKey: ViewerBackground.customKey)
        defaults.set("#FFFFFF", forKey: ViewerBackground.colorKey)
        defer {
            defaults.set(saved.0, forKey: ViewerBackground.customKey)
            defaults.set(saved.1, forKey: ViewerBackground.colorKey)
        }
        let model = EditorModel()
        defer { model.shutdown() }
        let host = NSHostingView(rootView: InspectorPanel(model: model, built: false))
        host.appearance = NSAppearance(named: .darkAqua)
        host.frame = NSRect(x: 0, y: 0, width: 100, height: 100)
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let colour = try XCTUnwrap(bitmap.colorAt(x: bitmap.pixelsWide / 2, y: bitmap.pixelsHigh / 2)?.usingColorSpace(.sRGB))
        XCTAssertEqual(colour.redComponent * 255, 0, accuracy: 2)
    }

    /// The line under the toolbar runs across the canvas and the sidebar, in the
    /// colour of the sidebar's edge.
    func testALineRunsUnderTheToolbar() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        func rows<V: View>(_ view: V) throws -> (top: Int, below: Int) {
            let host = NSHostingView(rootView: view)
            host.appearance = NSAppearance(named: .darkAqua)
            host.frame = NSRect(x: 0, y: 0, width: 800, height: 800)
            host.layoutSubtreeIfNeeded()
            let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
            host.cacheDisplay(in: host.bounds, to: bitmap)
            func grey(_ y: Int) throws -> Int {
                let colour = try XCTUnwrap(bitmap.colorAt(x: bitmap.pixelsWide / 2, y: y * bitmap.pixelsHigh / 800)?.usingColorSpace(.sRGB))
                return Int((colour.redComponent * 255).rounded())
            }
            return (try grey(0), try grey(4))
        }
        let sidebar = try rows(InspectorPanel(model: model, built: false))
        XCTAssertGreaterThan(sidebar.top, sidebar.below + 8, "Over the sidebar")
        let canvas = try rows(EditorCanvas(model: model))
        XCTAssertGreaterThan(canvas.top, canvas.below + 8, "Over the canvas")
        XCTAssertEqual(canvas.top, sidebar.top, accuracy: 2, "The same colour across both")
    }

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
        let saved = ["expandedAdjustments", "imageSizeLongestSide", "inspectorPane"].map { ($0, defaults.object(forKey: $0)) }
        defaults.set("Enlarger", forKey: "expandedAdjustments")
        // A previous Presets pane leaves the inspector without its form.
        defaults.set("adjustments", forKey: "inspectorPane")
        defer { for (key, value) in saved { defaults.set(value, forKey: key) } }

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-layout-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        await model.start(paths: paths)
        XCTAssertTrue(model.ready, model.error ?? "No engine")
        // The Enlarger controls live on Print now; keep them on screen.
        model.inspectorTab = .print
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

        // A pin fills the zone caption and draws on the canvas; both reserve
        // their space already.
        await model.placeZone(5, at: CGPoint(x: 0.5, y: 0.5))
        try await waitUntil { model.pins.count == 1 }
        try await Task.sleep(for: .milliseconds(200))
        try capture("pick")
        XCTAssertEqual(document.frame.height, initialHeight, accuracy: 1, "A pin must not resize the inspector form")
        XCTAssertEqual(sliderY(), interactiveY, accuracy: 1, "A pin must keep sliders under the pointer")
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(predicate(), "Timed out waiting for image engine")
    }
}

/// Amber on black, from the window's edges in.
@MainActor final class ThemeTests: XCTestCase {
    func testTheWindowIsBlackEdgeToEdge() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        XCTAssertEqual(window.backgroundColor, .black)
        XCTAssertTrue(window.titlebarAppearsTransparent)
        XCTAssertEqual(window.appearance?.name, .darkAqua)
    }

    /// Full screen, the title comes off the black toolbar and the status bar carries it.
    func testFullScreenMovesTheTitleToTheStatusBar() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        XCTAssertEqual(window.titleVisibility, .visible)
        XCTAssertNil(model.fullScreenCaption)
        NotificationCenter.default.post(name: NSWindow.willEnterFullScreenNotification, object: window)
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(window.titleVisibility, .hidden)
        XCTAssertTrue(model.windowFullScreen)
        NotificationCenter.default.post(name: NSWindow.willExitFullScreenNotification, object: window)
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(window.titleVisibility, .visible)
        XCTAssertFalse(model.windowFullScreen)
    }

    func testTheDefaultViewerIsBlackAndACustomColourStays() {
        XCTAssertEqual(NSColor(ViewerBackground.canvas(custom: false, hex: "#FFFFFF")).usingColorSpace(.sRGB)?.redComponent, 0)
        XCTAssertEqual(NSColor(ViewerBackground.canvas(custom: true, hex: "#FFFFFF")).usingColorSpace(.sRGB)?.redComponent, 1)
    }

    func testSlidersAreAmber() {
        let view = NSHostingView(rootView: ResettableSlider(title: "T", value: .constant(1), range: 0...2, editing: { _ in }, reset: {}))
        view.frame = NSRect(x: 0, y: 0, width: 200, height: 30)
        view.layoutSubtreeIfNeeded()
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        XCTAssertEqual(all(view).compactMap { $0 as? NSSlider }.first?.trackFillColor, EdgePrint.colour)
    }
}
