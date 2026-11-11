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

    /// A tab is as big chosen (700) as not (500), so choosing one moves
    /// none of the others, and as tall with an edit count as without, so a
    /// count never moves its title up or down. The count itself takes room
    /// in the row: the gap to the next tab starts after it.
    func testTabWordsKeepTheirSizeWhenChosenOrEdited() {
        _ = NSApplication.shared
        func size(_ title: String, chosen: Bool, count: Int) -> CGSize {
            NSHostingView(rootView: TabWord(title: title, chosen: chosen, count: count) {}).fittingSize
        }
        for tab in InspectorTab.allCases {
            let title = tab.rawValue, plain = size(title, chosen: false, count: 0)
            XCTAssertGreaterThan(plain.width, 0)
            for count in [0, 1, 12] {
                XCTAssertEqual(size(title, chosen: true, count: count), size(title, chosen: false, count: count), "\(title), \(count) edits")
                XCTAssertEqual(size(title, chosen: false, count: count).height, plain.height, "\(title), \(count) edits")
            }
            XCTAssertGreaterThan(size(title, chosen: false, count: 1).width, plain.width, "\(title): the count is in the flow")
            // The tracking evens the two weights' widths out exactly.
            let gaps = CGFloat(title.count - 1)
            XCTAssertEqual(TabWord.width(title, weight: 700) + gaps * TabWord.tracking(title, chosen: true),
                           TabWord.width(title, weight: 500) + gaps * TabWord.tracking(title, chosen: false), accuracy: 0.001, title)
        }
    }

    /// The tab row as drawn, in every state: the same gap between each word
    /// (from its last ink, its count's if it has one, to the next title),
    /// titles that don't move when another tab is chosen, one baseline, and
    /// the capitals centred between the rules.
    func testTabRowGapsAndBaselineAsDrawn() throws {
        _ = NSApplication.shared
        XCTAssertTrue(Theme.Fonts.load(), "Newsreader")
        let states: [[InspectorTab: Int]] = [[:], [.prep: 1], [.lab: 1, .output: 1], [.prep: 3, .lab: 12, .output: 1]]
        for counts in states {
            var lefts: [[Int]] = []
            for chosen in InspectorTab.allCases {
                let row = try TabRowPixels(counts: counts, chosen: chosen)
                let label = "\(counts.map { "\($0.key.rawValue) \($0.value)" }.sorted()), \(chosen.rawValue) chosen"
                XCTAssertEqual(row.words.count, 3, label)
                guard row.words.count == 3 else { continue }
                // Glyphs' own side bearings differ by up to a point.
                let gaps = zip(row.words, row.words.dropFirst()).map { $1.title.lowerBound - $0.end }
                XCTAssertLessThanOrEqual(abs(gaps[0] - gaps[1]), row.pixels(1.5), "\(label): gaps \(gaps)")
                XCTAssertEqual(Set(row.words.map(\.baseline)).count, 1, "\(label): baselines \(row.words.map(\.baseline))")
                // P and L: the room over the capitals and under the baseline.
                for word in row.words.prefix(2) {
                    XCTAssertLessThanOrEqual(abs(word.room.above - word.room.below), row.pixels(1), "\(label): \(word.room)")
                }
                lefts.append(row.words.map(\.title.lowerBound))
            }
            for other in lefts.dropFirst() {
                for (a, b) in zip(lefts[0], other) { XCTAssertLessThanOrEqual(abs(a - b), 1, "\(counts): titles move when another is chosen, \(lefts)") }
            }
        }
    }

    /// The slider row's columns are fitted, not generous (round 26): every
    /// label leaves 8 pt before the track, and every value the slider can
    /// show, in bold and in the reader's locale, fits the value column. A
    /// new longer label or a wider range fails here, not as a cut label.
    func testSliderColumnsFitEveryLabelAndValue() {
        _ = NSApplication.shared
        XCTAssertTrue(Theme.Fonts.load(), "Newsreader")
        struct Spec { let title: String, range: ClosedRange<Double>, step: Double; var unit = "" }
        let controls = prepToneControls + prepColorControls + prepDetailControls + adjustmentGroups.flatMap(\.controls)
        var specs: [Spec] = controls.flatMap { control -> [Spec] in
            switch control {
            case .slider(let title, _, let range, let step): return [Spec(title: title, range: range, step: step)]
            case .stockSlider(let title, _, _, let range, let step, let bw):
                return [title, bw].compactMap { $0 }.map { Spec(title: $0, range: range, step: step) }
            default: return []
            }
        }
        // The rows built by hand. Exposure's range moves with the matched EV,
        // within ±3, so ±6 covers it.
        specs += [Spec(title: "Exposure (EV)", range: -6...6, step: 0.05),
                  Spec(title: "Angle", range: -CropSettings.maxAngle...CropSettings.maxAngle, step: 0.1, unit: "°"),
                  Spec(title: "Width", range: 0...15, step: 0.1, unit: "%"),
                  Spec(title: "Line width", range: 0.05...2, step: 0.05, unit: "%")]
        XCTAssertGreaterThan(specs.count, 60)

        let labelFont = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        let valueFont = Theme.serifFont(14.5, weight: 700) as NSFont
        func width(_ text: String, _ font: NSFont) -> CGFloat { (text as NSString).size(withAttributes: [.font: font]).width }
        for spec in specs {
            XCTAssertLessThanOrEqual(width(spec.title, labelFont) + 8, Theme.Size.sliderLabel, spec.title)
            let format = FloatingPointFormatStyle<Double>.number.precision(.fractionLength(0...LabeledSlider.decimals(spec.step)))
            let (low, high) = (spec.range.lowerBound, spec.range.upperBound)
            for value in [low, low + spec.step, high - spec.step, high] {
                let text = "\(value.formatted(format))\(spec.unit)"
                XCTAssertLessThanOrEqual(width(text, valueFont), Theme.Size.valueWidth, "\(spec.title): \(text)")
            }
        }
    }

    /// Exercise the real form and helper through a reduced drag frame and
    /// the settled render.
    private func checkInspectorLayout(limited: Bool) async throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = ["expandedAdjustments", "inspectorTab", "inspectorPane", "imageSizeLongestSide", "limitPreview"].map { ($0, defaults.object(forKey: $0)) }
        defaults.set("Enlarger", forKey: "expandedAdjustments")
        defaults.set("Lab", forKey: "inspectorTab")
        // The Presets pane, if a run left it chosen, has no form to scroll.
        defaults.set(InspectorPane.adjustments.rawValue, forKey: "inspectorPane")
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
        model.limitsPreview = limited
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
        let split = try XCTUnwrap((window.contentViewController as? EditorFrame)?.split)
        let panel = try XCTUnwrap(split.splitViewItems[1].viewController as? NSHostingController<InspectorPanel>)
        // Populate the inspector even when the test window is occluded.
        panel.rootView = InspectorPanel(model: model)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        try await Task.sleep(for: .milliseconds(700))
        let inspector = split.splitViewItems[1].viewController.view
        func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
        // The tab's scrolling sections, below the pinned scope, stocks and tabs.
        let scroll = try XCTUnwrap(descendants(inspector).compactMap { $0 as? NSScrollView }.max { $0.frame.height < $1.frame.height })
        let document = try XCTUnwrap(scroll.documentView)
        // Only Enlarger is expanded; its last slider is Preflash yellow.
        let slider = try XCTUnwrap(descendants(inspector).compactMap { $0 as? InkSliderView }.last)
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

