import AppKit
import SwiftUI

/// The inspector's slider, drawn with four layers instead of an `NSSlider`
/// (≈0.1 ms to build, against ≈4 ms for an `NSSlider` on macOS 26; §9).
/// A 1 pt track, a tick at the default, a 2 pt pencil fill from the default
/// to the knob once changed, and a 9 pt knob (§4.9).
///
/// Double-click or ⌥-click resets, as in Lightroom and Capture One. With
/// keyboard focus, ← → step it (⇧ for ten steps), ↑ ↓ and Tab move to the
/// next control, and Return starts typing.
final class InkSliderView: NSView, InkFocusTarget {
    var range: ClosedRange<Double> = 0...1 { didSet { if range != oldValue { needsLayout = true } } }
    var step = 0.01
    var neutral = 0.0 { didSet { if neutral != oldValue { needsLayout = true } } }
    /// The tick at the neutral value; a slider without a default has none.
    var showsNeutral = true { didSet { tick.isHidden = !showsNeutral } }
    /// The model's value, which the knob shows (snapped by the model).
    var value = 0.0 { didSet { if value != oldValue { needsLayout = true } } }
    var changed = false { didSet { if changed != oldValue { restyle() } } }
    /// The row is pointed at: the knob turns `paper` and grows.
    var rowHovered = false { didSet { if rowHovered != oldValue { restyle() } } }
    var title = "" { didSet { setAccessibilityLabel(title) } }

    var onChange: (Double) -> Void = { _ in }
    var onEditing: (Bool) -> Void = { _ in }
    var onReset: () -> Void = {}
    /// A double-click's reset, when it differs: its first click has already
    /// moved the value to the pointer (and a linked slider with it).
    var onDoubleClickReset: (() -> Void)?
    /// Return with keyboard focus: type a value.
    var onType: () -> Void = {}
    var enabled = true
    var takesKeyboardFocus: Bool { enabled }

