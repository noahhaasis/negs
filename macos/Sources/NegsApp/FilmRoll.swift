import AppKit
import SwiftUI

/// The folder as a roll of 35mm film under the photograph, running across as
/// the strip artwork does.
enum FilmRollLayout {
    /// The strip's height: the film and nothing else.
    static let stripHeight: CGFloat = 180
    /// The artwork's film base, for frames before it has loaded.
    static let filmBase = Color(red: 15.0 / 255, green: 13.0 / 255, blue: 12.0 / 255)
    /// Until the artwork loads: its proportions and gate, from `frames.json`.
    static let defaultTexture = CGSize(width: 3800, height: 3500)
    static let defaultWindow = CGRect(x: 0.026316, y: 0.157143, width: 0.947368, height: 0.685714)

    /// How long one frame is along the roll. Whole points, so nothing shows between frames.
    static func frameLength(stripHeight: CGFloat, texture: CGSize) -> CGFloat {
        (stripHeight * texture.width / texture.height).rounded()
    }

    /// Where the photograph shows, in a frame of `frame`, from the artwork's
    /// `window` (fractions of it, from its top left).
    static func gate(window: CGRect, frame: CGSize) -> CGRect {
        CGRect(x: window.minX * frame.width, y: window.minY * frame.height,
               width: window.width * frame.width, height: window.height * frame.height)
    }

    /// A portrait photograph lies on its side on a roll running across, as on
    /// real film; a landscape one sits upright.
    static func liesOnItsSide(_ image: CGSize) -> Bool { image.height > image.width }

    /// How many blank frames keep the film going to the end of a strip
    /// `viewport` long, after `photos` frames.
    static func blankFrames(photos: Int, viewport: CGFloat, frameLength: CGFloat) -> Int {
        guard frameLength > 0 else { return 0 }
        return max(0, Int((viewport / frameLength).rounded(.up)) - photos)
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

    /// Still films print a frame number, and the artwork's is always the same
    /// one. Cine and print films have other text in those places.
    static func printsFrameNumbers(film: String) -> Bool {
        !(film.hasPrefix("kodak_vision3") || ["kodak_doublex", "kodak_2302", "kodak_2383", "kodak_2393", "kodak_verita_200d"].contains(film))
    }
}

/// Moves the strip with a mouse drag, and lets it coast on after a flick, as
/// macOS scrolling does. The strip's scroll view stays its own for the trackpad.
@MainActor final class FilmRollScroller {
    weak var scroll: NSScrollView? {
        didSet { if scroll != nil, let pendingReveal { reveal(frame: pendingReveal.index, length: pendingReveal.length) } }
    }
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

/// One frame of the roll: the photograph, with the strip's artwork over it.
struct FilmFrame: View {
    let file: PhotoFile
    /// Its place on the roll, from 1.
    let number: Int
    let selected: Bool
    let strip: FilmStrip?
    let film: String
    let open: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    var body: some View {
        let frame = FilmArtwork.size(strip)
        let gate = FilmRollLayout.gate(window: strip?.window ?? FilmRollLayout.defaultWindow, frame: frame)
        ZStack(alignment: .topLeading) {
            // Solid film, so nothing behind the roll shows round the gate.
            FilmRollLayout.filmBase
            // `FilmMouseSurface` takes the mouse; this is for assistive tools.
            photograph(in: gate)
                .offset(x: gate.minX, y: gate.minY)
                .accessibilityElement()
                .accessibilityLabel(file.name)
                .accessibilityAddTraits(selected ? [.isButton, .isSelected] : [.isButton])
                .accessibilityAction(.default, open)
            FilmArtwork(number: number, strip: strip, film: film)
                .allowsHitTesting(false)
        }
        .frame(width: frame.width, height: frame.height, alignment: .topLeading)
        .clipped()
        .onAppear { visible = true }
        .onDisappear { visible = false; image = nil }
        .task(id: visible ? file : nil) {
            guard visible else { return }
            let request = Task.detached(priority: .utility) { await PhotoThumbnails.shared.image(for: file) }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            image = result
        }
    }

