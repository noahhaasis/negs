import AppKit
import SwiftUI

/// The adjustment a speed-edit key holds, as in Capture One's Speed Edit:
/// Q exposure, W contrast, E saturation, A highlights, S shadows, D gain,
/// F lift, Z temperature, X tint.
enum SpeedEditTarget: CaseIterable, Equatable {
    case exposure, contrast, saturation, highlights, shadows, gain, lift, temperature, tint

    static func key(_ characters: String) -> SpeedEditTarget? {
        switch characters {
        case "q": .exposure
        case "w": .contrast
        case "e": .saturation
        case "a": .highlights
        case "s": .shadows
        case "d": .gain
        case "f": .lift
        case "z": .temperature
        case "x": .tint
        default: nil
        }
    }

    var title: String {
        switch self {
        case .exposure: "Exposure"
        case .contrast: "Contrast"
        case .saturation: "Saturation"
        case .highlights: "Highlights"
        case .shadows: "Shadows"
        case .gain: "Gain"
        case .lift: "Lift"
        case .temperature: "Temperature"
        case .tint: "Tint"
        }
    }

    /// The edit's path; exposure goes through the Brightness offset instead.
    var path: String? {
        switch self {
        case .exposure: nil
        case .contrast: "prep.contrast"
        case .saturation: "prep.saturation"
        case .highlights: "prep.highlights"
        case .shadows: "prep.shadows"
        case .gain: "prep.gain"
        case .lift: "prep.lift"
        case .temperature: EditorModel.temperaturePath
        case .tint: EditorModel.tintPath
        }
    }

    var whiteBalance: Bool { self == .temperature || self == .tint }

    /// The inspector's range and step for a Prep slider.
    var slider: (range: ClosedRange<Double>, step: Double)? {
        switch self {
        case .exposure: return nil
        case .temperature: return (EditorModel.temperatureRange, 1)
        case .tint: return (EditorModel.tintRange, 0.01)
        default:
            for case .slider(_, let path, let range, let step) in prepToneControls + prepColorControls where path == self.path {
                return (range, step)
            }
            return nil
        }
    }

    /// How far one notch moves it: with ⌥ (fine), alone, and with ⇧
    /// (coarse). Temperature moves in mireds, so a notch looks about the
    /// same at 3000 K as at 9000 K.
    func amount(fine: Bool, coarse: Bool) -> Double {
        let (small, normal, large): (Double, Double, Double) = switch self {
        case .exposure: (0.05, 0.1, 0.5)
        case .temperature: (1, 3, 15)
        case .tint: (0.01, 0.02, 0.1)
        default: (1, 2, 10)
        }
        return fine ? small : coarse ? large : normal
    }

    /// `value` moved by `steps` notches of `amount`: up is brighter, more,
    /// warmer (higher kelvin) and greener, as the sliders go.
    func moved(_ value: Double, steps: Double, amount: Double) -> Double {
        switch self {
        case .temperature:
            let mired = 1e6 / max(value, 1) - steps * amount
            return 1e6 / max(mired, 1)
        default:
            return value + steps * amount
        }
    }

    func formatted(_ value: Double) -> String {
        switch self {
        case .exposure: return String(format: "%+.2f EV", value)
        case .temperature: return String(format: "%.0f K", value)
        case .tint: return String(format: "%.2f", value)
        default: return value > 0 ? String(format: "+%.0f", value) : String(format: "%.0f", value)
        }
    }
}

/// Hold a speed-edit key (`SpeedEditTarget`) and scroll, or press ↑ ↓, to
/// adjust in fixed steps: ⌥ for fine ones, ⇧ for coarse. The canvas shows
/// the adjustment and its value while the key is held, and a hold is one
/// undo step. A tap of S in Compare or of X still switches the layout or
/// rejects. Temperature and tint switch a RAW to Custom white balance,
/// starting from As shot.
@MainActor
final class SpeedEdit {
    private let model: EditorModel
    private var held: (key: UInt16, target: SpeedEditTarget)?
    /// The hold changed the edit (and began an interaction).
    private var changed = false
    /// Switching to Custom white balance before the first change applies.
    private var preparing = false
    /// Notches asked for while preparing.
    private var queued = 0.0
    /// Let go while preparing: the hold ends once that's done.
    private var released = false
    /// A trackpad's scroll, in points, not yet a whole notch.
    private var scrolled = 0.0
    /// A tap's key event, posted again for its own meaning (S in Compare, X).
    private var replaying: (keyCode: UInt16, timestamp: TimeInterval)?

