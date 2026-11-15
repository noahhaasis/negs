import AppKit
import QuartzCore
import SwiftUI

/// Where the strip's frames and the grid's thumbnails are drawn, in their
/// hosting views (top left origin), and the picture each shows: where a
/// flight between them starts and ends. Written by the views, read only
/// by `SheetFlight`, so it isn't observed.
@MainActor final class ThumbnailPlaces {
    struct Place {
        var rect: CGRect
        var image: CGImage?
    }
    var strip: [URL: Place] = [:]
    var sheet: [URL: Place] = [:]
    /// The two compared photographs (ticket 46), side by side or in the slider.
    var compare: [URL: Place] = [:]
    /// Where they were when compare was left for the grid, which flies them
    /// from there; taken by the next opening.
    var leftCompare: [URL: CGRect] = [:]
    /// The coordinate space the places are measured in, named at the root of
    /// the canvas's and the sheet's hosting views. Not `.global`: that's the
    /// window's, which takes in the title bar above them, so the flights
    /// landed 38 pt low and the frames jumped up as they took over.
    static let space = "thumbnailPlaces"
}

/// Reports a thumbnail's rect and picture as they change, and nil once it's gone.
struct PlaceReport: ViewModifier {
    let image: CGImage?
    let placed: (ThumbnailPlaces.Place?) -> Void
    @State private var rect: CGRect?

    func body(content: Content) -> some View {
        content
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(ThumbnailPlaces.space)) } action: { rect in
                self.rect = rect
                placed(ThumbnailPlaces.Place(rect: rect, image: image))
            }
            .onChange(of: image.map(ObjectIdentifier.init)) { _, _ in
                if let rect { placed(ThumbnailPlaces.Place(rect: rect, image: image)) }
            }
            .onDisappear { placed(nil) }
    }
}

/// The grid's opening and folding (§4.6, `sheet.js`): the sheet grows from
/// the strip's band (the strip and the status line, the canvas only) to the
/// window, and each frame on screen in both travels from its place in the
/// strip to its place in the grid, 480 ms on `cubic-bezier(.22,.8,.24,1)`,
/// those nearer the current photograph first. Thumbnails not in the strip
/// fade up after. Folding plays it backwards. Core Animation layers over the
/// window carry the pictures; the views under them wait, hidden. Nothing
/// is built until the grid first opens, and Reduce Motion skips it.
@MainActor final class SheetFlight {
    static let duration: CFTimeInterval = 0.48
    static let timing = CAMediaTimingFunction(controlPoints: 0.22, 0.8, 0.24, 1)
    /// Each step from the current photograph starts 16 ms later, up to 12 steps.
    static let stagger: CFTimeInterval = 0.016
    static let staggerSteps = 12

    private let model: EditorModel
    /// The window's content (the split view's container), the canvas's
    /// hosting view, and the sheet's container and hosting view.
    private let root: NSView, canvas: NSView, container: NSView, sheet: NSView
    private let flight = FlightView()
    /// Bumped by each flight, so a stale one's callbacks do nothing.
    private var generation = 0

    init(model: EditorModel, root: NSView, canvas: NSView, container: NSView, sheet: NSView) {
        self.model = model
        self.root = root
        self.canvas = canvas
        self.container = container
        self.sheet = sheet
        flight.frame = root.bounds
        flight.autoresizingMask = [.width, .height]
        flight.wantsLayer = true
        root.addSubview(flight, positioned: .above, relativeTo: container)
    }

    static var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    /// The strip's band in the root's coordinates.
    private var band: CGRect {
        let height = (model.stripShown ? Theme.Size.strip : 0) + Theme.Size.statusLine
        let canvasRect = root.convert(canvas.bounds, from: canvas)
        return CGRect(x: canvasRect.minX, y: root.isFlipped ? canvasRect.maxY - height : canvasRect.minY,
                      width: canvasRect.width, height: height)
    }

    // MARK: Opening

    /// Shows the sheet; with motion, grows it out of the strip.
    func open() {
        generation += 1
        cancelFlight()
        model.sheetFolding = false
        container.isHidden = false
        let fromCompare = model.places.leftCompare
        model.places.leftCompare = [:]
        guard !Self.reduceMotion else {
            container.alphaValue = 1
            return
        }
        let generation = generation
        if !fromCompare.isEmpty {
            // From compare, which has no strip: the sheet covers its canvas
            // at once, so the photo view under it never shows, and widens
            // over the panel as the two fly back to their cells.
            let canvasRect = root.convert(canvas.bounds, from: canvas)
            container.alphaValue = 1
            mask(from: canvasRect, to: canvasRect)
            afterLayout { [weak self] in
                guard let self, self.generation == generation, self.model.mode == .grid else { return }
                let from = fromCompare.mapValues { ThumbnailPlaces.Place(rect: self.root.convert($0, from: self.canvas), image: nil) }
                self.flyOpen(generation, from: from, band: canvasRect, dimmed: false)
            }
            return
        }
        // Hidden until it's laid out on the current photograph.
        container.alphaValue = 0
        afterLayout { [weak self] in
            guard let self, self.generation == generation, self.model.mode == .grid else { return }
            self.flyOpen(generation, from: self.places(self.model.places.strip, in: self.canvas), band: self.band, dimmed: true)
        }
    }

