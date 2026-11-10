import AppKit
import Observation
import SwiftUI

// The inspector's controls in the T2 · Label look (IMPLEMENTATION.md §4.8,
// §4.9, §4.13): words and rules instead of boxes, cream ink, and the yellow
// pencil only ever as a mark. Never the system blue.

/// Whether Tab or an arrow key moved focus last, so the cream focus box
/// shows only for the keyboard, never after a click (§4.13).
@MainActor @Observable final class InputMode {
    static let shared = InputMode()
    private(set) var keyboard = false

    /// Called for every event the editing window receives.
    func note(_ event: NSEvent) {
        switch event.type {
        case .keyDown:
            // Tab, ⇧Tab and the arrows move focus; other keys don't.
            if [48, 123, 124, 125, 126].contains(event.keyCode), !keyboard { keyboard = true }
        case .leftMouseDown, .rightMouseDown, .otherMouseDown:
            if keyboard { keyboard = false }
        default: break
        }
    }
}

/// The keyboard focus box: 1.5 pt `paper`, 5 pt outside, radius 3.
struct FocusBox: View {
    var body: some View {
        RoundedRectangle(cornerRadius: Theme.Size.focusRadius)
            .stroke(Theme.paper, lineWidth: Theme.Size.focusLine)
            .padding(-Theme.Size.focusOutset)
            .allowsHitTesting(false)
    }
}

/// Something Tab can reach in the inspector: the slider, or the anchor
/// behind a SwiftUI control. SwiftUI's own focus doesn't reach controls in
/// the inspector column, so keyboard focus is AppKit's, and Tab steps
/// through these in the order they appear (`InkKeyLoop`).
@MainActor protocol InkFocusTarget: NSView {
    var takesKeyboardFocus: Bool { get }
}

/// Tab and ⇧Tab through the inspector's controls, top to bottom.
@MainActor enum InkKeyLoop {
    /// The targets under `root` that can take focus now, in reading order.
    static func targets(in root: NSView) -> [NSView] {
        func walk(_ view: NSView) -> [NSView] {
            guard !view.isHidden, view.alphaValue > 0.01 else { return [] }
            let own: [NSView] = (view as? InkFocusTarget).map { $0.takesKeyboardFocus ? [view] : [] } ?? []
            return own + view.subviews.flatMap(walk)
        }
        let found = walk(root).filter { $0.window != nil && $0.bounds.width > 0 }
        func origin(_ view: NSView) -> CGPoint {
            let frame = view.convert(view.bounds, to: nil)
            return CGPoint(x: frame.minX, y: -frame.maxY)
        }
        return found.sorted { a, b in
            let (pa, pb) = (origin(a), origin(b))
            return abs(pa.y - pb.y) > 4 ? pa.y < pb.y : pa.x < pb.x
        }
    }

    /// Moves focus from `view` (or from outside, when nil) to the next or
    /// previous target under `root`. Returns false when there's none.
    @discardableResult static func advance(in root: NSView, from view: NSView?, forward: Bool) -> Bool {
        let all = targets(in: root)
        guard !all.isEmpty, let window = root.window else { return false }
        let index = view.flatMap { current in all.firstIndex { $0 === current } }
        let next: NSView
        if let index { next = all[(index + (forward ? 1 : all.count - 1)) % all.count] }
        else { next = forward ? all[0] : all[all.count - 1] }
        window.makeFirstResponder(next)
        next.scrollToVisible(next.bounds.insetBy(dx: 0, dy: -24))
        return true
    }

    /// The inspector column holding `view`.
    static func root(of view: NSView) -> NSView? {
        var current: NSView? = view
        while let next = current {
            if next.identifier == inspectorIdentifier { return next }
            current = next.superview
        }
        return nil
    }
    static let inspectorIdentifier = NSUserInterfaceItemIdentifier("inspector")
}