    private let track = CALayer(), tick = CALayer(), fill = CALayer(), knob = CAShapeLayer(), focus = CAShapeLayer()
    private var dragging = false { didSet { restyle() } }
    private static let knobSize: CGFloat = 9
    private static let inset: CGFloat = 5

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = false
        focusRingType = .none
        for sublayer in [track, tick, fill, knob, focus] { layer?.addSublayer(sublayer) }
        track.backgroundColor = Theme.NS.track.cgColor
        tick.backgroundColor = Theme.NS.dim.cgColor
        fill.backgroundColor = Theme.NS.accent.cgColor
        knob.lineWidth = 1.5
        knob.path = CGPath(ellipseIn: CGRect(x: -Self.knobSize / 2, y: -Self.knobSize / 2, width: Self.knobSize, height: Self.knobSize), transform: nil)
        focus.fillColor = nil
        focus.strokeColor = Theme.NS.paper.cgColor
        focus.lineWidth = Theme.Size.focusLine
        focus.isHidden = true
        for sublayer in [track, tick, fill, knob, focus] { sublayer.actions = Self.noAnimation }
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
        restyle()
    }
    required init?(coder: NSCoder) { fatalError() }

    private static let noAnimation: [String: CAAction] = ["position": NSNull(), "bounds": NSNull(), "frame": NSNull(), "transform": NSNull(),
                                                          "backgroundColor": NSNull(), "fillColor": NSNull(), "strokeColor": NSNull(), "hidden": NSNull(), "path": NSNull()]

    override var isFlipped: Bool { true }
    override var intrinsicContentSize: NSSize { NSSize(width: NSView.noIntrinsicMetric, height: Theme.Size.row) }

    // MARK: Geometry

    private var trackRect: CGRect {
        CGRect(x: Self.inset, y: (bounds.height / 2).rounded(.down), width: max(bounds.width - Self.inset * 2, 1), height: 1)
    }
    private func x(for value: Double) -> CGFloat {
        let span = range.upperBound - range.lowerBound
        let t = span > 0 ? (min(max(value, range.lowerBound), range.upperBound) - range.lowerBound) / span : 0
        return trackRect.minX + CGFloat(t) * trackRect.width
    }
    private func value(at x: CGFloat) -> Double {
        let t = Double((x - trackRect.minX) / trackRect.width)
        return range.lowerBound + min(max(t, 0), 1) * (range.upperBound - range.lowerBound)
    }

    override func layout() {
        super.layout()
        let line = trackRect
        let (knobX, neutralX) = (x(for: value), x(for: neutral))
        track.frame = line
        tick.frame = CGRect(x: neutralX.rounded() - 0.5, y: line.midY - 4.5, width: 1, height: 9)
        fill.frame = CGRect(x: min(knobX, neutralX), y: line.midY - 1, width: abs(knobX - neutralX), height: 2)
        knob.position = CGPoint(x: knobX, y: line.midY)
        let box = CGRect(x: line.minX - Self.inset, y: line.midY - 9, width: line.width + Self.inset * 2, height: 18)
            .insetBy(dx: -Theme.Size.focusOutset + 3, dy: -2)
        focus.path = CGPath(roundedRect: box, cornerWidth: Theme.Size.focusRadius, cornerHeight: Theme.Size.focusRadius, transform: nil)
        setAccessibilityValue(value)
    }

    private func restyle() {
        let hot = rowHovered || dragging
        knob.strokeColor = (dragging ? Theme.NS.paper : changed ? Theme.NS.accent : hot ? Theme.NS.paper : Theme.NS.label).cgColor
        knob.fillColor = (dragging ? Theme.NS.paper : changed ? Theme.NS.accent : Theme.NS.panel).cgColor
        knob.setAffineTransform(hot ? CGAffineTransform(scaleX: 1.2, y: 1.2) : .identity)
        fill.isHidden = !changed
        updateFocus()
    }
    private func updateFocus() {
        focus.isHidden = !(window?.firstResponder === self && InputMode.shared.keyboard)
    }

    // MARK: Mouse

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) {
        guard enabled else { return }
        if event.clickCount == 2 {
            (onDoubleClickReset ?? onReset)()
            return
        }
        if event.modifierFlags.contains(.option) {
            onReset()
            return
        }
        dragging = true
        onEditing(true)
        onChange(value(at: convert(event.locationInWindow, from: nil).x))
    }
    override func mouseDragged(with event: NSEvent) {
        guard dragging else { return }
        onChange(value(at: convert(event.locationInWindow, from: nil).x))
    }
    override func mouseUp(with event: NSEvent) {
        guard dragging else { return }
        dragging = false
        onEditing(false)
    }

    // MARK: Keyboard

    override var acceptsFirstResponder: Bool { enabled }
    override var canBecomeKeyView: Bool { true }
    override func becomeFirstResponder() -> Bool { defer { DispatchQueue.main.async { [weak self] in self?.updateFocus() } }; return true }
    override func resignFirstResponder() -> Bool { defer { DispatchQueue.main.async { [weak self] in self?.updateFocus() } }; return true }
    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 123: nudge(-1, event)
        case 124: nudge(1, event)
        case 125, 126: if let root = InkKeyLoop.root(of: self) { InkKeyLoop.advance(in: root, from: self, forward: event.keyCode == 125) }
        case 36, 76: onType()
        case 48: if let root = InkKeyLoop.root(of: self) { InkKeyLoop.advance(in: root, from: self, forward: !event.modifierFlags.contains(.shift)) }
        default: super.keyDown(with: event)
        }
    }
    private func nudge(_ direction: Double, _ event: NSEvent) {
        let steps = event.modifierFlags.contains(.shift) ? 10.0 : 1.0
        onChange(value + direction * steps * step)
        updateFocus()
    }

    // MARK: Accessibility

    override func accessibilityPerformIncrement() -> Bool { onChange(value + step); return true }
    override func accessibilityPerformDecrement() -> Bool { onChange(value - step); return true }
    override func accessibilityMinValue() -> Any? { range.lowerBound }
    override func accessibilityMaxValue() -> Any? { range.upperBound }
}

/// SwiftUI's handle on `InkSliderView`.
struct InkSlider: NSViewRepresentable {
    let title: String
    let value: Binding<Double>
    let range: ClosedRange<Double>
    let step: Double
    let neutral: Double
    let changed: Bool
    let hovered: Bool
    let editing: (Bool) -> Void
    let reset: () -> Void
    var type: () -> Void = {}
    var toolTip: String? = nil
    var showsNeutral = true
    var resetAfterClick: (() -> Void)? = nil

