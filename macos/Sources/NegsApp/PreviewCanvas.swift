import AppKit
import QuartzCore
import SwiftUI

/// AppKit owns scrolling and trackpad magnification, including gesture inertia.
/// Core Animation composites the photograph, so zoom and pan never redraw it.
struct PreviewCanvas: NSViewRepresentable {
    /// An IOSurface or CGImage, scaled to fill the photograph's layout size.
    let image: AnyObject
    let size: CGSize
    let rotation: Int
    let crop: CropSettings
    let border: BorderSettings?
    let filmTexture: CGImage?
    let filmStrip: FilmStrip?
    let zoomCommand: UUID
    let fit: Bool
    var scale: CanvasScale?
    /// The pins shown over the photograph.
    var pins: [TonePinMarker] = []
    /// Called when a pin's chip is clicked.
    var onSelectPin: ((Int) -> Void)? = nil
    /// A tone can be pinned right now, so ⌘ shows the sample circle.
    var allowsPinning = false
    /// Called with the centre (unit coordinates, top left) and radius (a
    /// fraction of the shorter side) of a ⌘-click.
    var onPin: ((CGPoint, Double) -> Void)? = nil
    /// Called when a placed pin is dragged to a new point.
    var onMovePin: ((Int, CGPoint) -> Void)? = nil
    /// Esc and Delete on the photograph: drop the tone selection / remove it.
    var onEscapeKey: (() -> Void)? = nil
    var onDeleteKey: (() -> Void)? = nil
    /// Called when the user changes between fit and a zoomed view.
    var onFitChanged: ((Bool) -> Void)? = nil

    func makeNSView(context: Context) -> ImageScrollView {
        let scroll = ImageScrollView()
        scroll.contentView = CenteringClipView()
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true; scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        scroll.documentView = PhotographView()
        scroll.setAccessibilityLabel("Photograph preview")
        scroll.setAccessibilityHelp("Pinch to zoom, drag to pan, ⌘-click to pin the tone inside the circle, ⌘-scroll to size the circle, double-click to fit or show actual size.")
        scroll.onFitChanged = onFitChanged
        return scroll
    }

    func updateNSView(_ scroll: ImageScrollView, context: Context) {
        scale?.view = scroll
        scroll.onFitChanged = onFitChanged
        let photo = scroll.documentView as! PhotographView
        photo.onEscapeKey = onEscapeKey
        photo.onDeleteKey = onDeleteKey
        photo.onSelectPin = onSelectPin
        photo.onPin = onPin
        photo.onMovePin = onMovePin
        photo.allowsPinning = allowsPinning
        photo.setPins(pins)
        let cropped = crop.size(of: size)
        let displaySize = rotation % 2 != 0 ? CGSize(width: cropped.height, height: cropped.width) : cropped
        let documentSize = border.map { border in
            let canvas = border.canvas(for: displaySize, strip: filmStrip)
            return border.instagram ? border.instagramCanvas(for: canvas) : canvas
        } ?? displaySize
        let oldSize = photo.frame.size, oldPhotoSize = photo.displaySize
        let commanded = scroll.lastCommand != zoomCommand
        // A different crop (such as an undone one) is a new picture: fit it.
        let recropped = photo.crop != crop && oldSize.width != 0
        if recropped { scroll.fitting = true }
        photo.show(image, size: size, rotation: rotation, crop: crop, border: border, filmTexture: filmTexture, filmStrip: filmStrip)
        if oldSize != documentSize {
            if commanded || scroll.fitting || oldSize.width == 0 {
                photo.setFrameSize(documentSize)
            } else if oldPhotoSize != displaySize {
                scroll.keepVisibleContent(resizingDocumentFrom: oldSize, to: documentSize)
            } else {
                // Only the border changed: keep the photograph where it is.
                scroll.keepPhotographInPlace(resizingDocumentFrom: oldSize, to: documentSize)
            }
        }
        if commanded {
            scroll.lastCommand = zoomCommand; scroll.fitting = fit
            scroll.applyZoom()
        } else if oldSize != documentSize || recropped, scroll.fitting { scroll.applyZoom() }
    }
}

