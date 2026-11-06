import AppKit
import SwiftUI

/// The folder as a strip under the photograph: its photographs in a row on black.
enum FilmRollLayout {
    /// The strip's height, until it's dragged.
    static let defaultStripHeight: CGFloat = 180
    static let stripHeightRange: ClosedRange<CGFloat> = 90...300
    /// Black between the photographs.
    static let frameGap: CGFloat = 6

    /// How long one frame is along the strip: a 3:2 photograph and its gap, in
    /// whole points so frames never drift.
    static func frameLength(stripHeight: CGFloat) -> CGFloat {
        ((stripHeight - frameGap) * 3 / 2).rounded() + frameGap
    }

    /// Where the photograph shows in a frame of `frame`.
    static func gate(frame: CGSize) -> CGRect {
        CGRect(origin: .zero, size: frame).insetBy(dx: frameGap / 2, dy: frameGap / 2)
    }

    /// The photograph at `point` on the film, from the film's top left. Only
    /// the gate counts: the film around it opens nothing.
    static func photo(at point: CGPoint, frame: CGSize, gate: CGRect, photos: Int) -> Int? {
        guard frame.width > 0, point.x >= 0 else { return nil }
        let index = Int(point.x / frame.width)
        let inFrame = CGPoint(x: point.x - CGFloat(index) * frame.width, y: point.y)
        return index < photos && gate.contains(inFrame) ? index : nil
    }

    /// The open photograph is at full brightness; the rest dim to half, over black.
    static func photographOpacity(selected: Bool) -> Double { selected ? 1 : 0.5 }

    /// Where the strip has to go to show a frame, or nil if any of it is already
    /// in view: only the person moves the film, unless the open photograph
    /// would otherwise be nowhere to be seen. Then it comes just into view.
    static func revealOffset(frame index: Int, frameLength: CGFloat, offset: CGFloat, viewport: CGFloat, maxOffset: CGFloat) -> CGFloat? {
        let start = CGFloat(index) * frameLength, end = start + frameLength
        if end > offset, start < offset + viewport { return nil }
        return min(max(start < offset ? start : end - viewport, 0), maxOffset)
    }

}

/// Moves the strip with a mouse drag, and lets it coast on after a flick, as
/// macOS scrolling does. The strip's scroll view stays its own for the trackpad.
@MainActor final class FilmRollScroller {
    weak var scroll: NSScrollView? {
        didSet {
            if let observing { NotificationCenter.default.removeObserver(observing); self.observing = nil }
            if let clip = scroll?.contentView {
                clip.postsBoundsChangedNotifications = true
                observing = NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: clip, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.onScroll?() }
                }
                onScroll?()
            }
            if scroll != nil, let pendingReveal { reveal(frame: pendingReveal.index, length: pendingReveal.length) }
        }
    }
    /// Whenever the film moves, by any means.
    var onScroll: (() -> Void)? { didSet { if scroll != nil { onScroll?() } } }
    private var observing: NSObjectProtocol?
    /// A frame to show once the scroll view has been found.
    private var pendingReveal: (index: Int, length: CGFloat)?
    /// macOS's normal deceleration: the speed kept after each millisecond.
    static let deceleration = 0.998
    /// Slower than this, in points a second, the strip has stopped.
    static let stopSpeed = 8.0
    /// How far back a release looks for its speed, and how long a pause before
    /// it means there was no flick.
    static let sampleWindow = 0.1, pauseLimit = 0.05

    private var startOffset: CGFloat?
    private var samples: [(time: TimeInterval, x: CGFloat)] = []
    private(set) var velocity = 0.0
    private var timer: Timer?
    private var lastTick: TimeInterval = 0

    var offset: CGFloat {
        get { scroll?.contentView.bounds.origin.x ?? 0 }
        set {
            guard let scroll else { return }
            let clip = scroll.contentView
            clip.scroll(to: NSPoint(x: min(max(newValue, 0), maxOffset), y: clip.bounds.origin.y))
            scroll.reflectScrolledClipView(clip)
        }
    }
    private var maxOffset: CGFloat {
        guard let scroll, let document = scroll.documentView else { return 0 }
        return max(0, document.frame.width - scroll.contentView.bounds.width)
    }
    var isCoasting: Bool { velocity != 0 }

    /// Brings a frame into view if it's wholly out of it, and otherwise leaves the film alone.
    func reveal(frame index: Int, length: CGFloat) {
        guard let scroll else { pendingReveal = (index, length); return }
        pendingReveal = nil
        guard let target = FilmRollLayout.revealOffset(frame: index, frameLength: length, offset: offset,
                                                       viewport: scroll.contentView.bounds.width, maxOffset: maxOffset) else { return }
        stop()
        offset = target
    }

    /// The mouse went down: a coasting strip stops under it.
    func press() {
        stop()
        startOffset = nil
        samples = []
    }

    /// The film follows the pointer, `translation` from where the drag began.
    func drag(translation: CGFloat, time: TimeInterval) {
        if startOffset == nil { startOffset = offset; velocity = 0 }
        offset = (startOffset ?? 0) - translation
        samples.append((time, translation))
        samples.removeAll { time - $0.time > Self.sampleWindow }
    }

    /// The mouse came up. The film keeps the speed it had, if it was moving.
    func release(time: TimeInterval, animate: Bool = true) {
        defer { startOffset = nil; samples = [] }
        guard startOffset != nil, let first = samples.first, let last = samples.last,
              time - last.time <= Self.pauseLimit, last.time > first.time else { return }
        // The film moves against the pointer's offset.
        velocity = -Double(last.x - first.x) / (last.time - first.time)
        guard abs(velocity) >= Self.stopSpeed else { velocity = 0; return }
        if animate { startTimer() }
    }

    /// Coasts `seconds` further: the speed falls away, and the ends of the roll stop it.
    func advance(seconds: TimeInterval) {
        guard velocity != 0 else { return }
        let wanted = offset + CGFloat(velocity * seconds)
        offset = wanted
        velocity *= pow(Self.deceleration, seconds * 1000)
        if abs(velocity) < Self.stopSpeed || wanted <= 0 || wanted >= maxOffset { stop() }
    }

    func stop() {
        velocity = 0
        timer?.invalidate()
        timer = nil
    }

    private func startTimer() {
        timer?.invalidate()
        lastTick = ProcessInfo.processInfo.systemUptime
        let timer = Timer(timeInterval: 1.0 / 120, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let now = ProcessInfo.processInfo.systemUptime
                self.advance(seconds: now - self.lastTick)
                self.lastTick = now
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }
}