/// The AppKit view behind a SwiftUI control that holds its keyboard focus:
/// draws the cream box round the control while it has the keys, and runs
/// the control's action on Space or Return.
final class FocusAnchorView: NSView, InkFocusTarget {
    var activate: (() -> Void)?
    var delete: (() -> Void)?
    var enabled = true
    var takesKeyboardFocus: Bool { enabled }
    private let box = CAShapeLayer()

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = false
        box.fillColor = nil
        box.strokeColor = Theme.NS.paper.cgColor
        box.lineWidth = Theme.Size.focusLine
        box.isHidden = true
        box.actions = ["hidden": NSNull(), "path": NSNull()]
        layer?.addSublayer(box)
        focusRingType = .none
        setAccessibilityElement(false)
    }
    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        let rect = bounds.insetBy(dx: -Theme.Size.focusOutset + 4, dy: -2)
        box.path = CGPath(roundedRect: rect, cornerWidth: Theme.Size.focusRadius, cornerHeight: Theme.Size.focusRadius, transform: nil)
    }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override var acceptsFirstResponder: Bool { enabled }
    override func becomeFirstResponder() -> Bool { box.isHidden = false; return true }
    override func resignFirstResponder() -> Bool { box.isHidden = true; return true }
    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 49, 36, 76: activate?()
        case 51, 117 where delete != nil: delete?()
        case 48: if let root = InkKeyLoop.root(of: self) { InkKeyLoop.advance(in: root, from: self, forward: !event.modifierFlags.contains(.shift)) }
        case 125: if let root = InkKeyLoop.root(of: self) { InkKeyLoop.advance(in: root, from: self, forward: true) }
        case 126: if let root = InkKeyLoop.root(of: self) { InkKeyLoop.advance(in: root, from: self, forward: false) }
        default: super.keyDown(with: event)
        }
    }
}

private struct FocusAnchor: NSViewRepresentable {
    let activate: (() -> Void)?
    var delete: (() -> Void)?
    func makeNSView(context: Context) -> FocusAnchorView { FocusAnchorView() }
    func updateNSView(_ view: FocusAnchorView, context: Context) {
        view.activate = activate
        view.delete = delete
        view.enabled = context.environment.isEnabled
    }
}

extension View {
    /// Takes keyboard focus, shows the cream box only when the keyboard
    /// moved it there, and runs `activate` on Space or Return (and `delete`,
    /// if given, on ⌫ or ⌦).
    func inkFocus(activate: (() -> Void)? = nil, delete: (() -> Void)? = nil) -> some View {
        background(FocusAnchor(activate: activate, delete: delete))
    }

    /// The pencil under a word, 5 pt tall, just under its baseline.
    func pencilUnderline(_ shown: Bool = true, height: CGFloat = 5, offset: CGFloat = 4) -> some View {
        overlay(alignment: .bottom) {
            if shown {
                PencilStroke().pencil().frame(height: height).offset(y: offset).allowsHitTesting(false)
            }
        }
    }

    /// Fades a control that can't be used, as the design does everywhere.
    func inkDisabled(_ disabled: Bool) -> some View {
        self.disabled(disabled).opacity(disabled ? Theme.disabledOpacity : 1)
    }
}

/// The yellow edit dot, 5 pt.
struct EditDot: View {
    var size: CGFloat = 5
    var body: some View {
        Circle().fill(Theme.accent).frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

// MARK: Rows

/// A row's label in SF 12: `label`, or `paper` when changed or pointed at.
struct RowLabel: View {
    let title: String
    var bright = false

    var body: some View {
        Text(title).font(Theme.ui(12)).foregroundStyle(bright ? Theme.paper : Theme.label).lineLimit(1)
    }
}

/// The app's switches as tick boxes: a 13 pt square, crossed when on.
struct TickBox: View {
    let on: Bool
    var hovered = false
    var pressed = false

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 2)
                .fill(pressed ? Theme.paper.opacity(0.12) : .clear)
            RoundedRectangle(cornerRadius: 2)
                .strokeBorder(hovered || pressed ? Theme.label : on ? Theme.dim : Theme.track, lineWidth: 1.2)
            if on {
                Path { path in
                    path.move(to: CGPoint(x: 1.5, y: 1.5)); path.addLine(to: CGPoint(x: 11.5, y: 11.5))
                    path.move(to: CGPoint(x: 11.5, y: 1.5)); path.addLine(to: CGPoint(x: 1.5, y: 11.5))
                }
                .stroke(Theme.paper, style: StrokeStyle(lineWidth: 1.4, lineCap: .round))
            }
        }
        .frame(width: 13, height: 13)
        .accessibilityHidden(true)
    }
}