/// The tab row as AppKit draws it, read back as ink: each word's title
/// columns, its last ink (the count's, if any), its baseline, and the room
/// over its first capital and under the baseline, inside the rules. In
/// pixels; `scale` is pixels per point.
@MainActor private struct TabRowPixels {
    struct Word { var title: ClosedRange<Int>; var end: Int; var baseline: Int; var room: (above: Int, below: Int) }
    var words: [Word] = []
    var scale: CGFloat = 1

    init(counts: [InspectorTab: Int], chosen: InspectorTab) throws {
        let host = NSHostingView(rootView: TabRow(counts: counts, tab: .constant(chosen)).environment(\.colorScheme, .dark))
        host.frame = NSRect(x: 0, y: 0, width: 340, height: Theme.Size.tabRow)
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        scale = CGFloat(bitmap.pixelsWide) / host.bounds.width
        let width = bitmap.pixelsWide, height = bitmap.pixelsHigh
        func color(_ x: Int, _ y: Int) -> NSColor? { bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) }
        // The rules: what's drawn at the right end, past the words.
        let ruled = (0..<height).filter { (color(width - 2, $0)?.alphaComponent ?? 0) > 0.2 }
        let top = try XCTUnwrap(ruled.filter { $0 < height / 2 }.max()), bottom = try XCTUnwrap(ruled.filter { $0 > height / 2 }.min())
        func ink(_ x: Int, _ y: Int) -> Bool {
            guard let c = color(x, y), c.alphaComponent > 0.5 else { return false }
            return (c.redComponent + c.greenComponent + c.blueComponent) / 3 > 0.3
        }
        // Rows down to just under the baseline, leaving out the pencil
        // under the chosen tab and the p's descender.
        let font = Theme.serifFont(TabWord.size, weight: 700)
        let limit = Int(((TabWord.topInset(row: Theme.Size.tabRow) + CTFontGetAscent(font) + 2) * scale).rounded())
        let rows = (top + 1)..<min(limit, bottom)
        let columns = (0..<(width - 4)).map { x in rows.contains { ink(x, $0) } }
        func runs(_ range: Range<Int>, gap: Int) -> [ClosedRange<Int>] {
            var out: [ClosedRange<Int>] = [], x = range.lowerBound
            while x < range.upperBound {
                guard columns[x] else { x += 1; continue }
                var end = x, y = x + 1
                while y < range.upperBound, y - end <= gap { if columns[y] { end = y }; y += 1 }
                out.append(x...end); x = end + 1
            }
            return out
        }
        // Words are 24 pt apart; a title's letters, and its dot, lie closer.
        for word in runs(0..<(width - 4), gap: Int(15 * scale)) {
            let title = runs(word.lowerBound..<(word.upperBound + 1), gap: Int(4 * scale))[0]
            var bottoms: [Int: Int] = [:]
            for x in title { if let b = rows.last(where: { ink(x, $0) }) { bottoms[b, default: 0] += 1 } }
            let baseline = bottoms.max { $0.value < $1.value }?.key ?? 0
            let capTop = (title.lowerBound + 1...title.lowerBound + Int(1.5 * scale)).compactMap { x in rows.first { ink(x, $0) } }.min() ?? 0
            words.append(Word(title: title, end: word.upperBound, baseline: baseline, room: (capTop - top - 1, bottom - baseline - 1)))
        }
    }
    /// A tolerance in points, as whole pixels (one at least).
    func pixels(_ points: CGFloat) -> Int { max(1, Int((points * scale).rounded())) }
}
