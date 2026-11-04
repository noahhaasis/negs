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
        /// A ⌘-click: pins the average of the sample circle, which ⌘-scroll sizes.
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

/// The circle a ⌘-click averages, as a fraction of the photograph's shorter
/// side. Zero is the one pixel under the pointer.
enum SampleSize {
    static let initial = 0.02
    static let smallest = 0.002
    static let largest = 0.5

    /// One step of ⌘-scroll, in the direction ⌥-scroll zooms in. A wheel's
    /// notch moves as far as ten pixels of a trackpad's scroll.
    static func radius(from radius: Double, scrollDelta: Double, precise: Bool) -> Double {
        let step = precise ? scrollDelta : scrollDelta * 10
        guard step != 0 else { return radius }
        guard radius >= smallest else { return step < 0 ? smallest : 0 }
        let next = radius * exp(-step * 0.01)
        return next < smallest ? 0 : min(next, largest)
    }
}
