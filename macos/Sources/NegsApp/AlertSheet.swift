import AppKit
import Observation
import SwiftUI

// The alert sheet (§4.14): for what isn't about the photograph on screen. It
// drops from under the title bar in the inspector's colours, 440 wide, and
// the window behind it dims. It replaces `NSAlert`, whose default button
// takes the system blue. Return is the default button, Esc the Cancel one.

/// What an alert says and offers.
struct AppAlert: Equatable {
    struct Button: Equatable {
        enum Role: Equatable { case plain, cancel, destructive }
        var title: String
        var role = Role.plain
    }

    struct Choice: Equatable {
        var title: String
        var on: Bool
    }

    var title: String
    var detail: String?
    /// As `NSAlert` takes them: the first is the default (Return) and sits
    /// at the right, the rest follow it leftwards.
    var buttons = [Button(title: "OK")]
    /// Tick boxes between the words and the buttons.
    var choices: [Choice] = []

    /// Return's button.
    var defaultIndex: Int { 0 }
    /// Esc's button: the Cancel one, or the only one there is. A question
    /// with neither stays until it's answered.
    var cancelIndex: Int? {
        buttons.firstIndex { $0.role == .cancel } ?? (buttons.count == 1 ? 0 : nil)
    }

    /// One of the app's error messages: its first sentence is the heading,
    /// the rest the detail. "Could not save the editing session. Negs
    /// stayed open." reads "Could not save the editing session" over
    /// "Negs stayed open."
    static func error(_ message: String) -> AppAlert {
        let text = message.trimmingCharacters(in: .whitespacesAndNewlines)
        if let end = text.range(of: ". ") {
            let rest = text[end.upperBound...].trimmingCharacters(in: .whitespaces)
            if !rest.isEmpty { return AppAlert(title: String(text[..<end.lowerBound]), detail: rest) }
        }
        return AppAlert(title: text)
    }
}

/// How an alert was answered: the button, and the tick boxes as they were left.
struct AlertAnswer: Equatable {
    var button: Int
    var choices: [Bool] = []
}

/// One alert on its way to an answer. Its tick boxes change as they're clicked.
@MainActor @Observable final class AlertRequest: Identifiable {
    let alert: AppAlert
    var choices: [AppAlert.Choice]
    @ObservationIgnored private var finish: ((AlertAnswer) -> Void)?

    init(alert: AppAlert, finish: @escaping (AlertAnswer) -> Void) {
        self.alert = alert
        self.choices = alert.choices
        self.finish = finish
    }

    fileprivate func answer(_ button: Int) {
        let finish = self.finish
        self.finish = nil
        finish?(AlertAnswer(button: button, choices: choices.map(\.on)))
    }
}

/// The alerts waiting, shown one at a time in the order asked.
@MainActor @Observable final class AlertQueue {
    private(set) var shown: AlertRequest?
    @ObservationIgnored private var waiting: [AlertRequest] = []

    func ask(_ alert: AppAlert) async -> AlertAnswer {
        await withCheckedContinuation { continuation in
            let request = AlertRequest(alert: alert) { continuation.resume(returning: $0) }
            if shown == nil { shown = request } else { waiting.append(request) }
        }
    }

    /// Answers the alert shown with one of its buttons, and shows the next.
    func answer(_ button: Int) {
        guard let request = shown, request.alert.buttons.indices.contains(button) else { return }
        shown = waiting.isEmpty ? nil : waiting.removeFirst()
        request.answer(button)
    }
}

extension EditorModel {
    /// Asks in the alert sheet, after any alert already waiting.
    func ask(_ alert: AppAlert) async -> AlertAnswer { await alerts.ask(alert) }

    /// Says something in the alert sheet; there's nothing to choose.
    func tell(_ alert: AppAlert) { Task { _ = await alerts.ask(alert) } }

    /// The alert on screen: a question first, then an error.
    var alert: AlertRequest? { alerts.shown ?? errorAlert }

    /// Answers the alert on screen, as its buttons and keys do.
    func answerAlert(_ button: Int) {
        if alerts.shown != nil { alerts.answer(button) } else if errorAlert != nil { error = nil }
    }

    /// Return: the default button.
    func confirmAlert() { if let alert { answerAlert(alert.alert.defaultIndex) } }

    /// Esc: the Cancel button, if there is one to take. False if not.
    @discardableResult func dismissAlert() -> Bool {
        guard let index = alert?.alert.cancelIndex else { return false }
        answerAlert(index)
        return true
    }
}

// MARK: - The sheet

/// The alert as drawn: heading in Newsreader 20 600, the detail in italic 16
/// `label`, tick boxes if any, and the buttons outlined at the right. In
/// `panel` with a 1 pt `line` border that leaves the top open, the bottom
/// corners rounded 10, and a deep shadow, which the window leaves room for.
struct AlertSheet: View {
    let request: AlertRequest
    let answer: (Int) -> Void

