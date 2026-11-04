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
    /// The pins shown over the photograph: a numbered chip each, with the
    /// ring of the area it averaged.
    var pins: [TonePinMarker] = []
    /// Called when a pin's chip is clicked.
    var onSelectPin: ((Int?) -> Void)? = nil
    /// Called with the centre (unit coordinates, top left) and radius (a
    /// fraction of the shorter side) of a ⌘-click or ⌘-drag.
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
        scroll.setAccessibilityHelp("Pinch to zoom, drag to pan, ⌘-click to pin a tone, ⌘-drag to pin the average of a circle, double-click to fit or show actual size.")
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
    override func magnify(with event: NSEvent) { setFitting(false); super.magnify(with: event) }
    override func scrollWheel(with event: NSEvent) {
        if event.modifierFlags.contains(.option) {
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
    /// The circle a ⌘-drag is growing, until the mouse comes up.
    private let dragRing = CAShapeLayer()
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
    var onSelectPin: ((Int?) -> Void)?
    /// Called with the centre (unit coordinates, top left) and radius (a
    /// fraction of the shorter side) of a completed ⌘-click or ⌘-drag.
    var onPin: ((CGPoint, Double) -> Void)?
    /// Called when a placed pin is dragged to a new point.
    var onMovePin: ((Int, CGPoint) -> Void)?
    var onEscapeKey: (() -> Void)?
    var onDeleteKey: (() -> Void)?
    /// The drag in progress.
    private enum Drag {
        case pan
        /// A ⌘-drag: the circle grows from `centre` to the pointer.
        case pin(centre: CGPoint, unitCentre: CGPoint)
        /// A pin dragged to a new place: the chip follows, and the tone is
        /// measured again on mouse-up.
        case movePin(index: Int, centre: CGPoint, unitCentre: CGPoint, moved: Bool)
    }
    private var drag: Drag?
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
        ringLayer.actions = none; dragRing.actions = none
        clip.masksToBounds = true
        // The pins and the drag ring, drawn in the photographed frame so they
        // follow the crop and the straightening, never exported. The photo is
        // added first: a layer added earlier would be painted under it.
        ringLayer.fillColor = nil
        ringLayer.strokeColor = NSColor.controlAccentColor.withAlphaComponent(0.75).cgColor
        ringLayer.lineWidth = 1
        dragRing.fillColor = NSColor.controlAccentColor.withAlphaComponent(0.12).cgColor
        dragRing.strokeColor = NSColor.controlAccentColor.withAlphaComponent(0.95).cgColor
        dragRing.lineWidth = 1
        dragRing.isHidden = true
        clip.addSublayer(photo)
        clip.addSublayer(ringLayer)
        clip.addSublayer(dragRing)
        layer?.addSublayer(clip)
        borderLayers = BorderLayers(in: layer!, around: clip)
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

    /// A chip's diameter on screen, in clip pixels, so it stays the same size
    /// as the view zooms.
    private var chipSize: CGFloat {
        guard clip.bounds.width > 0, bounds.width > 0 else { return 18 }
        let zoom = max(enclosingScrollView?.magnification ?? 1, 0.01)
        return 18 / zoom * clip.bounds.width / bounds.width
    }

    private func updatePins() {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let rings = CGMutablePath()
        var anyRing = false
        let diameter = chipSize
        var live = Set<Int>()
        for marker in pinMarkers {
            live.insert(marker.index)
            let centre = clipPoint(marker.source.centre)
            let radius = marker.source.radius * min(size.width, size.height)
            if radius > 0.5 {
                rings.addEllipse(in: CGRect(
                    x: centre.x - radius, y: centre.y - radius,
                    width: radius * 2, height: radius * 2
                ))
                anyRing = true
            }
            let chip = chips[marker.index] ?? makeChip()
            chip.frame = CGRect(
                x: centre.x - diameter / 2, y: centre.y - diameter / 2,
                width: diameter, height: diameter
            )
            chip.cornerRadius = diameter / 2
            chip.backgroundColor = Self.tone(marker.lstar).cgColor
            chip.borderWidth = marker.selected ? 2 : 0
            chip.borderColor = NSColor.controlAccentColor.cgColor
            if let label = chip.sublayers?.first as? CATextLayer {
                label.frame = CGRect(x: 0, y: -1, width: diameter, height: diameter)
                label.fontSize = diameter * 0.55
                label.foregroundColor = (marker.lstar < 55 ? NSColor.white : NSColor.black).cgColor
                label.string = "\(marker.number)"
                label.contentsScale = window?.backingScaleFactor ?? 2
            }
        }
        ringLayer.path = anyRing ? rings : nil
        for (index, chip) in chips where !live.contains(index) {
            chip.removeFromSuperlayer()
            chips.removeValue(forKey: index)
        }
        CATransaction.commit()
    }

    private func makeChip() -> CALayer {
        let chip = CALayer()
        chip.masksToBounds = true
        chip.actions = [
            "contents": NSNull(), "bounds": NSNull(), "position": NSNull(),
            "backgroundColor": NSNull(), "borderWidth": NSNull()
        ]
        let label = CATextLayer()
        label.alignmentMode = .center
        label.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        chip.addSublayer(label)
        clip.addSublayer(chip)
        chips[chips.count] = chip
        return chip
    }

    /// The grey a pin's tone looks like, as sRGB.
    static func tone(_ lstar: Double) -> NSColor {
        let y = lstar > 8 ? pow((lstar + 16) / 116, 3) : lstar * 27.0 / 24389
        let encoded = y <= 0.003_130_8 ? 12.92 * y : 1.055 * pow(y, 1 / 2.4) - 0.055
        let v = min(max(encoded, 0), 1)
        return NSColor(srgbRed: v, green: v, blue: v, alpha: 1)
    }

    /// The pin whose chip or ring contains `point`, if any.
    private func pinIndex(at point: CGPoint) -> Int? {
        guard let root = layer, size.width > 0, !pinMarkers.isEmpty else { return nil }
        let clipped = clip.convert(point, from: root)
        let half = chipSize / 2
        return pinMarkers.last { marker in
            let centre = clipPoint(marker.source.centre)
            let radius = max(marker.source.radius * min(size.width, size.height), half)
            let dx = clipped.x - centre.x, dy = clipped.y - centre.y
            return dx * dx + dy * dy <= radius * radius
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
        let start = CanvasGesture.start(
            clickCount: event.clickCount, modifiers: event.modifierFlags,
            pinUnderPointer: pinIndex(at: point)
        )
        switch start {
        case .zoomToggle:
            (enclosingScrollView as? ImageScrollView)?.toggleZoom(about: point)
        case .selectPin(let index):
            onSelectPin?(index)
            guard let unit = unitPoint(at: point) else { return }
            drag = .movePin(index: index, centre: point, unitCentre: unit, moved: false)
            pushCursor(.openHand)
        case .pin:
            guard let unit = unitPoint(at: point) else { return }
            drag = .pin(centre: point, unitCentre: unit)
            dragRing.isHidden = false
            updateDragRing(centre: point, radius: 0)
            pushCursor(.crosshair)
        case .pan:
            guard let scroll = enclosingScrollView as? ImageScrollView, scroll.canScroll else { return }
            drag = .pan
            pushCursor(.closedHand)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        switch drag {
        case .pin(let centre, _):
            updateDragRing(centre: centre, radius: hypot(point.x - centre.x, point.y - centre.y))
        case .movePin(let index, let start, _, _):
            guard hypot(point.x - start.x, point.y - start.y) > 2, let unit = unitPoint(at: point) else { return }
            drag = .movePin(index: index, centre: point, unitCentre: unit, moved: true)
            if let chip = chips[index] {
                CATransaction.begin(); CATransaction.setDisableActions(true)
                chip.position = clip.convert(point, from: layer)
                CATransaction.commit()
            }
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
        let point = convert(event.locationInWindow, from: nil)
        if case .movePin(let index, _, let unit, let moved) = drag, moved {
            onMovePin?(index, unit)
        }
        if case .pin(let centre, let unit) = drag {
            // The radius is a fraction of the shorter side, in photograph
            // pixels, so a circle means the same area at any zoom.
            let pixels = hypot(point.x - centre.x, point.y - centre.y) * (size.width > 0 ? clip.bounds.width / bounds.width : 1)
            let radius = min(max(Double(pixels / max(min(size.width, size.height), 1)), 0), 1)
            onPin?(unit, radius)
        }
        drag = nil
        dragRing.isHidden = true
        popCursor()
    }

    private func updateDragRing(centre: CGPoint, radius: CGFloat) {
        let local = clip.convert(centre, from: layer)
        dragRing.path = CGPath(
            ellipseIn: CGRect(x: local.x - radius, y: local.y - radius, width: radius * 2, height: radius * 2),
            transform: nil
        )
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

    /// The chips' on-screen diameter in view points, for tests.
    var chipDiameter: CGFloat { chipSize }

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