    /// Trackpad points per notch.
    static let pointsPerNotch = 6.0

    init(model: EditorModel) { self.model = model }

    var holding: Bool { held != nil }

    /// Key events, before the window's other keys. True if handled.
    func handleKey(_ event: NSEvent, in window: NSWindow?) -> Bool {
        if event.type == .keyUp {
            guard let held, event.keyCode == held.key else { return false }
            release(event)
            return true
        }
        guard event.type == .keyDown, event.window === window else { return false }
        if let replay = replaying, replay.keyCode == event.keyCode, replay.timestamp == event.timestamp {
            replaying = nil
            return false
        }
        if let held {
            if event.keyCode == held.key { return true } // auto-repeat
            switch event.keyCode {
            case 126, 125: // ↑ ↓
                adjust(steps: event.keyCode == 126 ? 1 : -1, flags: event.modifierFlags)
                return true
            case 123, 124: return true // ← → would open another photograph mid-hold
            default: break
            }
        }
        guard !event.isARepeat, event.modifierFlags.isDisjoint(with: [.command, .control, .option, .shift]),
              let characters = event.charactersIgnoringModifiers?.lowercased(),
              let target = SpeedEditTarget.key(characters), takesKeys(window) else { return false }
        // Where it can't adjust, a key with a meaning of its own keeps it at once.
        if unavailable(target) != nil, hasOwnMeaning(target) { return false }
        if held != nil { finish() }
        begin(target, key: event.keyCode)
        return true
    }

    /// Scrolling while a key is held adjusts. True if handled.
    func handleScroll(_ event: NSEvent, in window: NSWindow?) -> Bool {
        guard held != nil, event.window === window else { return false }
        // The glide after a flick would run on past what was meant.
        guard event.momentumPhase.isEmpty else { return true }
        // Up (wheel away, or fingers up with natural scrolling) is more.
        let deltaY = Double(event.isDirectionInvertedFromDevice ? -event.scrollingDeltaY : event.scrollingDeltaY)
        // A sideways scroll isn't an adjustment.
        guard abs(deltaY) >= abs(Double(event.scrollingDeltaX)) else { return true }
        if event.hasPreciseScrollingDeltas {
            scrolled += deltaY
            let notches = (scrolled / Self.pointsPerNotch).rounded(.towardZero)
            guard notches != 0 else { return true }
            scrolled -= notches * Self.pointsPerNotch
            adjust(steps: notches, flags: event.modifierFlags)
        } else if deltaY != 0 {
            // A wheel's notch, one step however fast it spins.
            adjust(steps: deltaY > 0 ? 1 : -1, flags: event.modifierFlags)
        }
        return true
    }

    /// Ends a hold without its key: the window stopped being key.
    func cancel() {
        guard held != nil else { return }
        finish()
    }

    // MARK: Hold

    private func takesKeys(_ window: NSWindow?) -> Bool {
        guard let window, model.mode == .photo, model.source != nil, !model.cropping, model.tray == nil,
              model.alert == nil, !(window.firstResponder is NSText) else { return false }
        return true
    }

    /// X rejects, and S in Compare switches the layout: a tap still does.
    private func hasOwnMeaning(_ target: SpeedEditTarget) -> Bool {
        target == .tint || (target == .shadows && model.comparison != nil)
    }

    /// Why the adjustment isn't there for this photograph, if it isn't.
    private func unavailable(_ target: SpeedEditTarget) -> String? {
        if target.whiteBalance { return model.isRaw ? nil : "\(target.title): RAW files only" }
        return model.isNegative ? "\(target.title): not on a negative" : nil
    }