    func makeNSView(context: Context) -> InkSliderView {
        let slider = InkSliderView()
        slider.setContentHuggingPriority(.defaultLow, for: .horizontal)
        slider.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return slider
    }

    func updateNSView(_ slider: InkSliderView, context: Context) {
        slider.title = title
        slider.range = range
        slider.step = step
        slider.neutral = min(max(neutral, range.lowerBound), range.upperBound)
        if slider.showsNeutral != showsNeutral { slider.showsNeutral = showsNeutral }
        slider.value = value.wrappedValue
        slider.changed = changed
        slider.rowHovered = hovered
        slider.enabled = context.environment.isEnabled
        if slider.toolTip != toolTip { slider.toolTip = toolTip }
        let value = value
        slider.onChange = { [weak slider] newValue in
            value.wrappedValue = newValue
            // Show the model's snapped value at once, not at the next update.
            slider?.value = value.wrappedValue
        }
        slider.onEditing = editing
        slider.onReset = reset
        slider.onDoubleClickReset = resetAfterClick
        slider.onType = type
    }
}

/// A slider row: the label in a column fitted to the longest label, the track, the value in the
/// serif (click it to type one), and ↺ once it's changed (§4.9). A number
/// typed past either end is set to the end, with a note under the row.
struct LabeledSlider: View {
    let title: String
    let value: Binding<Double>
    let range: ClosedRange<Double>
    let step: Double
    /// Shown after the value, such as "%".
    var unit = ""
    let defaultValue: Double
    let modified: Bool
    let reset: () -> Void
    let editing: (Bool) -> Void
    /// The tooltip of the title and the slider.
    var help: String? = nil
    /// A double-click on the track's reset, when it differs from `reset`.
    var resetAfterClick: (() -> Void)? = nil

    @State private var typing = false
    @State private var hovered = false
    @State private var note: ClampNote?
    private var decimals: Int { Self.decimals(step) }
    private var format: FloatingPointFormatStyle<Double> { .number.precision(.fractionLength(0...decimals)) }

    struct ClampNote: Equatable {
        let text: String
        let serial = UUID()
    }

    static func decimals(_ step: Double) -> Int { max(0, Int(-log10(step).rounded(.down))) }

    /// "3 is the most; it was set to 3." for a typed number past the end.
    static func clampNote(typed: Double, range: ClosedRange<Double>, step: Double, unit: String = "") -> String? {
        let format = FloatingPointFormatStyle<Double>.number.precision(.fractionLength(0...decimals(step)))
        if typed > range.upperBound {
            let end = "\(range.upperBound.formatted(format))\(unit)"
            return "\(end) is the most; it was set to \(end)."
        }
        if typed < range.lowerBound {
            let end = "\(range.lowerBound.formatted(format))\(unit)"
            return "\(end) is the least; it was set to \(end)."
        }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                RowLabel(title: title, bright: modified || hovered)
                    .frame(width: Theme.Size.sliderLabel, alignment: .leading)
                    .onTapGesture(count: 2) { reset() }
                    .tooltip(help)
                InkSlider(title: title, value: value, range: range, step: step, neutral: defaultValue, changed: modified, hovered: hovered,
                          editing: editing, reset: reset, type: { typing = true }, toolTip: help, resetAfterClick: resetAfterClick)
                    .frame(height: Theme.Size.row)
                valueView.frame(width: Theme.Size.valueWidth, alignment: .trailing)
                resetButton.frame(width: 18).padding(.trailing, -8)
            }
            .padding(.horizontal, Theme.Size.gutter)
            .frame(height: Theme.Size.row)
            .contentShape(Rectangle())
            .onHover { hovered = $0 }
            if let note {
                HintText(note.text)
                    .transition(.opacity)
                    .task(id: note.serial) {
                        try? await Task.sleep(for: .seconds(2.5))
                        guard !Task.isCancelled else { return }
                        withAnimation(.easeOut(duration: 0.25)) { self.note = nil }
                    }
            }
        }
        .contextMenu { Button("Reset to Default", action: reset).disabled(!modified) }
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private var valueView: some View {
        // A field only while typing, so the window never focuses one by default.
        if typing {
            ValueField(title: title, value: value, range: range, decimals: decimals) { typed in
                typing = false
                if let typed, let text = Self.clampNote(typed: typed, range: range, step: step, unit: unit) { note = ClampNote(text: text) }
            }
            .frame(width: Theme.Size.valueWidth, height: 20)
            .overlay(alignment: .bottom) { Rectangle().fill(Theme.paper).frame(height: 1) }
        } else {
            ValueText(text: "\(value.wrappedValue.formatted(format))\(unit)", changed: modified, hovered: hovered) { typing = true }
        }
    }

    @ViewBuilder private var resetButton: some View {
        // Hidden until the value changes, and while typing.
        if modified && !typing {
            Button(action: reset) {
                Text("↺").font(Theme.ui(12)).foregroundStyle(Theme.dim).frame(width: 18, height: 18)
            }
            .buttonStyle(InkPressStyle())
            .help("Reset to \(defaultValue.formatted(format))\(unit)")
            .accessibilityLabel("Reset \(title)")
        } else {
            Color.clear.frame(width: 18, height: 18)
        }
    }
}