/// How many screen pixels a pixel of the photograph covers at the canvas's
/// zoom. A render this much smaller than the photograph looks the same on screen.
@MainActor final class CanvasScale {
    fileprivate weak var view: ImageScrollView?
    var screenPixelsPerPhotoPixel: CGFloat? {
        guard let view, let window = view.window, view.magnification > 0 else { return nil }
        return view.magnification * window.backingScaleFactor
    }
}

final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var bounds = super.constrainBoundsRect(proposedBounds)
        guard let documentView else { return bounds }
        if bounds.width > documentView.frame.width { bounds.origin.x = (documentView.frame.width - bounds.width) / 2 }
        if bounds.height > documentView.frame.height { bounds.origin.y = (documentView.frame.height - bounds.height) / 2 }
        return bounds
    }
}

final class ImageScrollView: NSScrollView {
    var lastCommand: UUID?
    var fitting = true
    var onFitChanged: ((Bool) -> Void)?
    private var lastViewport = CGSize.zero
    override func layout() {
        super.layout()
        if lastViewport != contentSize {
            lastViewport = contentSize
            if fitting { applyZoom() }
        }
    }
    func applyZoom() {
        guard let documentView, documentView.frame.width > 0, contentSize.width > 0 else { return }
        let size = documentView.frame.size
        // 1:1 is one photograph pixel per screen pixel. Fitting shows the whole
        // photograph, but never enlarges it past 1:1.
        let oneToOne = 1 / (window?.backingScaleFactor ?? 2)
        let fit = min(contentSize.width / size.width, contentSize.height / size.height)
        let amount = fitting ? min(fit, oneToOne) : oneToOne
        setMagnification(min(max(amount, minMagnification), maxMagnification), centeredAt: NSPoint(x: size.width / 2, y: size.height / 2))
    }
    /// Whether the photograph is larger than the viewport, so panning has
    /// somewhere to go.
    var canScroll: Bool {
        guard let documentView else { return false }
        return documentView.frame.width > contentSize.width + 0.5
            || documentView.frame.height > contentSize.height + 0.5
    }
    /// Double-click: fit the whole photograph, or show it at 1:1 about the
    /// clicked point, which then stays under the pointer.
    func toggleZoom(about point: CGPoint) {
        if fitting {
            setFitting(false)
            let oneToOne = 1 / (window?.backingScaleFactor ?? 2)
            setMagnification(
                min(max(oneToOne, minMagnification), maxMagnification),
                centeredAt: point
            )
        } else {
            setFitting(true)
            applyZoom()
        }
    }
    /// A higher-resolution render of the same photograph replaces the shown
    /// one; keep the same region on screen at the same apparent scale.
    func keepVisibleContent(resizingDocumentFrom old: CGSize, to new: CGSize) {
        guard let documentView else { return }
        let visible = contentView.bounds
        let center = CGPoint(x: visible.midX / old.width, y: visible.midY / old.height)
        let ratio = old.width / new.width
        documentView.setFrameSize(new)
        magnification = min(max(magnification * ratio, minMagnification), maxMagnification)
        let bounds = contentView.bounds
        contentView.scroll(to: CGPoint(x: center.x * new.width - bounds.width / 2, y: center.y * new.height - bounds.height / 2))
        reflectScrolledClipView(contentView)
    }
    func keepPhotographInPlace(resizingDocumentFrom old: CGSize, to new: CGSize) {
        guard let documentView else { return }
        let origin = contentView.bounds.origin
        documentView.setFrameSize(new)
        contentView.scroll(to: CGPoint(x: origin.x + (new.width - old.width) / 2, y: origin.y + (new.height - old.height) / 2))
        reflectScrolledClipView(contentView)
    }
    /// Records a zoom the user made by hand, so changing photographs keeps
    /// the same fit or 1:1 setting.
    func setFitting(_ value: Bool) {
        guard fitting != value else { return }
        fitting = value
        onFitChanged?(value)
    }
    /// Pins keep their size on screen, so every change of zoom redraws them.
    override var magnification: CGFloat { didSet { (documentView as? PhotographView)?.zoomChanged() } }
    override func setMagnification(_ magnification: CGFloat, centeredAt point: NSPoint) {
        super.setMagnification(magnification, centeredAt: point)
        (documentView as? PhotographView)?.zoomChanged()
    }
    override func magnify(with event: NSEvent) {
        setFitting(false)
        super.magnify(with: event)
        (documentView as? PhotographView)?.zoomChanged()
    }
    override func scrollWheel(with event: NSEvent) {
        if event.modifierFlags.contains(.command), let photo = documentView as? PhotographView {
            photo.sizeSample(with: event)
        } else if event.modifierFlags.contains(.option) {
            setFitting(false)
            let center = documentView?.convert(event.locationInWindow, from: nil) ?? .zero
            setMagnification(min(max(magnification * exp(-event.scrollingDeltaY * 0.01), minMagnification), maxMagnification), centeredAt: center)
        } else { super.scrollWheel(with: event) }
    }
}

