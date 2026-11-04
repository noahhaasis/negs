import AppKit

/// What a mouse-down on the photograph starts. Pure, so the map from clicks
/// and modifiers to a gesture is testable without events.
///
/// The order matters: a double-click wins over every modifier, ⇧ pans (even
/// inside a selection), and ⌃ picks — or moves the selection it lands inside.
/// A plain drag selects a region, or moves the selection it lands inside.
struct CanvasGesture {
    enum Start: Equatable {
        /// Toggle between fit and 1:1, about the click.
        case zoomToggle
        /// Scroll, only when the view has somewhere to go.
        case pan
        case select
        case move
        /// 3×3 and 7×7 samples of the tone under the pointer.
        case pickSmall
        case pickLarge
    }

    static func start(
        clickCount: Int,
        modifiers: NSEvent.ModifierFlags,
        insideSelection: Bool
    ) -> Start {
        if clickCount == 2 { return .zoomToggle }
        if modifiers.contains(.shift) { return .pan }
        if modifiers.contains(.control) {
            if insideSelection { return .move }
            return modifiers.contains(.option) ? .pickLarge : .pickSmall
        }
        return insideSelection ? .move : .select
    }
}