/// The value: `dim` at its default; 700 `paper` with the pencil under it
/// once changed. Pointing at it underlines it in `dim`; a click types.
private struct ValueText: View {
    let text: String
    let changed: Bool
    let hovered: Bool
    let type: () -> Void
    @State private var pointed = false

    var body: some View {
        Text(text)
            .font(Theme.serif(14.5, weight: changed ? 700 : 400))
            .foregroundStyle(changed || hovered ? Theme.paper : Theme.dim)
            .lineLimit(1)
            .fixedSize()
            .padding(.bottom, 2)
            .overlay(alignment: .bottom) {
                if changed { PencilStroke().pencil().frame(height: 5).offset(y: 3) }
                else if pointed { Rectangle().fill(Theme.dim).frame(height: 1) }
            }
            .contentShape(Rectangle())
            .onHover { pointed = $0 }
            .onTapGesture(perform: type)
            .help("Click to type a value")
    }
}

/// The number field shown while typing a slider's value: the serif on a
/// `paper` rule with a `paper` caret, the selection a faint yellow. AppKit,
/// because SwiftUI's focus never reaches a field in the inspector column;
/// the field takes focus as it appears, which selects the value so typing
/// replaces it. Return, Tab or a click elsewhere applies; Esc puts the old
/// value back.
struct ValueField: NSViewRepresentable {
    let title: String
    @Binding var value: Double
    let range: ClosedRange<Double>
    let decimals: Int
    /// Called once editing ends, with the number typed (nil for Esc or
    /// something that isn't a number).
    let onEnd: (Double?) -> Void

    private var formatter: NumberFormatter {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.usesGroupingSeparator = false
        formatter.maximumFractionDigits = decimals
        formatter.isLenient = true
        return formatter
    }

    func makeNSView(context: Context) -> NSTextField {
        let field = InkTextField()
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.font = Theme.serifFont(14.5) as NSFont
        field.textColor = Theme.NS.paper
        field.alignment = .right
        field.cell?.isScrollable = true
        field.setAccessibilityLabel(title)
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.currentEditor() == nil { field.stringValue = formatter.string(from: value as NSNumber) ?? "" }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: ValueField
        private var cancelled = false
        init(_ parent: ValueField) { self.parent = parent }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
            cancelled = true
            control.window?.makeFirstResponder(nil)
            return true
        }

        /// Return, Tab, clicking elsewhere and Escape all end here; only Escape discards the typing.
        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            var typed: Double?
            if !cancelled, let number = parent.formatter.number(from: field.stringValue) {
                typed = number.doubleValue
                parent.value = number.doubleValue
            }
            parent.onEnd(typed)
        }
    }
}

/// A borderless field that takes focus as it appears (selecting its text)
/// and edits with a `paper` caret and a faint yellow selection, never the
/// system blue.
final class InkTextField: FocusingTextField {
    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became, let editor = currentEditor() as? NSTextView { InkTextField.style(editor) }
        return became
    }
    static func style(_ editor: NSTextView) {
        editor.insertionPointColor = Theme.NS.paper
        editor.selectedTextAttributes = [.backgroundColor: Theme.NS.accent.withAlphaComponent(0.32), .foregroundColor: Theme.NS.paper]
    }
}