    private func begin(_ target: SpeedEditTarget, key: UInt16) {
        held = (key, target)
        changed = false; preparing = false; queued = 0; released = false; scrolled = 0
        if let reason = unavailable(target) {
            model.speedEditReadout = reason
            return
        }
        // Read the camera's white balance while the key is down, so the
        // first notch needn't wait for it.
        if target.whiteBalance, model.value(EditorModel.whiteBalanceModePath).string != "custom" {
            Task { _ = await model.asShotWhiteBalance() }
        }
        showReadout()
    }

    private func release(_ event: NSEvent) {
        guard let held else { return }
        let tapped = !changed && !preparing
        if preparing {
            released = true
            return
        }
        finish()
        // A tap keeps the key's own meaning where it has one.
        if tapped, hasOwnMeaning(held.target),
           let down = NSEvent.keyEvent(with: .keyDown, location: event.locationInWindow, modifierFlags: event.modifierFlags,
                                       timestamp: event.timestamp, windowNumber: event.windowNumber, context: nil,
                                       characters: event.characters ?? "", charactersIgnoringModifiers: event.charactersIgnoringModifiers ?? "",
                                       isARepeat: false, keyCode: event.keyCode) {
            replaying = (down.keyCode, down.timestamp)
            NSApp.postEvent(down, atStart: true)
        }
    }

    private func finish() {
        if let held, changed { model.endInteraction(action: "Change \(held.target.title)") }
        held = nil
        changed = false; preparing = false; queued = 0; released = false; scrolled = 0
        model.speedEditReadout = nil
    }

    // MARK: Adjusting

    private func adjust(steps: Double, flags: NSEvent.ModifierFlags) {
        guard let held, unavailable(held.target) == nil else { return }
        let target = held.target
        let amount = target.amount(fine: flags.contains(.option), coarse: flags.contains(.shift))
        if preparing { queued += steps * amount; return }
        if !changed {
            changed = true
            model.beginInteraction()
        }
        if target.whiteBalance, model.value(EditorModel.whiteBalanceModePath).string != "custom" {
            preparing = true
            queued = steps * amount
            Task { await switchToCustom(for: held.key) }
            return
        }
        apply(target, delta: steps * amount)
    }

    /// Custom white balance first, then the notches asked for meanwhile.
    private func switchToCustom(for key: UInt16) async {
        let custom = await model.chooseCustomWhiteBalance()
        guard let held, held.key == key, preparing else { return }
        preparing = false
        let delta = queued
        queued = 0
        if custom, delta != 0 { apply(held.target, delta: delta) } else { showReadout() }
        if released { finish() }
    }

    /// Moves the adjustment by `delta` (notches times their amount).
    private func apply(_ target: SpeedEditTarget, delta: Double) {
        if target == .exposure {
            let offset = model.exposureOffset
            offset.wrappedValue = offset.wrappedValue + delta
        } else if let path = target.path, let slider = target.slider {
            let current = model.value(path).double
            let binding = model.number(path, range: slider.range, step: slider.step)
            binding.wrappedValue = target.moved(current, steps: 1, amount: delta)
        }
        showReadout()
    }

    private func showReadout() {
        guard let held else { return }
        let target = held.target
        let value: Double
        if target == .exposure {
            value = model.exposureOffset.wrappedValue
        } else if let path = target.path {
            value = model.value(path).double
        } else {
            return
        }
        if target.whiteBalance, model.value(EditorModel.whiteBalanceModePath).string != "custom" {
            model.speedEditReadout = "\(target.title)  As shot"
        } else {
            model.speedEditReadout = "\(target.title)  \(target.formatted(value))"
        }
    }
}

/// The speed-edit readout, on a dark plate like Developing's.
struct SpeedEditTag: View {
    let text: String

    var body: some View {
        Text(text)
            .font(Theme.serif(17))
            .foregroundStyle(Theme.paper)
            .monospacedDigit()
            .fixedSize()
            .padding(EdgeInsets(top: 7, leading: 16, bottom: 9, trailing: 16))
            .background(RoundedRectangle(cornerRadius: 3).fill(Theme.bg.opacity(0.78)))
            .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(Theme.paper.opacity(0.08), lineWidth: 1))
            .shadow(color: .black.opacity(0.4 * Theme.shadowStrength), radius: 8, y: 4)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(text)
            .accessibilityAddTraits(.updatesFrequently)
    }
}