/// A switch: the label, then the tick box at the row's end.
struct TickRow: View {
    let title: String
    @Binding var isOn: Bool
    var changed = false
    var help: String? = nil
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 8) {
            RowLabel(title: title, bright: changed || hovered)
            Spacer(minLength: 8)
            TickBox(on: isOn, hovered: hovered)
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row)
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture { isOn.toggle() }
        .inkFocus { isOn.toggle() }
        .tooltip(help)
        .accessibilityRepresentation { Toggle(title, isOn: $isOn) }
    }
}

/// One item of a choice's menu.
enum ChoiceItem: Hashable {
    case option(String, tag: String)
    /// A section title in the menu.
    case header(String)
    case divider
}

/// Remembers the AppKit view under a SwiftUI control, to pop a menu there.
@MainActor final class MenuAnchor {
    weak var view: NSView?
}

struct AnchorView: NSViewRepresentable {
    let anchor: MenuAnchor
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        anchor.view = view
        return view
    }
    func updateNSView(_ view: NSView, context: Context) { anchor.view = view }
}

/// A menu item that runs a closure.
final class ClosureMenuItem: NSMenuItem {
    private let handler: () -> Void
    init(_ title: String, checked: Bool = false, handler: @escaping () -> Void) {
        self.handler = handler
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
        state = checked ? .on : .off
    }
    required init(coder: NSCoder) { fatalError() }
    @objc private func fire() { handler() }
}

/// Pops a native menu of `items` just under `view`, with a tick by `selected`.
@MainActor func popUpChoiceMenu(_ items: [ChoiceItem], selected: String, under view: NSView?, choose: @escaping (String) -> Void) {
    guard let view else { return }
    let menu = NSMenu()
    menu.appearance = NSAppearance(named: .darkAqua)
    menu.autoenablesItems = false
    for item in items {
        switch item {
        case .option(let title, let tag): menu.addItem(ClosureMenuItem(title, checked: tag == selected) { choose(tag) })
        case .header(let title):
            if #available(macOS 14, *) { menu.addItem(.sectionHeader(title: title)) }
        case .divider: menu.addItem(.separator())
        }
    }
    menu.popUp(positioning: nil, at: NSPoint(x: 0, y: -4), in: view)
}

/// A choice: the label, then the value in the serif and a `⌄`; a click
/// opens a native menu under the value (§4.9).
struct ChoiceRow: View {
    let title: String
    let items: [ChoiceItem]
    let selection: String
    var changed = false
    var help: String? = nil
    let choose: (String) -> Void
    @State private var hovered = false
    @State private var anchor = MenuAnchor()

    private var current: String {
        for case .option(let title, let tag) in items where tag == selection { return title }
        return selection
    }

    var body: some View {
        HStack(spacing: 8) {
            RowLabel(title: title, bright: changed || hovered)
            Spacer(minLength: 8)
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(current).font(Theme.serif(14.5, weight: changed ? 700 : 400)).foregroundStyle(Theme.paper).lineLimit(1)
                    .pencilUnderline(changed)
                Text("⌄").font(Theme.ui(11)).foregroundStyle(hovered ? Theme.paper : Theme.dim).offset(y: -2)
            }
            .background(AnchorView(anchor: anchor))
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row)
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture(perform: open)
        .inkFocus(activate: open)
        .tooltip(help)
        .accessibilityElement()
        .accessibilityLabel(title)
        .accessibilityValue(current)
        .accessibilityAddTraits(.isButton)
        .accessibilityAction(.default, open)
    }

    private func open() { popUpChoiceMenu(items, selected: selection, under: anchor.view, choose: choose) }
}

/// A label and a value to read, not change: "Reads *Display P3, from its profile*".
struct ReadoutRow: View {
    let title: String
    let value: String
    var italic = false
    var help: String? = nil

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RowLabel(title: title)
            Spacer(minLength: 8)
            Text(value).font(Theme.serif(14.5, italic: italic)).foregroundStyle(Theme.paper)
                .lineLimit(2).multilineTextAlignment(.trailing)
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(minHeight: Theme.Size.row)
        .tooltip(help)
        .accessibilityElement(children: .combine)
    }
}

/// A hint under a row: italic 13 `dim`; warnings and errors in `warn`, in words.
struct HintText: View {
    let text: String
    var warning = false