    /// `from`: where each picture starts, in the root's coordinates; `band`:
    /// what the sheet grows from. `dimmed`: they start as the strip shows
    /// them, all but the current one dimmed.
    private func flyOpen(_ generation: Int, from starts: [URL: ThumbnailPlaces.Place], band: CGRect, dimmed: Bool) {
        let viewport = root.convert(sheet.bounds, from: sheet)
        let shown = model.shownPhotos.map(\.url)
        let current = model.selection.current
        let centre = current.flatMap { shown.firstIndex(of: $0) } ?? 0
        var order: [URL: Int] = [:]
        for (index, url) in shown.enumerated() { order[url] = index }
        let grid = places(model.places.sheet, in: sheet).filter { $0.value.rect.intersects(viewport) }
        let strip = starts.filter { $0.value.rect.intersects(band) }
        model.sheetWaiting = Set(grid.keys)

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        container.alphaValue = 1
        mask(from: band, to: root.bounds)
        var latest: CFTimeInterval = Self.duration
        for (url, to) in grid {
            let delay = Self.stagger * CFTimeInterval(min(abs((order[url] ?? centre) - centre), Self.staggerSteps))
            if let from = strip[url], let image = to.image ?? from.image {
                let opacity: Float = url == model.sourceItem || !dimmed ? 1 : Float(Theme.stripDim)
                let layer = flyer(image: image, from: from.rect, to: to.rect, opacity: (opacity, 1), delay: delay)
                latest = max(latest, delay + Self.duration)
                after(delay + Self.duration, generation) { [weak self] in
                    layer.removeFromSuperlayer()
                    self?.model.sheetWaiting.remove(url)
                }
            } else {
                // Not in the strip: fades up once the sheet is mostly open.
                after(Self.duration * 0.35 + delay, generation) { [weak self] in
                    withAnimation(.easeOut(duration: Self.duration * 0.55)) { _ = self?.model.sheetWaiting.remove(url) }
                }
            }
        }
        CATransaction.commit()
        after(latest + 0.02, generation) { [weak self] in
            self?.unmask()
            self?.model.sheetWaiting = []
        }
    }

    // MARK: Folding

    /// Folds the sheet back into the strip, then hides it. Into compare,
    /// which has no strip, the two photographs fly to their places instead.
    func fold() {
        generation += 1
        cancelFlight()
        let generation = generation
        guard !Self.reduceMotion, !container.isHidden else { return finishFold() }
        let viewport = root.convert(sheet.bounds, from: sheet)
        let grid = places(model.places.sheet, in: sheet).filter { $0.value.rect.intersects(viewport) }
        if let comparison = model.comparison {
            let compared = [comparison.first, comparison.second].compactMap { $0.isOriginal ? nil : $0.item }
            return afterCompareLaid(out: compared, generation, waited: 0) { [weak self] in self?.foldIntoCompare(grid: grid, generation) }
        }
        // The strip moves to the photograph opened; it's laid out under the sheet.
        afterLayout { [weak self] in
            guard let self, self.generation == generation else { return }
            let strip = self.places(self.model.places.strip, in: self.canvas).filter { $0.value.rect.intersects(self.band) }
            let carried = Set(grid.keys).intersection(strip.keys)
            self.model.sheetWaiting = carried
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            self.mask(from: self.root.bounds, to: self.band)
            for url in carried {
                guard let from = grid[url], let to = strip[url], let image = from.image ?? to.image else { continue }
                let opacity: Float = url == self.model.sourceItem ? 1 : Float(Theme.stripDim)
                _ = self.flyer(image: image, from: from.rect, to: to.rect, opacity: (1, opacity), delay: 0)
            }
            CATransaction.commit()
            // The rest fade out early.
            withAnimation(.easeOut(duration: Self.duration * 0.4)) {
                self.model.sheetWaiting = Set(grid.keys)
            }
            self.after(Self.duration, generation) { [weak self] in self?.finishFold() }
        }
    }