final class PhotographView: NSView {
    /// The crop: it clips the photograph and takes the quarter turns.
    private let clip = CALayer()
    /// The whole photograph, straightened inside the crop.
    private let photo = CALayer()
    /// The circles the pins average, over the photograph.
    private let ringLayer = CAShapeLayer()
    /// The circle a ⌘-click would average, while ⌘ is held.
    private let sampleRing = CAShapeLayer()
    /// Where the sample circle is drawn; nil while it's hidden.
    private var samplePoint: CGPoint?
    private var flagsMonitor: Any?
    private var borderLayers: BorderLayers!
    private var image: AnyObject?
    private var size = CGSize.zero
    private var rotation = 0
    private(set) var crop = CropSettings()
    private var border: BorderSettings?
    private var filmTexture: CGImage?
    private var filmStrip: FilmStrip?
    /// The pins to show: a numbered, luminance-coloured chip each, with the
    /// ring of the area it averaged.
    private var pinMarkers: [TonePinMarker] = []
    /// One chip layer per pin, keyed by the pin's index.
    private var chips: [Int: CALayer] = [:]
    /// Called when a pin's chip is clicked.
    var onSelectPin: ((Int) -> Void)?
    /// Called with the centre (unit coordinates, top left) and radius (a
    /// fraction of the shorter side) of a ⌘-click.
    var onPin: ((CGPoint, Double) -> Void)?
    /// Called when a placed pin is dragged to a new point.
    var onMovePin: ((Int, CGPoint) -> Void)?
    var onEscapeKey: (() -> Void)?
    var onDeleteKey: (() -> Void)?
    /// A tone can be pinned right now.
    var allowsPinning = false {
        didSet { if allowsPinning != oldValue { refreshSampleCircle(NSEvent.modifierFlags) } }
    }
    /// The circle a ⌘-click averages, which ⌘-scroll sizes.
    var sampleRadius = SampleSize.initial
    var sampleCircleVisible: Bool { !sampleRing.isHidden }
    /// The drag in progress.
    private enum Drag {
        case pan
        /// A pin dragged from its chip: it follows the pointer from where it
        /// was grabbed, and the tone is measured again on mouse-up.
        case movePin(index: Int, start: CGPoint, grab: CGVector, unit: CGPoint?)
    }
    private var drag: Drag?
    /// Where a dragged pin is drawn until the model has measured it there.
    private var movedPin: (index: Int, centre: CGPoint)?
    private var cursorPushed = false
    /// The photograph's size on the canvas, after cropping and rotation.
    var displaySize: CGSize {
        let cropped = crop.size(of: size)
        return rotation % 2 != 0 ? CGSize(width: cropped.height, height: cropped.width) : cropped
    }
    override var acceptsFirstResponder: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        photo.contentsGravity = .resize
        photo.minificationFilter = .trilinear
        photo.magnificationFilter = .linear
        photo.isOpaque = true
        let none = [
            "contents": NSNull(), "bounds": NSNull(), "position": NSNull(),
            "transform": NSNull(), "path": NSNull(), "lineDashPhase": NSNull(),
            "backgroundColor": NSNull(), "borderWidth": NSNull()
        ]
        photo.actions = none; clip.actions = none
        ringLayer.actions = none; sampleRing.actions = none
        clip.masksToBounds = true
        // The pins and the sample circle, drawn in the photographed frame so they
        // follow the crop and the straightening, never exported. The photo is
        // added first: a layer added earlier would be painted under it.
        ringLayer.fillColor = nil
        ringLayer.strokeColor = NSColor.controlAccentColor.withAlphaComponent(0.75).cgColor
        ringLayer.lineWidth = 1
        sampleRing.fillColor = NSColor.controlAccentColor.withAlphaComponent(0.12).cgColor
        sampleRing.strokeColor = NSColor.controlAccentColor.withAlphaComponent(0.95).cgColor
        sampleRing.lineWidth = 1
        sampleRing.isHidden = true
        clip.addSublayer(photo)
        clip.addSublayer(ringLayer)
        clip.addSublayer(sampleRing)
        layer?.addSublayer(clip)
        borderLayers = BorderLayers(in: layer!, around: clip)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ image: AnyObject, size: CGSize, rotation: Int, crop: CropSettings = CropSettings(), border: BorderSettings? = nil, filmTexture: CGImage? = nil, filmStrip: FilmStrip? = nil) {
        guard image !== self.image || size != self.size || rotation != self.rotation || crop != self.crop
            || border != self.border || filmTexture !== self.filmTexture || filmStrip != self.filmStrip else { return }
        self.image = image; self.size = size; self.rotation = rotation; self.crop = crop
        self.border = border; self.filmTexture = filmTexture; self.filmStrip = filmStrip
        photo.contents = image
        needsLayout = true
    }

    /// Shows the pins: a numbered chip filled with the pin's tone, and a thin
    /// ring around the area the pin averaged.
    func setPins(_ markers: [TonePinMarker]) {
        guard markers != pinMarkers else { return }
        pinMarkers = markers
        movedPin = nil
        updatePins()
    }

    /// The photograph point under `viewPoint`, as unit coordinates of the
    /// uncropped photograph with the origin at its top left. Nil outside the
    /// crop or the photograph.
    func unitPoint(at viewPoint: CGPoint) -> CGPoint? {
        guard let root = layer, size.width > 0, size.height > 0 else { return nil }
        let inClip = clip.convert(viewPoint, from: root)
        guard inClip.x >= 0, inClip.y >= 0,
              inClip.x < clip.bounds.width, inClip.y < clip.bounds.height
        else { return nil }
        // The photo layer carries the straightening, so its own space is the
        // photograph's: y runs up from the bottom left.
        let inPhoto = photo.convert(viewPoint, from: root)
        let x = inPhoto.x / size.width, y = 1 - inPhoto.y / size.height
        guard x >= 0, x <= 1, y >= 0, y <= 1 else { return nil }
        return CGPoint(x: x, y: y)
    }

    /// A unit point in the clip's coordinates (photograph pixels, y up).
    private func clipPoint(_ unit: CGPoint) -> CGPoint {
        clip.convert(CGPoint(x: unit.x * size.width, y: (1 - unit.y) * size.height), from: photo)
    }

    /// The canvas's zoom. Clip units are the document's, so this alone maps
    /// them to screen points, whatever the rotation or border.
    private var zoom: CGFloat { max(enclosingScrollView?.magnification ?? 1, 0.01) }

    /// A chip's diameter in clip units: 18 points on screen.
    private var chipSize: CGFloat { 18 / zoom }

    func zoomChanged() {
        drawSampleCircle()
        guard !pinMarkers.isEmpty else { return }
        updatePins()
    }

    private func updatePins() {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let rings = CGMutablePath()
        let diameter = chipSize
        let upright = CGAffineTransform(rotationAngle: .pi / 2 * CGFloat(rotation))
        let scale = (window?.backingScaleFactor ?? 2) * zoom
        var live = Set<Int>()
        for marker in pinMarkers {
            live.insert(marker.index)
            let moved = movedPin.flatMap { $0.index == marker.index ? $0.centre : nil }
            let centre = moved ?? clipPoint(marker.source.centre)
            let radius = marker.source.radius * min(size.width, size.height)
            if radius > 0.5 {
                rings.addEllipse(in: CGRect(
                    x: centre.x - radius, y: centre.y - radius,
                    width: radius * 2, height: radius * 2
                ))
            }
            let chip = chips[marker.index] ?? makeChip(marker.index)
            chip.setAffineTransform(upright)
            chip.bounds = CGRect(x: 0, y: 0, width: diameter, height: diameter)
            chip.position = centre
            chip.name = "pin \(marker.number)"
            chip.cornerRadius = diameter / 2
            chip.backgroundColor = ZoneScale.grey(lstar: marker.lstar).cgColor
            // A chip sits on the tone it's filled with, so it needs an edge
            // of its own to be seen.
            chip.borderWidth = (marker.selected ? 2 : 1) / zoom
            chip.borderColor = (marker.selected ? NSColor.controlAccentColor : NSColor.white.withAlphaComponent(0.8)).cgColor
            if let label = chip.sublayers?.first as? CATextLayer {
                // CATextLayer draws from its top, so centring the line means
                // dropping it by half the space its height leaves.
                let fontSize = diameter * 0.58
                let lineHeight = fontSize * 1.2
                label.frame = CGRect(x: 0, y: -(diameter - lineHeight) / 2, width: diameter, height: diameter)
                label.fontSize = fontSize
                label.foregroundColor = (marker.lstar < 55 ? NSColor.white : NSColor.black).cgColor
                label.string = "\(marker.number)"
                label.contentsScale = scale
            }
        }
        ringLayer.path = rings.isEmpty ? nil : rings
        ringLayer.lineWidth = 1 / zoom
        for (index, chip) in chips where !live.contains(index) {
            chip.removeFromSuperlayer()
            chips.removeValue(forKey: index)
        }
        CATransaction.commit()
    }

    private func makeChip(_ index: Int) -> CALayer {
        let chip = CALayer()
        chip.masksToBounds = true
        chip.actions = [
            "contents": NSNull(), "bounds": NSNull(), "position": NSNull(),
            "backgroundColor": NSNull(), "borderWidth": NSNull(), "borderColor": NSNull(),
            "transform": NSNull(), "cornerRadius": NSNull()
        ]
        let label = CATextLayer()
        label.alignmentMode = .center
        label.font = NSFont.systemFont(ofSize: 0, weight: .semibold)
        label.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        chip.addSublayer(label)
        clip.addSublayer(chip)
        chips[index] = chip
        return chip
    }

    /// The pin whose chip is under `point`, if any. Only the chip takes the
    /// pointer, so a plain drag inside a large circle still pans.
    func pinIndex(at point: CGPoint) -> Int? {
        guard let root = layer, size.width > 0, !pinMarkers.isEmpty else { return nil }
        let clipped = clip.convert(point, from: root)
        let half = chipSize / 2
        return pinMarkers.last { marker in
            let centre = clipPoint(marker.source.centre)
            return hypot(clipped.x - centre.x, clipped.y - centre.y) <= half
        }?.index
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 53:
            onEscapeKey?()
            return
        case 51, 117:
            onDeleteKey?()
            return
        default:
            super.keyDown(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        movedPin = nil
        let start = CanvasGesture.start(
            clickCount: event.clickCount, modifiers: event.modifierFlags,
            pinUnderPointer: pinIndex(at: point)
        )
        switch start {
        case .zoomToggle:
            (enclosingScrollView as? ImageScrollView)?.toggleZoom(about: point)
        case .selectPin(let index):
            onSelectPin?(index)
            guard let marker = pinMarkers.first(where: { $0.index == index }), let root = layer else { return }
            let chip = root.convert(clipPoint(marker.source.centre), from: clip)
            drag = .movePin(index: index, start: point, grab: CGVector(dx: point.x - chip.x, dy: point.y - chip.y), unit: nil)
            pushCursor(.closedHand)
        case .pin:
            guard let unit = unitPoint(at: point) else { return }
            if allowsPinning { onPin?(unit, sampleRadius) }
        case .pan:
            guard let scroll = enclosingScrollView as? ImageScrollView, scroll.canScroll else { return }
            drag = .pan
            pushCursor(.closedHand)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        switch drag {
        case .movePin(let index, let start, let grab, _):
            guard hypot(point.x - start.x, point.y - start.y) > 2 else { return }
            let centre = CGPoint(x: point.x - grab.dx, y: point.y - grab.dy)
            guard let unit = unitPoint(at: centre) else { return }
            drag = .movePin(index: index, start: start, grab: grab, unit: unit)
            movedPin = (index, clip.convert(centre, from: layer))
            updatePins()
        case .pan:
            guard let scroll = enclosingScrollView else { return }
            var origin = scroll.contentView.bounds.origin
            origin.x -= event.deltaX / scroll.magnification
            origin.y += event.deltaY / scroll.magnification
            scroll.contentView.scroll(to: origin)
            scroll.reflectScrolledClipView(scroll.contentView)
        case nil:
            break
        }
    }

    override func mouseUp(with event: NSEvent) {
        if case .movePin(let index, _, _, let unit?) = drag {
            onMovePin?(index, unit)
        }
        drag = nil
        popCursor()
    }

    // MARK: The sample circle

    override func mouseMoved(with event: NSEvent) {
        updateSampleCircle(at: convert(event.locationInWindow, from: nil), commandHeld: event.modifierFlags.contains(.command))
    }

    override func mouseExited(with event: NSEvent) {
        updateSampleCircle(at: .zero, commandHeld: false)
    }

    /// ⌘ can go down without the mouse moving, and with a field holding the
    /// keyboard, so the window's modifier changes are watched here.
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let flagsMonitor { NSEvent.removeMonitor(flagsMonitor) }
        flagsMonitor = nil
        guard window != nil else { return }
        flagsMonitor = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.refreshSampleCircle(event.modifierFlags)
            return event
        }
    }

    private func refreshSampleCircle(_ modifiers: NSEvent.ModifierFlags) {
        let point = window.map { convert($0.mouseLocationOutsideOfEventStream, from: nil) } ?? .zero
        updateSampleCircle(at: point, commandHeld: modifiers.contains(.command) && (window == nil || visibleRect.contains(point)))
    }

    /// Shows the sample circle at `point` while ⌘ is held over the photograph.
    func updateSampleCircle(at point: CGPoint, commandHeld: Bool) {
        let shown = commandHeld && allowsPinning && unitPoint(at: point) != nil
        let wasShown = samplePoint != nil
        samplePoint = shown ? point : nil
        sampleRing.isHidden = !shown
        drawSampleCircle()
        if shown { NSCursor.crosshair.set() } else if wasShown { NSCursor.arrow.set() }
    }

    /// One step of ⌘-scroll.
    func sizeSample(with event: NSEvent) {
        sampleRadius = SampleSize.radius(
            from: sampleRadius, scrollDelta: Double(event.scrollingDeltaY), precise: event.hasPreciseScrollingDeltas
        )
        drawSampleCircle()
    }

    private func drawSampleCircle() {
        guard let samplePoint else { return }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let centre = clip.convert(samplePoint, from: layer)
        // One pixel still shows, as a small circle.
        let radius = max(CGFloat(sampleRadius) * min(size.width, size.height), 3 / zoom)
        sampleRing.path = CGPath(
            ellipseIn: CGRect(x: centre.x - radius, y: centre.y - radius, width: radius * 2, height: radius * 2),
            transform: nil
        )
        sampleRing.lineWidth = 1 / zoom
        CATransaction.commit()
    }

    override func layout() {
        super.layout()
        CATransaction.begin(); CATransaction.setDisableActions(true)
        // The crop's pixels, as the exporter cuts them. Layers are y-up, so
        // the rectangle's top-left origin flips, and clockwise is negative.
        let pixels = crop.pixels(in: size)
        clip.bounds = CGRect(origin: .zero, size: pixels.size)
        clip.position = CGPoint(x: bounds.midX, y: bounds.midY)
        clip.setAffineTransform(CGAffineTransform(rotationAngle: -.pi / 2 * CGFloat(rotation)))
        photo.bounds = CGRect(origin: .zero, size: size)
        photo.position = CGPoint(x: size.width / 2 - pixels.minX, y: pixels.height - (size.height / 2 - pixels.minY))
        photo.setAffineTransform(CGAffineTransform(rotationAngle: -crop.angle * .pi / 180))
        borderLayers.layout(border, photo: displaySize, host: layer!, texture: filmTexture, filmStrip: filmStrip)
        updatePins()
        CATransaction.commit()
    }

    /// The layers the canvas builds, so tests can drive its coordinate map.
    var photographLayer: CALayer { photo }
    var clipLayer: CALayer { clip }

    /// Each pin chip's centre in the view's coordinates, for tests.
    var chipCentres: [CGPoint] {
        chips.values.map { $0.convert(CGPoint(x: $0.bounds.midX, y: $0.bounds.midY), to: self.layer) }
    }

    var chipLayers: [CALayer] { Array(chips.values) }

    override func resetCursorRects() { addCursorRect(bounds, cursor: .arrow) }

    private func pushCursor(_ cursor: NSCursor) {
        guard !cursorPushed else { return }
        cursorPushed = true
        cursor.push()
    }

    private func popCursor() {
        guard cursorPushed else { return }
        cursorPushed = false
        NSCursor.pop()
    }
}
