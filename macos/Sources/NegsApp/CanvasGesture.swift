import AppKit

/// What a mouse-down on the photograph starts. Pure, so the map from clicks
/// to gestures is testable without events.
struct CanvasGesture {
    enum Start: Equatable {
        /// Toggle between fit and 1:1, about the click.
        case zoomToggle
        /// Scroll when there's somewhere to go; without a move it's a click
        /// on empty photograph.
        case pan
        /// A press on a pin's chip selects it, and a drag from there moves it.
        case selectPin(Int)
    }

    static func start(clickCount: Int, pinUnderPointer: Int?) -> Start {
        if clickCount == 2 { return .zoomToggle }
        if let pinUnderPointer { return .selectPin(pinUnderPointer) }
        return .pan
    }
}

/// The circle a pin averages, as a fraction of the photograph's shorter side.
/// Zero is the one pixel under its centre.
enum SampleSize {
    static let initial = 0.02
    static let smallest = 0.002
    static let largest = 0.5

    /// One step of the scroll wheel: pushed up grows, pulled down shrinks. A wheel's
    /// notch moves as far as ten pixels of a trackpad's scroll.
    /// A scroll's delta as the hand moved: positive is the wheel pushed up,
    /// whatever the natural scrolling setting does to the event.
    static func handDelta(scrollingDeltaY delta: Double, inverted: Bool) -> Double {
        inverted ? -delta : delta
    }

    static func radius(from radius: Double, scrollDelta: Double, precise: Bool) -> Double {
        let step = precise ? scrollDelta : scrollDelta * 10
        guard step != 0 else { return radius }
        guard radius >= smallest else { return step > 0 ? smallest : 0 }
        let next = radius * exp(step * 0.01)
        return next < smallest ? 0 : min(next, largest)
    }
}