    /// The two fly from their cells to their places in compare as the sheet
    /// fades over it. Shrunk into the strip's band, it was a black bar that
    /// then vanished, as compare has no strip.
    private func foldIntoCompare(grid: [URL: ThumbnailPlaces.Place], _ generation: Int) {
        let compare = places(model.places.compare, in: canvas)
        let carried = Set(grid.keys).intersection(compare.keys)
        model.sheetWaiting = carried
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for url in carried {
            guard let from = grid[url], let to = compare[url], let image = from.image ?? to.image else { continue }
            _ = flyer(image: image, from: from.rect, to: to.rect, opacity: (1, 1), delay: 0)
        }
        CATransaction.commit()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Self.duration * 0.6
            context.timingFunction = CAMediaTimingFunction(name: .easeOut)
            container.animator().alphaValue = 0
        }
        after(Self.duration, generation) { [weak self] in self?.finishFold() }
    }

    /// Once both compared photographs have something to show, so they don't
    /// move after the flight lands; at most a few frames (the sheet still
    /// covers them).
    private func afterCompareLaid(out compared: [URL], _ generation: Int, waited: Int, _ work: @escaping @MainActor () -> Void) {
        afterLayout { [weak self] in
            guard let self, self.generation == generation else { return }
            let laid = compared.allSatisfy { self.model.places.compare[$0] != nil }
                && self.model.comparison.map { [$0.first, $0.second].allSatisfy { self.model.comparePicture($0) != nil } } == true
            if laid || waited >= 12 { return work() }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.016) {
                MainActor.assumeIsolated { self.afterCompareLaid(out: compared, generation, waited: waited + 1, work) }
            }
        }
    }

    private func finishFold() {
        cancelFlight()
        container.isHidden = true
        container.alphaValue = 1
        model.sheetWaiting = []
        model.sheetFolding = false
    }

    // MARK: Layers

    /// `places` (in `view`'s coordinates) in the root's.
    private func places(_ places: [URL: ThumbnailPlaces.Place], in view: NSView) -> [URL: ThumbnailPlaces.Place] {
        places.mapValues { ThumbnailPlaces.Place(rect: root.convert($0.rect, from: view), image: $0.image) }
    }

    /// A rect in the root's coordinates, in the flight layer's.
    private func layerRect(_ rect: CGRect) -> CGRect { flight.convert(rect, from: root) }

    /// Masks the sheet and the flyers to a rect growing (or shrinking) from
    /// `from` to `to`. A frame partly past the strip's end is clipped there
    /// as the strip clips it, rather than drawn over the inspector; on the
    /// same timing, the flyers otherwise stay inside it all the way.
    private func mask(from: CGRect, to: CGRect) {
        for view in [container, flight] {
            view.wantsLayer = true
            let mask = CALayer()
            mask.backgroundColor = NSColor.black.cgColor
            let start = view.convert(from, from: root), end = view.convert(to, from: root)
            mask.frame = end
            view.layer?.mask = mask
            animate(mask, from: start, to: end, opacity: nil, delay: 0)
        }
    }

    private func unmask() {
        container.layer?.mask = nil
        flight.layer?.mask = nil
    }

    /// A picture flying from `from` to `to` (root coordinates), cropped to fill like the views.
    private func flyer(image: CGImage, from: CGRect, to: CGRect, opacity: (Float, Float), delay: CFTimeInterval) -> CALayer {
        let layer = CALayer()
        layer.contents = image
        layer.contentsGravity = .resizeAspectFill
        layer.masksToBounds = true
        layer.contentsScale = flight.window?.backingScaleFactor ?? 2
        let start = layerRect(from), end = layerRect(to)
        layer.frame = end
        layer.opacity = opacity.1
        flight.layer?.addSublayer(layer)
        animate(layer, from: start, to: end, opacity: opacity, delay: delay)
        return layer
    }

    private func animate(_ layer: CALayer, from: CGRect, to: CGRect, opacity: (Float, Float)?, delay: CFTimeInterval) {
        let begin = CACurrentMediaTime() + delay
        func basic(_ path: String, _ a: Any, _ b: Any) -> CABasicAnimation {
            let animation = CABasicAnimation(keyPath: path)
            animation.fromValue = a
            animation.toValue = b
            animation.duration = Self.duration
            animation.beginTime = begin
            animation.timingFunction = Self.timing
            // Holds the start until a staggered one begins.
            animation.fillMode = .backwards
            return animation
        }
        layer.add(basic("position", NSValue(point: CGPoint(x: from.midX, y: from.midY)), NSValue(point: CGPoint(x: to.midX, y: to.midY))), forKey: "position")
        layer.add(basic("bounds.size", NSValue(size: from.size), NSValue(size: to.size)), forKey: "size")
        if let opacity { layer.add(basic("opacity", opacity.0, opacity.1), forKey: "opacity") }
    }

    private func cancelFlight() {
        flight.layer?.sublayers?.forEach { $0.removeFromSuperlayer() }
        unmask()
    }

    // MARK: Timing

    private func after(_ seconds: CFTimeInterval, _ generation: Int, _ work: @escaping @MainActor () -> Void) {
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { [weak self] in
            MainActor.assumeIsolated {
                guard self?.generation == generation else { return }
                work()
            }
        }
    }

    /// Once SwiftUI has laid out what the last change asked for (two turns
    /// of the main queue: its update, then the views' geometry reports).
    private func afterLayout(_ work: @escaping @MainActor () -> Void) {
        DispatchQueue.main.async {
            DispatchQueue.main.async { MainActor.assumeIsolated { work() } }
        }
    }
}

/// The layer the flyers travel in, over everything; clicks pass through.
private final class FlightView: NSView {
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// The sheet's container, which the flight masks; top left origin.
final class SheetContainer: NSView {
    override var isFlipped: Bool { true }
}