    @ViewBuilder private func photograph(in gate: CGRect) -> some View {
        ZStack {
            Color.black
            if let image {
                let sideways = FilmRollLayout.liesOnItsSide(CGSize(width: image.width, height: image.height))
                Image(decorative: image, scale: 1)
                    .resizable()
                    .scaledToFill()
                    .frame(width: sideways ? gate.height : gate.width, height: sideways ? gate.width : gate.height)
                    .clipped()
                    .rotationEffect(.degrees(sideways ? -90 : 0))
                    .opacity(FilmRollLayout.photographOpacity(selected: selected))
            }
        }
        .frame(width: gate.width, height: gate.height)
        .clipped()
    }
}

/// A frame past the last photograph: the roll carries on, with nothing on it.
struct BlankFilmFrame: View {
    let number: Int
    let strip: FilmStrip?
    let film: String

    var body: some View {
        let frame = FilmArtwork.size(strip)
        let gate = FilmRollLayout.gate(window: strip?.window ?? FilmRollLayout.defaultWindow, frame: frame)
        ZStack(alignment: .topLeading) {
            FilmRollLayout.filmBase
            Color.black
                .frame(width: gate.width, height: gate.height)
                .offset(x: gate.minX, y: gate.minY)
            FilmArtwork(number: number, strip: strip, film: film)
                .allowsHitTesting(false)
        }
        .frame(width: frame.width, height: frame.height, alignment: .topLeading)
        .clipped()
        .accessibilityHidden(true)
    }
}

/// The strip's artwork for one frame of the roll.
private struct FilmArtwork: View {
    let number: Int
    let strip: FilmStrip?
    let film: String

    static func size(_ strip: FilmStrip?) -> CGSize {
        let height = FilmRollLayout.stripHeight
        return CGSize(width: FilmRollLayout.frameLength(stripHeight: height, texture: strip?.pixelSize ?? FilmRollLayout.defaultTexture), height: height)
    }

    var body: some View {
        let frame = Self.size(strip)
        if let strip {
            ZStack(alignment: .topLeading) {
                Image(decorative: strip.image, scale: 1)
                    .resizable()
                    .frame(width: frame.width, height: frame.height)
                if FilmRollLayout.printsFrameNumbers(film: film) {
                    numbers(length: frame.width)
                }
            }
            .frame(width: frame.width, height: frame.height)
        } else {
            // Film with its gate cut out, until the artwork loads.
            let gate = FilmRollLayout.gate(window: FilmRollLayout.defaultWindow, frame: frame)
            Rectangle()
                .fill(FilmRollLayout.filmBase)
                .reverseMask {
                    Rectangle().frame(width: gate.width, height: gate.height).position(x: gate.midX, y: gate.midY)
                }
                .frame(width: frame.width, height: frame.height)
        }
    }

    /// This frame's own number over the artwork's: in millimetres along and
    /// across the 38 × 35 mm strip, where `spektrafilm-io::film35` prints it.
    @ViewBuilder private func numbers(length: CGFloat) -> some View {
        let scale = length / 38
        let label = { (text: String, x: CGFloat, y: CGFloat) in
            Text(text)
                .font(.system(size: 1.2 * scale, weight: .semibold).width(.condensed))
                .foregroundStyle(Color(red: 0.97, green: 0.63, blue: 0.2))
                .fixedSize()
                .frame(width: 2.2 * scale, height: 1.4 * scale, alignment: .leading)
                .background(FilmRollLayout.filmBase)
                .offset(x: x * scale, y: y * scale - 0.7 * scale)
        }
        label("\(number)", 2.3, 1.1)
        label("\(number)", 2.3, 33.95)
        label("\(number)A", 21.1, 33.95)
    }
}

private extension View {
    /// Cuts `mask` out of this view.
    func reverseMask<Mask: View>(@ViewBuilder _ mask: () -> Mask) -> some View {
        self.mask {
            Rectangle().overlay { mask().blendMode(.destinationOut) }.compositingGroup()
        }
    }
}