    init(_ text: String, warning: Bool = false) {
        self.text = text
        self.warning = warning
    }

    var body: some View {
        Text(text).font(Theme.serif(13, italic: true)).foregroundStyle(warning ? Theme.warn : Theme.dim)
            .lineSpacing(13 * 0.25 - 3)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Theme.Size.gutter)
            .padding(.bottom, 7)
    }
}

/// Outlined button (Balance Print, Try Again, OK, Import Presets…): 23 tall,
/// a `track` outline, radius 12, Newsreader 13.5 500 in `paper`.
struct OutlinedButtonStyle: ButtonStyle {
    /// An alert's default button (Return): its ring in `dim`, a step
    /// brighter, as well as at the right.
    var prominent = false

    func makeBody(configuration: Configuration) -> some View { Outlined(configuration: configuration, prominent: prominent) }

    private struct Outlined: View {
        let configuration: Configuration
        let prominent: Bool
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false

        var body: some View {
            configuration.label
                .font(Theme.serif(13.5, weight: 500))
                .foregroundStyle(Theme.paper)
                .padding(.horizontal, 12).padding(.bottom, 1)
                .frame(height: 23)
                .background(Capsule().fill(Theme.paper.opacity(configuration.isPressed ? 0.09 : hovered ? 0.04 : 0)))
                .overlay(Capsule().strokeBorder(hovered || configuration.isPressed ? (prominent ? Theme.label : Theme.dim) : prominent ? Theme.dim : Theme.track, lineWidth: 1))
                .contentShape(Capsule())
                .opacity(enabled ? 1 : Theme.disabledOpacity)
                .onHover { hovered = $0 && enabled }
        }
    }
}

/// The two filled buttons: Export and crop's Done. Yellow, radius 3,
/// Newsreader 18 700 in `onAccent`; hover brightens, pressing sinks 1 pt.
struct PencilButtonStyle: ButtonStyle {
    var height: CGFloat = 36

    func makeBody(configuration: Configuration) -> some View { Filled(configuration: configuration, height: height) }

    private struct Filled: View {
        let configuration: Configuration
        let height: CGFloat
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false

        var body: some View {
            configuration.label
                .font(Theme.serif(18, weight: 700))
                .foregroundStyle(Theme.onAccent)
                .frame(maxWidth: .infinity, minHeight: height, maxHeight: height)
                .padding(.bottom, 2)
                .background(RoundedRectangle(cornerRadius: 3).fill(Theme.accent))
                .brightness(configuration.isPressed ? -0.12 : hovered ? 0.05 : 0)
                .offset(y: configuration.isPressed ? 1 : 0)
                .contentShape(Rectangle())
                .opacity(enabled ? 1 : Theme.disabledOpacity)
                .onHover { hovered = $0 && enabled }
        }
    }
}

/// Words that act, written as a link: italic, with a `track` underline that
/// darkens on hover (Reset, New, Import…).
struct WordButton: View {
    let title: String
    var size: CGFloat = 14
    var help: String? = nil
    let action: () -> Void
    @Environment(\.isEnabled) private var enabled
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text(title).font(Theme.serif(size, italic: true))
                .foregroundStyle(hovered ? Theme.paper : Theme.label)
                .underline(true, color: hovered ? Theme.dim : Theme.track)
        }
        .buttonStyle(InkPressStyle())
        .opacity(enabled ? 1 : Theme.disabledOpacity)
        .onHover { hovered = $0 && enabled }
        .inkFocus(activate: action)
        .tooltip(help)
    }
}

/// Pressed words drop to 0.62 (§4.13).
struct InkPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.62 : 1).contentShape(Rectangle())
    }
}

// MARK: Sections and modules

/// A rule between sections and modules, in `line`.
struct InkRule: View {
    var body: some View { Rectangle().fill(Theme.line).frame(height: 1).accessibilityHidden(true) }
}

