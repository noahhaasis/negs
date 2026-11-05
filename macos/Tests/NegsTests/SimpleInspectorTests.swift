import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

@MainActor final class SimpleInspectorTests: XCTestCase {
    private var savedMode: Any?

    override func setUp() async throws {
        savedMode = UserDefaults.standard.object(forKey: "inspectorMode")
        UserDefaults.standard.removeObject(forKey: "inspectorMode")
    }

    override func tearDown() async throws {
        UserDefaults.standard.set(savedMode, forKey: "inspectorMode")
    }

    /// Advanced until someone chooses otherwise, and remembered after that.
    func testTheModeIsAdvancedUntilChosenAndThenRemembered() {
        let first = EditorModel()
        defer { first.shutdown() }
        XCTAssertEqual(first.inspectorMode, .advanced)
        first.inspectorMode = .simple
        let second = EditorModel()
        defer { second.shutdown() }
        XCTAssertEqual(second.inspectorMode, .simple)
    }

    /// Simple is one page of the same sliders Advanced uses, in this order.
    func testSimpleShowsItsSlidersOnOnePage() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        model.inspectorMode = .simple
        let (host, window) = render(model)
        defer { window.orderOut(nil) }
        XCTAssertEqual(sliders(in: host).map { $0.accessibilityLabel() ?? "" }, [
            "Print exposure",
            "Brightness", "Contrast", "Whites", "Blacks", "Saturation",
            "Midtone Detail", "Unsharp radius (px)", "Unsharp amount",
        ])
    }

    /// Simple shares its ranges with Advanced, so a value is never clamped by
    /// switching modes.
    func testSimpleSlidersKeepAdvancedsRanges() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        model.inspectorMode = .simple
        let (host, window) = render(model)
        defer { window.orderOut(nil) }
        let found = Dictionary(uniqueKeysWithValues: sliders(in: host).map { ($0.accessibilityLabel() ?? "", $0) })
        XCTAssertEqual(found["Print exposure"]?.maxValue, 5)
        XCTAssertEqual(found["Midtone Detail"]?.minValue, -100)
        XCTAssertEqual(found["Unsharp radius (px)"]?.maxValue, 3)
        XCTAssertEqual(found["Unsharp amount"]?.maxValue, 2)
    }

    /// Advanced keeps showing one tab at a time, and follows the tab bar's choice.
    func testAdvancedStillShowsOneTabAtATime() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        model.inspectorMode = .advanced
        func labels(_ tab: InspectorTab) -> [String] {
            model.inspectorTab = tab
            let (host, window) = render(model)
            defer { window.orderOut(nil) }
            return sliders(in: host).map { $0.accessibilityLabel() ?? "" }
        }
        let camera = labels(.camera)
        XCTAssertTrue(camera.contains("Exposure (EV)"))
        XCTAssertFalse(camera.contains("Brightness"))
        let finish = labels(.finish)
        XCTAssertTrue(finish.contains("Brightness"))
        XCTAssertFalse(finish.contains("Exposure (EV)"))
    }

    /// With the film scanned directly there's no print, so its controls are
    /// off while the rest of the page still works. Slide films do this by
    /// themselves, and Simple has no switch for it.
    func testThePrintControlsTurnOffWhenThereIsNoPrint() async throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-simple-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        await model.start(paths: paths)
        XCTAssertTrue(model.hasCatalog, model.error ?? "No catalog")
        model.inspectorMode = .simple

        func enabled(_ scanning: Bool) throws -> (print: Bool, tone: Bool) {
            model.set("io.scan_film", .bool(scanning))
            let (host, window) = render(model)
            defer { window.orderOut(nil) }
            let found = sliders(in: host)
            let print = try XCTUnwrap(found.first { $0.accessibilityLabel() == "Print exposure" })
            let tone = try XCTUnwrap(found.first { $0.accessibilityLabel() == "Brightness" })
            return (print.isEnabled, tone.isEnabled)
        }
        let printing = try enabled(false)
        XCTAssertTrue(printing.print)
        XCTAssertTrue(printing.tone)
        let scanning = try enabled(true)
        XCTAssertFalse(scanning.print, "No print to expose")
        XCTAssertTrue(scanning.tone, "The rest of the page still works")
    }

    // MARK: Helpers

    /// The real inspector, in a window tall enough for its lazy form to build
    /// every row.
    private func render(_ model: EditorModel) -> (NSView, NSWindow) {
        let host = NSHostingView(rootView: AdjustmentInspector(model: model).background(Color(nsColor: .underPageBackgroundColor)))
        host.appearance = NSAppearance(named: .darkAqua)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 360, height: 2000), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.4))
        host.layoutSubtreeIfNeeded()
        if let folder = ProcessInfo.processInfo.environment["NEGS_INSPECTOR_LAYOUT_SNAPSHOTS"],
           let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) {
            host.cacheDisplay(in: host.bounds, to: bitmap)
            let name = "\(model.inspectorMode)-\(model.value("io.scan_film").bool ? "scan" : "print")-\(model.hasCatalog ? "catalog" : "empty").png"
            try? FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
            try? bitmap.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: folder).appendingPathComponent(name))
        }
        return (host, window)
    }

    private func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }

    /// The sliders on screen, from the top down.
    private func sliders(in host: NSView) -> [NSSlider] {
        descendants(host).compactMap { $0 as? NSSlider }
            .sorted { host.convert($0.bounds, from: $0).midY * (host.isFlipped ? 1 : -1) < host.convert($1.bounds, from: $1).midY * (host.isFlipped ? 1 : -1) }
    }
}
