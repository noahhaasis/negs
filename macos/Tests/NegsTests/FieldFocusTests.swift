import AppKit
import XCTest
@testable import NegsApp

@MainActor final class FieldFocusTests: XCTestCase {
    func testClickOutsideCommitsBeforeDeliveringTheClick() throws {
        let (window, field, target) = makeWindow()
        defer { window.orderOut(nil) }
        let edits = Edits()
        field.delegate = edits
        XCTAssertTrue(window.makeFirstResponder(field))
        edits.values.removeAll()
        let editor = try XCTUnwrap(field.currentEditor() as? NSTextView)
        editor.insertText("1.5", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        target.onClick = {
            XCTAssertNil(field.currentEditor())
            XCTAssertEqual(edits.values, ["1.5"])
        }

        try click(target, in: window)

        XCTAssertNil(field.currentEditor(), "Clicking a view that cannot take focus must still end editing")
        XCTAssertEqual(edits.values, ["1.5"])
        XCTAssertEqual(target.clicks, 1, "The click must also reach its original destination")
    }

    func testClickInsideKeepsEditingAndAnotherFieldCanTakeFocus() throws {
        let (window, field, _) = makeWindow()
        defer { window.orderOut(nil) }
        let edits = Edits()
        field.delegate = edits
        XCTAssertTrue(window.makeFirstResponder(field))
        edits.values.removeAll()
        let editor = try XCTUnwrap(field.currentEditor())

        try click(field, in: window)

        XCTAssertTrue(field.currentEditor() === editor)
        XCTAssertTrue(edits.values.isEmpty, "Moving the caret must not commit or dismiss the field")

        let next = NSTextField(string: "Next")
        next.frame = NSRect(x: 20, y: 80, width: 140, height: 24)
        window.contentView?.addSubview(next)
        try click(next, in: window)

        XCTAssertNil(field.currentEditor())
        XCTAssertNotNil(next.currentEditor(), "The outside click must still focus the next input")
        XCTAssertEqual(edits.values, ["Initial"])
    }

    private func makeWindow() -> (EditorWindow, NSTextField, ClickTarget) {
        _ = NSApplication.shared
        let window = EditorWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 200),
                                  styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let field = NSTextField(string: "Initial")
        field.frame = NSRect(x: 20, y: 140, width: 140, height: 24)
        let target = ClickTarget(frame: NSRect(x: 200, y: 20, width: 180, height: 160))
        window.contentView?.addSubview(field)
        window.contentView?.addSubview(target)
        window.makeKeyAndOrderFront(nil)
        return (window, field, target)
    }

    private func click(_ view: NSView, in window: NSWindow) throws {
        let point = view.convert(NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil)
        func event(_ type: NSEvent.EventType) throws -> NSEvent {
            try XCTUnwrap(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        }
        // Text selection can track the mouse until it receives a mouse-up.
        NSApp.postEvent(try event(.leftMouseUp), atStart: true)
        window.sendEvent(try event(.leftMouseDown))
        if let up = NSApp.nextEvent(matching: .leftMouseUp, until: .distantPast, inMode: .default, dequeue: true) {
            window.sendEvent(up)
        }
    }

    private final class Edits: NSObject, NSTextFieldDelegate {
        var values: [String] = []
        func controlTextDidEndEditing(_ notification: Notification) {
            if let field = notification.object as? NSTextField { values.append(field.stringValue) }
        }
    }

    private final class ClickTarget: NSView {
        var clicks = 0
        var onClick: (() -> Void)?
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        override func mouseDown(with event: NSEvent) {
            clicks += 1
            onClick?()
        }
    }
}
