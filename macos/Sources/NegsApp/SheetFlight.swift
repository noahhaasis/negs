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
}

/// Reports a thumbnail's rect and picture as they change, and nil once it's gone.
struct PlaceReport: ViewModifier {
    let image: CGImage?
    let placed: (ThumbnailPlaces.Place?) -> Void
    @State private var rect: CGRect?

    func body(content: Content) -> some View {
        content
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { rect in
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
        let height = (model.filmstripVisible ? Theme.Size.strip : 0) + Theme.Size.statusLine + 1
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
        guard !Self.reduceMotion else {
            container.alphaValue = 1
            return
        }
        // Hidden until it's laid out on the current photograph.
        container.alphaValue = 0
        let generation = generation
        afterLayout { [weak self] in
            guard let self, self.generation == generation, self.model.mode == .grid else { return }
            self.flyOpen(generation)
        }
    }

    private func flyOpen(_ generation: Int) {
        let viewport = root.convert(sheet.bounds, from: sheet)
        let shown = model.shownPhotos.map(\.url)
        let current = model.selection.current
        let centre = current.flatMap { shown.firstIndex(of: $0) } ?? 0
        var order: [URL: Int] = [:]
        for (index, url) in shown.enumerated() { order[url] = index }
        let grid = places(model.places.sheet, in: sheet).filter { $0.value.rect.intersects(viewport) }
        let strip = places(model.places.strip, in: canvas).filter { $0.value.rect.intersects(band) }
        model.sheetWaiting = Set(grid.keys)

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        container.alphaValue = 1
        let mask = maskLayer(from: band, to: root.bounds)
        var latest: CFTimeInterval = Self.duration
        for (url, to) in grid {
            let delay = Self.stagger * CFTimeInterval(min(abs((order[url] ?? centre) - centre), Self.staggerSteps))
            if let from = strip[url], let image = to.image ?? from.image {
                let opacity: Float = url == model.sourceItem ? 1 : Float(Theme.stripDim)
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
            mask.removeFromSuperlayer()
            self?.container.layer?.mask = nil
            self?.model.sheetWaiting = []
        }
    }

    // MARK: Folding

    /// Folds the sheet back into the strip, then hides it.
    func fold() {
        generation += 1
        cancelFlight()
        let generation = generation
        guard !Self.reduceMotion, !container.isHidden else { return finishFold() }
        let viewport = root.convert(sheet.bounds, from: sheet)
        let grid = places(model.places.sheet, in: sheet).filter { $0.value.rect.intersects(viewport) }
        // The strip moves to the photograph opened; it's laid out under the sheet.
        afterLayout { [weak self] in
            guard let self, self.generation == generation else { return }
            let strip = self.places(self.model.places.strip, in: self.canvas).filter { $0.value.rect.intersects(self.band) }
            let carried = Set(grid.keys).intersection(strip.keys)
            self.model.sheetWaiting = carried
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            _ = self.maskLayer(from: self.root.bounds, to: self.band)
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

    /// Masks the sheet to a rect growing (or shrinking) from `from` to `to`.
    private func maskLayer(from: CGRect, to: CGRect) -> CALayer {
        container.wantsLayer = true
        let mask = CALayer()
        mask.backgroundColor = NSColor.black.cgColor
        let start = container.convert(from, from: root), end = container.convert(to, from: root)
        mask.frame = end
        container.layer?.mask = mask
        animate(mask, from: start, to: end, opacity: nil, delay: 0)
        return mask
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
        container.layer?.mask = nil
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