/// A section of the panel: a head in small bold caps (with the dot when
/// anything in it changed), its rows, and a rule under it.
struct InkSection<Content: View>: View {
    let title: String?
    var edited = false
    /// Struck through in pencil with "— skipped" after it (scanning the film).
    var skipped: String? = nil
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                HStack(spacing: 0) {
                    Text(title.uppercased())
                        .font(Theme.serif(11, weight: 700)).tracking(11 * 0.13)
                        .foregroundStyle(skipped == nil ? Theme.label : Theme.dim)
                        .overlay { if skipped != nil { PencilStroke().pencil().frame(height: 5).offset(y: 1) } }
                    if let skipped {
                        Text("— \(skipped)").font(Theme.serif(13, italic: true)).foregroundStyle(Theme.dim).padding(.leading, 10)
                    }
                    if edited { EditDot().padding(.horizontal, 8).padding(.top, 2) }
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, Theme.Size.gutter)
                .frame(height: Theme.Size.sectionHead - 8)
                .padding(.top, 8)
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
            } else {
                Color.clear.frame(height: 8)
            }
            content
            Color.clear.frame(height: 9)
            InkRule()
        }
    }
}

/// A module (Grain, Halation, …): one 37 pt line that folds open, with a
/// summary value (or "Off") and its own tick box; a switched-off module
/// still opens, its rows faded and unusable (§4.9).
struct InkModule<Content: View>: View {
    let title: String
    var help: String? = nil
    /// The module's on/off switch, if it has one.
    var enabled: Binding<Bool>? = nil
    let summary: String?
    var edited = false
    /// Scanning the film skips it: struck through, its body hidden.
    var skipped = false
    /// Whether a rule separates it from what's above.
    var ruled = true
    @Binding var open: Bool
    /// "N more controls…", and what showing them does.
    var more = 0
    var showMore: () -> Void = {}
    @ViewBuilder let content: Content
    @State private var hovered = false

    private var off: Bool { enabled.map { !$0.wrappedValue } ?? false }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if ruled { InkRule() }
            HStack(spacing: 0) {
                Text("›").font(Theme.serif(17)).foregroundStyle(hovered ? Theme.label : Theme.dim)
                    .rotationEffect(.degrees(open && !skipped ? 90 : 0)).offset(x: open && !skipped ? 2 : 0)
                    .frame(width: 15, alignment: .leading)
                    .padding(.trailing, 4)
                Text(title)
                    .font(Theme.serif(16, weight: off || skipped ? 400 : 600))
                    .foregroundStyle(off || skipped ? Theme.dim : Theme.paper)
                    .overlay { if skipped { PencilStroke().pencil().frame(height: 5).offset(y: 1) } }
                    .tooltip(help)
                Spacer(minLength: 8)
                if edited { EditDot().padding(.trailing, 8) }
                if let summary, !open || skipped {
                    Text(off ? "Off" : summary)
                        .font(Theme.serif(14, italic: off))
                        .foregroundStyle(edited ? Theme.paper : Theme.dim)
                        .padding(.trailing, 11)
                }
                if let enabled {
                    TickBox(on: enabled.wrappedValue)
                        .contentShape(Rectangle().inset(by: -8))
                        .onTapGesture { enabled.wrappedValue.toggle() }
                        .opacity(skipped ? 0.4 : 1)
                        .help("Turns \(title) on or off, keeping its settings.")
                        .accessibilityRepresentation { Toggle("\(title) on", isOn: enabled) }
                }
            }
            .padding(.horizontal, Theme.Size.gutter)
            .frame(height: Theme.Size.moduleRow)
            .contentShape(Rectangle())
            .onHover { hovered = $0 }
            .onTapGesture { if !skipped { open.toggle() } }
            .inkFocus { if !skipped { open.toggle() } }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(title)
            .accessibilityValue(open ? "Open" : "Closed")
            if open && !skipped {
                VStack(alignment: .leading, spacing: 0) {
                    content.inkDisabled(off)
                    if more > 0 { MoreLine(text: "\(more) more control\(more == 1 ? "" : "s")", action: showMore) }
                }
                .padding(.bottom, 4)
            }
        }
    }
}

/// "N more controls…" in italic `dim`; a click shows them in place.
struct MoreLine: View {
    let text: String
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text(text + "…").font(Theme.serif(13, italic: true)).foregroundStyle(hovered ? Theme.label : Theme.dim)
        }
        .buttonStyle(InkPressStyle())
        .onHover { hovered = $0 }
        .inkFocus(activate: action)
        .padding(.horizontal, Theme.Size.gutter)
        .padding(.bottom, 8)
        .padding(.top, 2)
        .help("Shows them here. View › Show All Controls shows every control.")
    }
}
