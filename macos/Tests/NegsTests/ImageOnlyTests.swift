import AppKit
import XCTest
@testable import NegsApp

/// View › Image Only (F) hides the strip, the status line and the inspector
/// without changing their own settings, and Esc leaves it before opening the
/// grid. Holding T turns the photograph upside down until it's let go or the
/// window loses the keys.
extension CALayer {
    /// Every layer under this one, however deep: the photograph sits under
    /// the turn layer and the crop.
    var descendants: [CALayer] { (sublayers ?? []).flatMap { [$0] + $0.descendants } }
}

@MainActor final class ImageOnlyTests: XCTestCase {
    private let frameKey = "NSWindow Frame editor"

    /// A model with a small photograph open, with the strip's setting put back afterwards.
    private func model() throws -> EditorModel {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-image-only-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        for name in ["DSC_0001.png", "DSC_0002.png"] {
            let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 48, pixelsHigh: 32, bitsPerSample: 8, samplesPerPixel: 3,
                                                        hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 48 * 3, bitsPerPixel: 24))
            try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: directory.appendingPathComponent(name))
        }
        let model = EditorModel(sidecars: SidecarStore())
        // Both are remembered in the defaults.
        let strip = model.filmstripVisible, pane = model.inspectorPane
        addTeardownBlock { @MainActor in model.filmstripVisible = strip; model.inspectorPane = pane; model.shutdown() }
        model.inspectorPane = .adjustments
        model.open(directory.appendingPathComponent("DSC_0001.png"))
        XCTAssertNotNil(model.source)
        return model
    }

    private func window(_ model: EditorModel) throws -> (EditorWindowController, NSWindow) {
        _ = NSApplication.shared
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: frameKey)
        addTeardownBlock { defaults.set(saved, forKey: self.frameKey) }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.editorFrame.split.splitView.autosaveName = nil
        window.setFrameAutosaveName("")
        window.setContentSize(NSSize(width: 1200, height: 800))
        window.orderFront(nil)
        addTeardownBlock { @MainActor in window.orderOut(nil) }
        controller.refresh()
        return (controller, window)
    }

    private func key(_ type: NSEvent.EventType, _ characters: String, _ code: UInt16, in window: NSWindow,
                     modifiers: NSEvent.ModifierFlags = [], repeating: Bool = false) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: type, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: window.windowNumber,
                                       context: nil, characters: characters, charactersIgnoringModifiers: characters, isARepeat: repeating, keyCode: code))
    }

    func testImageOnlyHidesThePanelsAndRestoresThem() throws {
        let model = try model()
        for strip in [true, false] {
            model.filmstripVisible = strip
            model.toggleImageOnly()
            XCTAssertTrue(model.imageOnly)
            XCTAssertFalse(model.stripShown)
            XCTAssertFalse(model.inspectorShown)
            XCTAssertEqual(model.filmstripVisible, strip, "the strip's own setting stays")
            XCTAssertTrue(model.inspectorVisible, "the inspector's own setting stays")
            model.toggleImageOnly()
            XCTAssertFalse(model.imageOnly)
            XCTAssertEqual(model.stripShown, strip, "comes back as it was")
            XCTAssertTrue(model.inspectorShown)
        }

        // A strip hidden with ⌥⌘F stays hidden, and so does the inspector hidden with ⌥⌘I.
        model.filmstripVisible = false
        model.togglePane(.adjustments)
        XCTAssertFalse(model.inspectorVisible)
        model.toggleImageOnly()
        model.toggleImageOnly()
        XCTAssertFalse(model.stripShown)
        XCTAssertFalse(model.inspectorShown)

        // ⌥⌘F and ⌥⌘I in Image Only end it and show what was asked for.
        model.toggleImageOnly()
        model.toggleFilmstrip()
        XCTAssertFalse(model.imageOnly)
        XCTAssertTrue(model.stripShown)
        XCTAssertFalse(model.inspectorShown)
        model.toggleImageOnly()
        model.togglePane(.presets)
        XCTAssertFalse(model.imageOnly)
        XCTAssertTrue(model.inspectorShown)
        XCTAssertEqual(model.inspectorPane, .presets)
    }

    func testTheGridEndsImageOnlyAndFOpensItsPhotographInImageOnly() throws {
        let model = try model()
        model.toggleImageOnly()
        model.showGrid()
        XCTAssertEqual(model.mode, .grid)
        XCTAssertFalse(model.imageOnly, "the grid grows out of the strip as set")
        model.toggleImageOnly()
        XCTAssertEqual(model.mode, .photo)
        XCTAssertTrue(model.imageOnly)
        // Stepping to another photograph keeps it.
        model.open(try XCTUnwrap(model.source).deletingLastPathComponent().appendingPathComponent("DSC_0002.png"))
        XCTAssertTrue(model.imageOnly)
    }

    /// The inspector collapses in Image Only and comes back, and the
    /// collapse doesn't hide the adjustments for good.
    func testTheWindowHidesTheInspectorOnlyWhileImageOnly() throws {
        let model = try model()
        let (controller, _) = try window(model)
        let inspector = controller.editorFrame.split.splitViewItems[1]
        XCTAssertFalse(inspector.isCollapsed)
        model.toggleImageOnly()
        controller.refresh()
        XCTAssertTrue(inspector.isCollapsed)
        XCTAssertTrue(model.inspectorVisible)
        XCTAssertTrue(model.imageOnly)
        model.toggleImageOnly()
        controller.refresh()
        XCTAssertFalse(inspector.isCollapsed)
        XCTAssertTrue(model.inspectorShown)
    }

    func testEscapeLeavesImageOnlyBeforeOpeningTheGrid() throws {
        let model = try model()
        let (controller, window) = try window(model)
        model.toggleImageOnly()
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "\u{1b}", 53, in: window)))
        XCTAssertFalse(model.imageOnly)
        XCTAssertEqual(model.mode, .photo)
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "\u{1b}", 53, in: window)))
        XCTAssertEqual(model.mode, .grid)
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "\u{1b}", 53, in: window)))
        XCTAssertEqual(model.mode, .photo)
    }

    func testHoldingTTurnsThePhotographUntilLetGo() throws {
        let model = try model()
        let (controller, window) = try window(model)
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "t", 17, in: window)))
        XCTAssertTrue(model.upsideDown)
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "t", 17, in: window, repeating: true)))
        XCTAssertTrue(model.upsideDown)
        XCTAssertTrue(controller.handleKey(try key(.keyUp, "t", 17, in: window)))
        XCTAssertFalse(model.upsideDown)

        // With \ held too, and in Image Only.
        model.toggleImageOnly()
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "\\", 42, in: window)))
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "t", 17, in: window)))
        XCTAssertTrue(model.upsideDown)
        XCTAssertTrue(controller.handleKey(try key(.keyUp, "\\", 42, in: window)))
        XCTAssertTrue(model.upsideDown, "letting go of \\ keeps it turned")
        XCTAssertTrue(controller.handleKey(try key(.keyUp, "t", 17, in: window)))
        XCTAssertFalse(model.upsideDown)

        // A key-up missed in another window: losing the keys turns it back.
        XCTAssertTrue(controller.handleKey(try key(.keyDown, "t", 17, in: window)))
        controller.releaseHeldKeys()
        XCTAssertFalse(model.upsideDown)
        XCTAssertFalse(controller.handleKey(try key(.keyUp, "t", 17, in: window)), "nothing left held")

        // ⌘T and the grid leave T alone; so does typing.
        XCTAssertFalse(controller.handleKey(try key(.keyDown, "t", 17, in: window, modifiers: .command)))
        XCTAssertFalse(model.upsideDown)
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 100, height: 22))
        window.contentView?.addSubview(field)
        defer { field.removeFromSuperview() }
        XCTAssertTrue(window.makeFirstResponder(field))
        XCTAssertFalse(controller.handleKey(try key(.keyDown, "t", 17, in: window)))
        XCTAssertFalse(model.upsideDown)
        window.makeFirstResponder(nil)
        model.leaveImageOnly()
        model.showGrid()
        XCTAssertFalse(controller.handleKey(try key(.keyDown, "t", 17, in: window)))
        XCTAssertFalse(model.upsideDown)
        model.holdUpsideDown(true)
        XCTAssertFalse(model.upsideDown, "photograph view only")
    }

    /// Only the layers holding the print turn, about the middle of what's on
    /// screen; the view's frame, and so the caption and label, stay.
    func testTheCanvasTurnsOnlyItsLayers() throws {
        let view = PhotographView(frame: NSRect(x: 0, y: 0, width: 300, height: 200))
        view.layout()
        let turn = try XCTUnwrap(view.layer?.sublayers?.first)
        XCTAssertEqual(turn.affineTransform(), .identity)
        view.turned = true
        XCTAssertEqual(turn.affineTransform().a, -1, accuracy: 1e-9)
        XCTAssertEqual(turn.affineTransform().d, -1, accuracy: 1e-9)
        for (value, expected) in [(turn.frame.minX, 0.0), (turn.frame.minY, 0), (turn.frame.width, 300), (turn.frame.height, 200)] {
            XCTAssertEqual(value, expected, accuracy: 1e-6, "turned in place")
        }
        XCTAssertEqual(view.frame, NSRect(x: 0, y: 0, width: 300, height: 200))
        view.turned = false
        XCTAssertEqual(turn.affineTransform(), .identity)

        let bounds = CGRect(x: 0, y: 0, width: 300, height: 200)
        XCTAssertEqual(PhotographView.turnCentre(visible: nil, bounds: bounds), CGPoint(x: 150, y: 100))
        XCTAssertEqual(PhotographView.turnCentre(visible: CGRect(x: -50, y: -50, width: 400, height: 300), bounds: bounds),
                       CGPoint(x: 150, y: 100), "fitted: the whole print")
        XCTAssertEqual(PhotographView.turnCentre(visible: CGRect(x: 200, y: 20, width: 60, height: 40), bounds: bounds),
                       CGPoint(x: 230, y: 40), "zoomed in: the part on screen")
    }
}