    static let width: CGFloat = 440
    /// Room round the sheet for its shadow, which a window can't draw past.
    static let margin = EdgeInsets(top: 0, leading: 44, bottom: 56, trailing: 44)

    var body: some View {
        let alert = request.alert
        VStack(alignment: .leading, spacing: 0) {
            Text(alert.title)
                .font(Theme.serif(20, weight: 600))
                .foregroundStyle(Theme.paper)
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
            if let detail = alert.detail {
                Text(detail)
                    .font(Theme.serif(16, italic: true))
                    .foregroundStyle(Theme.label)
                    .lineSpacing(4)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 8)
            }
            if !request.choices.isEmpty {
                VStack(alignment: .leading, spacing: 9) {
                    ForEach(request.choices.indices, id: \.self) { index in
                        AlertChoice(choice: Binding(get: { request.choices[index] }, set: { request.choices[index] = $0 }))
                    }
                }
                .padding(.top, 16)
            }
            HStack(spacing: 10) {
                Spacer(minLength: 0)
                ForEach(alert.buttons.indices.reversed(), id: \.self) { index in
                    Button { answer(index) } label: {
                        Text(alert.buttons[index].title).frame(minWidth: 52)
                    }
                    .buttonStyle(OutlinedButtonStyle(prominent: index == alert.defaultIndex))
                    .accessibilityHint(index == alert.defaultIndex ? "Return" : index == alert.cancelIndex ? "Escape" : "")
                }
            }
            .padding(.top, 20)
        }
        .padding(EdgeInsets(top: 22, leading: 26, bottom: 20, trailing: 26))
        .frame(width: Self.width, alignment: .leading)
        .background(UnevenRoundedRectangle(bottomLeadingRadius: 10, bottomTrailingRadius: 10).fill(Theme.panel))
        .overlay(OpenTopBorder(radius: 10).stroke(Theme.line, lineWidth: 1))
        .compositingGroup()
        .shadow(color: .black.opacity(0.7), radius: 25, y: 18)
        .shadow(color: .black.opacity(0.5), radius: 5, y: 3)
        .padding(Self.margin)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(alert.title)
    }
}

/// The sheet's border down its sides and round its bottom, open at the top
/// where it meets the title bar.
private struct OpenTopBorder: Shape {
    let radius: CGFloat

    func path(in rect: CGRect) -> Path {
        let rect = rect.insetBy(dx: 0.5, dy: 0)
        var path = Path()
        path.move(to: CGPoint(x: rect.minX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY - radius))
        path.addArc(tangent1End: CGPoint(x: rect.minX, y: rect.maxY - 0.5), tangent2End: CGPoint(x: rect.maxX, y: rect.maxY - 0.5), radius: radius)
        path.addArc(tangent1End: CGPoint(x: rect.maxX, y: rect.maxY - 0.5), tangent2End: CGPoint(x: rect.maxX, y: rect.minY), radius: radius)
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
        return path
    }
}

/// A tick box and its words, as in the inspector.
private struct AlertChoice: View {
    @Binding var choice: AppAlert.Choice
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 9) {
            TickBox(on: choice.on, hovered: hovered)
            Text(choice.title).font(Theme.ui(13)).foregroundStyle(hovered ? Theme.paper : Theme.label)
        }
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture { choice.on.toggle() }
        .accessibilityRepresentation { Toggle(choice.title, isOn: $choice.on) }
    }
}

// MARK: - Presenting it

/// What covers the window while an alert shows: the dimming below the title
/// bar, and the sheet hanging from it. It takes the clicks meant for what's
/// under it; the window controller holds back its keys (`alertKeys`).
final class AlertCover: NSView {
    let dim = NSView()

    override init(frame: NSRect) {
        super.init(frame: frame)
        dim.wantsLayer = true
        dim.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.28).cgColor
        addSubview(dim)
        setAccessibilityElement(false)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit === dim ? self : hit
    }

    override func mouseDown(with event: NSEvent) {}
    override func rightMouseDown(with event: NSEvent) {}
    override func otherMouseDown(with event: NSEvent) {}
    override func scrollWheel(with event: NSEvent) {}
    override func resetCursorRects() { addCursorRect(bounds, cursor: .arrow) }
}

/// The sheet's window when the editing window is hidden (removing the app
/// data): it stands alone. Return and Esc answer it whatever has the focus.
final class AlertPanel: NSPanel {
    var keys: (UInt16) -> Bool = { _ in false }

