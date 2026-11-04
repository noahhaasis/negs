import AppKit

/// What a mouse-down on the photograph starts. Pure, so the map from clicks
/// and modifiers to a gesture is testable without events.
///
/// A double-click zooms, ⌘ samples a tone into a pin, clicking a pin's chip
/// selects it, and everything else pans.
struct CanvasGesture {
    enum Start: Equatable {
        /// Toggle between fit and 1:1, about the click.
        case zoomToggle
        /// Scroll, only when the view has somewhere to go.
        case pan
        /// A ⌘-click, or a ⌘-drag that grows a sample circle from the point.
        case pin
        /// A click on a pin's chip selects it: its band lights up, and a band
        /// click aims it there. A drag from the chip moves it.
        case selectPin(Int)
    }

    static func start(
        clickCount: Int,
        modifiers: NSEvent.ModifierFlags,
        pinUnderPointer: Int?
    ) -> Start {
        if clickCount == 2 { return .zoomToggle }
        if let pin = pinUnderPointer, !modifiers.contains(.command) { return .selectPin(pin) }
        if modifiers.contains(.command) { return .pin }
        return .pan
    }
}
