import AppKit

// The title bar (§3): 38 pt of `bg` across the window, holding only the
// traffic lights and the file name, in Newsreader 15 600 `paper`. There's
// no toolbar; its items have homes of their own (§5.6).

/// The window's content: the title band over the split view. The columns,
/// the grid's sheet and the alert's dimming all start under the band.
@MainActor final class EditorFrame: NSViewController {
    let split: NSSplitViewController
    let band = TitleBand()

    init(split: NSSplitViewController, size: NSSize) {
        self.split = split
        super.init(nibName: nil, bundle: nil)
        let view = EditorFrameView(band: band, content: split.view)
        view.frame = NSRect(origin: .zero, size: size)
        self.view = view
        addChild(split)
        view.addSubview(split.view)
        view.addSubview(band)
        view.place()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
}

final class EditorFrameView: NSView {
    let band: TitleBand
    let content: NSView

    init(band: TitleBand, content: NSView) {
        self.band = band
        self.content = content
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = Theme.NS.bg.cgColor
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    // The band takes no clicks: they go to the window's title bar under
    // it, which drags and zooms the window.
    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit === self || hit === band ? nil : hit
    }

    // Placed as the frame changes, not on the next layout pass, so the
    // columns never have a moment at the wrong size.
    override func resizeSubviews(withOldSize oldSize: NSSize) { place() }

    func place() {
        let height = min(Theme.Size.titleBar, bounds.height)
        band.frame = NSRect(x: 0, y: bounds.height - height, width: bounds.width, height: height)
        content.frame = NSRect(x: 0, y: 0, width: bounds.width, height: bounds.height - height)
    }
}

/// The band behind the traffic lights, with the file name beside them. It
/// takes no clicks, so the window drags and zooms from it as from any title
/// bar.
final class TitleBand: NSView {
    let name = NSTextField(labelWithString: "Negs")
    /// Between the zoom button and the name (`.lights`' 14 pt padding).
    static let gap: CGFloat = 14
    /// Where the name starts in full screen, with no traffic lights.
    static let fullScreenInset: CGFloat = Theme.Size.gutter

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = Theme.NS.bg.cgColor
        name.font = Theme.serifFont(15, weight: 600) as NSFont
        name.textColor = Theme.NS.paper
        name.lineBreakMode = .byTruncatingMiddle
        name.cell?.truncatesLastVisibleLine = true
        // The window's title says it; VoiceOver reads that.
        name.setAccessibilityElement(false)
        addSubview(name)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    var title: String {
        get { name.stringValue }
        set { guard newValue != name.stringValue else { return }; name.stringValue = newValue; needsLayout = true }
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override var mouseDownCanMoveWindow: Bool { true }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsLayout = true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        needsLayout = true
    }

    override func layout() {
        super.layout()
        // From the text itself: the label's own size can lag a new name.
        let text = name.attributedStringValue.size()
        let size = NSSize(width: ceil(text.width) + 4, height: ceil(text.height))
        // Level with the traffic lights, after the zoom button.
        var left = Self.fullScreenInset, middle = bounds.midY
        if let window, !window.styleMask.contains(.fullScreen), let zoom = window.standardWindowButton(.zoomButton),
           let close = window.standardWindowButton(.closeButton), zoom.superview != nil {
            left = convert(zoom.bounds, from: zoom).maxX + Self.gap
            middle = convert(close.bounds, from: close).midY
        }
        let width = max(min(size.width, bounds.width - left - Theme.Size.gutter), 0)
        name.frame = NSRect(x: left.rounded(), y: (middle - size.height / 2).rounded(), width: width, height: size.height)
    }
}