    init() {
        super.init(contentRect: .zero, styleMask: [.borderless], backing: .buffered, defer: true)
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        isReleasedWhenClosed = false
        appearance = NSAppearance(named: .darkAqua)
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    override func sendEvent(_ event: NSEvent) {
        if event.type == .keyDown, event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
           [36, 76, 53].contains(event.keyCode), keys(event.keyCode) { return }
        super.sendEvent(event)
    }
}

/// Shows the model's alert over the editing window, made the first time
/// it's needed: the window dims below the title bar and the sheet drops
/// from under it. The keyboard's focus moves into the sheet and comes back
/// after. While the window is hidden the sheet stands alone instead.
@MainActor final class AlertPresenter {
    private let model: EditorModel
    private weak var window: NSWindow?
    /// The view the cover lies over: the whole window.
    private let root: NSView
    private var cover: AlertCover?
    private var host: NSHostingView<AlertSheet>?
    private var panel: AlertPanel?
    private var shown: AlertRequest?
    /// What had the keys before the sheet.
    private weak var returnFocus: NSResponder?

    init(model: EditorModel, window: NSWindow, root: NSView) {
        self.model = model
        self.window = window
        self.root = root
    }

    /// The sheet, if it's on the window now; for tests and the key monitor.
    var onWindow: Bool { cover?.superview != nil }

    func show(_ request: AlertRequest?) {
        guard request !== shown else { return }
        let replacing = shown != nil
        shown = request
        guard let request else { return close() }
        let view = AlertSheet(request: request) { [weak model] in model?.answerAlert($0) }
        let host = self.host ?? NSHostingView(rootView: view)
        host.rootView = view
        self.host = host
        let size = host.fittingSize
        host.setAccessibilityLabel(request.alert.title)
        if let window, window.isVisible {
            showOnWindow(window, host: host, size: size, sliding: !replacing)
        } else {
            showAlone(host: host, size: size)
        }
        NSAccessibility.post(element: host, notification: .layoutChanged)
        let words = [request.alert.title, request.alert.detail].compactMap { $0 }.joined(separator: ". ")
        NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested,
                             userInfo: [.announcement: "Alert. " + words, .priority: NSAccessibilityPriorityLevel.high.rawValue])
    }

    private func showOnWindow(_ window: NSWindow, host: NSHostingView<AlertSheet>, size: NSSize, sliding: Bool) {
        let cover = self.cover ?? {
            let cover = AlertCover(frame: root.bounds)
            cover.autoresizingMask = [.width, .height]
            self.cover = cover
            return cover
        }()
        let fresh = cover.superview == nil
        if fresh {
            cover.frame = root.bounds
            // Over everything, the grid's sheet too.
            root.addSubview(cover, positioned: .above, relativeTo: nil)
            returnFocus = window.firstResponder
        }
        let below = cover.convert(window.contentLayoutRect, from: nil)
        cover.dim.frame = below
        cover.dim.autoresizingMask = [.width, .height]
        if host.superview !== cover { cover.addSubview(host) }
        host.autoresizingMask = [.minXMargin, .maxXMargin, .minYMargin]
        let frame = NSRect(x: ((cover.bounds.width - size.width) / 2).rounded(), y: below.maxY - size.height,
                           width: size.width, height: size.height)
        window.standardWindowButton(.closeButton)?.isEnabled = false
        window.makeFirstResponder(host)
        guard fresh || sliding else { host.frame = frame; return }
        let still = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        host.frame = still ? frame : frame.offsetBy(dx: 0, dy: size.height)
        host.alphaValue = still ? 0 : 1
        cover.dim.alphaValue = 0
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            context.timingFunction = CAMediaTimingFunction(name: .easeOut)
            host.animator().frame = frame
            host.animator().alphaValue = 1
            cover.dim.animator().alphaValue = 1
        }
    }

    private func showAlone(host: NSHostingView<AlertSheet>, size: NSSize) {
        let panel = self.panel ?? {
            let panel = AlertPanel()
            panel.keys = { [weak model] key in
                guard let model else { return false }
                if key == 53 { return model.dismissAlert() }
                model.confirmAlert()
                return true
            }
            self.panel = panel
            return panel
        }()
        host.removeFromSuperview()
        host.frame = NSRect(origin: .zero, size: size)
        host.alphaValue = 1
        panel.contentView = host
        panel.setContentSize(size)
        if !panel.isVisible { panel.center() }
        NSApp.activate()
        panel.makeKeyAndOrderFront(nil)
    }

    private func close() {
        panel?.orderOut(nil)
        guard let cover, cover.superview != nil, let host else { return }
        window?.standardWindowButton(.closeButton)?.isEnabled = true
        if let window, window.firstResponder === host || (window.firstResponder as? NSView)?.isDescendant(of: host) == true {
            window.makeFirstResponder(returnFocus.flatMap { ($0 as? NSView)?.window === window ? $0 : nil })
        }
        let still = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        NSAnimationContext.runAnimationGroup({ context in
            context.duration = 0.15
            if still { host.animator().alphaValue = 0 } else { host.animator().frame = host.frame.offsetBy(dx: 0, dy: host.frame.height) }
            cover.dim.animator().alphaValue = 0
        }) { [weak self] in
            MainActor.assumeIsolated {
                // Unless another alert came meanwhile.
                guard let self, self.shown == nil else { return }
                cover.removeFromSuperview()
            }
        }
    }
}