/// The strip's top edge, which resizes it. The window's coordinates stay put
/// while the strip changes height, so a drag is measured against them.
struct StripResizeHandle: NSViewRepresentable {
    let height: CGFloat
    let resize: (CGFloat) -> Void

    func makeNSView(context: Context) -> Handle { Handle() }
    func updateNSView(_ view: Handle, context: Context) {
        view.height = height
        view.resize = resize
    }

    final class Handle: NSView {
        var height: CGFloat = 0
        var resize: ((CGFloat) -> Void)?
        private var grab: (y: CGFloat, height: CGFloat)?

        override func isAccessibilityElement() -> Bool { false }
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeUpDown) }
        override func mouseDown(with event: NSEvent) { grab = (event.locationInWindow.y, height) }
        override func mouseDragged(with event: NSEvent) {
            guard let grab else { return }
            resize?(grab.height + event.locationInWindow.y - grab.y)
        }
        override func mouseUp(with event: NSEvent) { grab = nil }
    }
}

/// Finds the scroll view of the strip it sits in.
struct EnclosingScrollFinder: NSViewRepresentable {
    let scroller: FilmRollScroller
    func makeNSView(context: Context) -> NSView { NSView() }
    func updateNSView(_ view: NSView, context: Context) {
        DispatchQueue.main.async { [weak view, scroller] in
            if let scroll = view?.enclosingScrollView { scroller.scroll = scroll }
        }
    }
}

/// Takes the mouse over the strip: a drag scrolls and flicks the film, and a
/// double-click reports where it was on the film. Scrolling passes straight
/// to the scroll view, so a trackpad keeps its own feel.
struct FilmMouseSurface: NSViewRepresentable {
    let scroller: FilmRollScroller
    let enabled: Bool
    /// A double-click that didn't drag, as a point on the film from its top left.
    let open: (CGPoint) -> Void

    func makeNSView(context: Context) -> Surface { Surface() }
    func updateNSView(_ view: Surface, context: Context) {
        view.scroller = scroller
        view.enabled = enabled
        view.open = open
    }

    final class Surface: NSView {
        var scroller: FilmRollScroller?
        var enabled = true
        var open: ((CGPoint) -> Void)?
        /// A little wobble while clicking isn't a drag.
        static let wobble: CGFloat = 3
        private var pressX: CGFloat?
        private var dragged = false

        override var isFlipped: Bool { true }
        override func isAccessibilityElement() -> Bool { false }
        override func hitTest(_ point: NSPoint) -> NSView? { enabled ? super.hitTest(point) : nil }
        // A click in the strip shouldn't have to activate the window first.
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

        override func mouseDown(with event: NSEvent) {
            scroller?.press()
            pressX = event.locationInWindow.x
            dragged = false
        }

        override func mouseDragged(with event: NSEvent) {
            guard let pressX else { return }
            let translation = event.locationInWindow.x - pressX
            if !dragged, abs(translation) < Self.wobble { return }
            dragged = true
            scroller?.drag(translation: translation, time: event.timestamp)
        }

        override func mouseUp(with event: NSEvent) {
            defer { pressX = nil; dragged = false }
            if dragged {
                scroller?.release(time: event.timestamp)
            } else if event.clickCount == 2, let scroller {
                let point = convert(event.locationInWindow, from: nil)
                open?(CGPoint(x: point.x + scroller.offset, y: point.y))
            }
        }

        override func scrollWheel(with event: NSEvent) {
            scroller?.stop()
            if let scroll = scroller?.scroll { scroll.scrollWheel(with: event) } else { super.scrollWheel(with: event) }
        }
    }
}

/// One frame of the strip: the photograph, on black, never turned.
struct FilmFrame: View {
    let height: CGFloat
    let file: PhotoFile
    let selected: Bool
    let open: () -> Void
    @State private var embedded: CGImage?
    @State private var visible = false

    var body: some View {
        let frame = CGSize(width: FilmRollLayout.frameLength(stripHeight: height), height: height)
        let gate = FilmRollLayout.gate(frame: frame)
        ZStack {
            Color.black
            if let image = embedded {
                Image(decorative: image, scale: 1)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: gate.width, height: gate.height)
                    .opacity(FilmRollLayout.photographOpacity(selected: selected))
            }
        }
        .frame(width: frame.width, height: frame.height)
        .accessibilityElement()
        .accessibilityLabel(file.name)
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : [.isButton])
        .accessibilityAction(.default, open)
        .onAppear { visible = true }
        .onDisappear { visible = false; embedded = nil }
        .task(id: visible ? file : nil) {
            guard visible else { return }
            let request = Task.detached(priority: .utility) { await PhotoThumbnails.shared.image(for: file) }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            embedded = result
        }
    }
}
