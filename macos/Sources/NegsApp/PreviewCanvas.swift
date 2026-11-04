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
    /// The region the histogram covers; nil is the whole photograph.
    var selection: CropSettings?
    /// Called while a region is dragged or moved, with `picking` true when
    /// ⌃ makes it a tone pick too.
    var onSelection: ((CropSettings?, _ picking: Bool) -> Void)? = nil
    /// Called on a ⌃-click that did not drag.
    var onPick: ((TonePick) -> Void)? = nil
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
        scroll.setAccessibilityHelp("Pinch to zoom, scroll or ⇧-drag to pan, drag to select a region for the histogram, Control-click to read a tone, double-click to fit or show actual size.")
        scroll.onFitChanged = onFitChanged
        return scroll
    }

    func updateNSView(_ scroll: ImageScrollView, context: Context) {
        scale?.view = scroll
        scroll.onFitChanged = onFitChanged
        let photo = scroll.documentView as! PhotographView
        photo.onSelection = onSelection
        photo.onPick = onPick
        photo.setSelection(selection)
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
    /// The selected region, outlined over the photograph.
    private let marquee = CAShapeLayer()
    private var borderLayers: BorderLayers!
    private var image: AnyObject?
    private var size = CGSize.zero
    private var rotation = 0
    private(set) var crop = CropSettings()
    private var border: BorderSettings?
    private var filmTexture: CGImage?
    private var filmStrip: FilmStrip?
    /// The region the histogram covers, in the crop's turned frame.
    private var selection: CropSettings?
    /// Called as a region is dragged or moved.
    var onSelection: ((CropSettings?, _ picking: Bool) -> Void)?
    /// Called on a ⌃-click that did not drag.
    var onPick: ((TonePick) -> Void)?
    /// The drag in progress.
    private enum Drag {
        case select(anchor: CGPoint)
        case move(start: CGPoint, original: CropSettings, picking: Bool, moved: Bool)
        case pick(pick: TonePick, start: CGPoint, moved: Bool)
        case pan
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
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull()]
        photo.actions = none; clip.actions = none
        clip.masksToBounds = true
        // The region the histogram covers, drawn in the photographed frame
        // so it follows the crop and the straightening, and never exported.
        marquee.fillColor = nil
        marquee.strokeColor = NSColor.white.withAlphaComponent(0.9).cgColor
        marquee.lineWidth = 1
        marquee.lineJoin = .round
        marquee.actions = none
        clip.addSublayer(marquee)
        clip.addSublayer(photo)
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

    /// Outlines `selection`, or clears the marquee for nil.
    func setSelection(_ selection: CropSettings?) {
        guard selection != self.selection else { return }
        self.selection = selection
        updateMarquee()
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

    /// The region between two view points, clamped to the crop. Nil when the
    /// drag is empty (a click), which means the whole photograph.
    func selection(from a: CGPoint, to b: CGPoint) -> CropSettings? {
        guard let root = layer, size.width >= 1, size.height >= 1 else { return nil }
        let (pa, pb) = (clip.convert(a, from: root), clip.convert(b, from: root))
        let bounds = clip.bounds
        let x0 = min(max(min(pa.x, pb.x), bounds.minX), bounds.maxX)
        let x1 = min(max(max(pa.x, pb.x), bounds.minX), bounds.maxX)
        let y0 = min(max(min(pa.y, pb.y), bounds.minY), bounds.maxY)
        let y1 = min(max(max(pa.y, pb.y), bounds.minY), bounds.maxY)
        guard x1 - x0 >= 0.5, y1 - y0 >= 0.5 else { return nil }
        return selection(fromClip: CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0))
    }

    /// A clip-space rectangle as the crop's turned-frame fractions. Clip
    /// coordinates are y-up with their origin at the crop's bottom left, so
    /// this removes the quarter turn and puts y back at the top left.
    private func selection(fromClip rect: CGRect) -> CropSettings {
        let pixels = crop.pixels(in: size)
        var result = crop
        result.rect = CGRect(
            x: (rect.minX + pixels.minX) / size.width,
            y: (pixels.maxY - rect.maxY) / size.height,
            width: rect.width / size.width,
            height: rect.height / size.height
        )
        return result
    }

    /// The selected pixels in the clip's coordinates.
    private func clipRect(of selection: CropSettings) -> CGRect {
        let pixels = crop.pixels(in: size)
        let chosen = selection.pixels(in: size)
        return CGRect(
            x: chosen.minX - pixels.minX, y: pixels.maxY - chosen.maxY,
            width: chosen.width, height: chosen.height
        )
    }

    private func updateMarquee() {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        if let selection, size.width > 0, size.height > 0 {
            marquee.path = CGPath(rect: clipRect(of: selection), transform: nil)
        } else {
            marquee.path = nil
        }
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
        updateMarquee()
        CATransaction.commit()
    }

    /// `original` shifted by a clip-space delta (photograph pixels, y up),
    /// clamped to the crop. The size never changes.
    func moving(_ original: CropSettings, by delta: CGPoint) -> CropSettings {
        let base = clipRect(of: original), bounds = clip.bounds
        let x = min(max(base.minX + delta.x, bounds.minX), max(bounds.maxX - base.width, bounds.minX))
        let y = min(max(base.minY + delta.y, bounds.minY), max(bounds.maxY - base.height, bounds.minY))
        return selection(fromClip: CGRect(x: x, y: y, width: base.width, height: base.height))
    }

    /// The layers the canvas builds, so tests can drive its coordinate map.
    var photographLayer: CALayer { photo }
    var clipLayer: CALayer { clip }

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

    private func clearSelection() {
        selection = nil
        updateMarquee()
        onSelection?(nil, false)
    }

    private func isInsideSelection(_ point: CGPoint) -> Bool {
        guard let selection, let root = layer, size.width > 0 else { return false }
        return clipRect(of: selection).contains(clip.convert(point, from: root))
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let start = CanvasGesture.start(
            clickCount: event.clickCount, modifiers: event.modifierFlags,
            insideSelection: isInsideSelection(point)
        )
        switch start {
        case .zoomToggle:
            clearSelection()
            (enclosingScrollView as? ImageScrollView)?.toggleZoom(about: point)
        case .select:
            drag = .select(anchor: point)
            pushCursor(.crosshair)
        case .move:
            drag = .move(
                start: point, original: selection ?? crop,
                picking: event.modifierFlags.contains(.control), moved: false
            )
            pushCursor(.closedHand)
        case .pickSmall, .pickLarge:
            guard let unit = unitPoint(at: point) else { return }
            // As in RPP, a pick's mouse-down also clears the region.
            clearSelection()
            drag = .pick(pick: start == .pickSmall ? .small(unit) : .large(unit), start: point, moved: false)
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
        case .select(let anchor):
            if let rect = selection(from: anchor, to: point) {
                selection = rect
                updateMarquee()
                onSelection?(rect, false)
            } else {
                clearSelection()
            }
        case .move(let start, let original, let picking, let moved):
            guard let root = layer else { return }
            let from = clip.convert(start, from: root), to = clip.convert(point, from: root)
            let rect = moving(original, by: CGPoint(x: to.x - from.x, y: to.y - from.y))
            selection = rect
            updateMarquee()
            let dragged = moved || hypot(point.x - start.x, point.y - start.y) > 2
            if dragged {
                drag = .move(start: start, original: original, picking: picking, moved: true)
            }
            if picking || dragged { onSelection?(rect, picking) }
        case .pick(let pick, let start, let moved):
            let dragged = moved || hypot(point.x - start.x, point.y - start.y) > 2
            guard dragged else { return }
            drag = .pick(pick: pick, start: start, moved: true)
            if let rect = selection(from: start, to: point) {
                selection = rect
                updateMarquee()
                onSelection?(rect, true)
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
        switch drag {
        case .select:
            // A click without a drag is an empty region: the whole photograph.
            if selection == nil { onSelection?(nil, false) }
        case .move(_, _, let picking, let moved):
            // A ⌃-click that did not move the region is a pick.
            if picking, !moved, let unit = unitPoint(at: point) { onPick?(.small(unit)) }
        case .pick(let pick, _, let moved):
            if !moved { onPick?(pick) }
        case .pan, nil:
            break
        }
        drag = nil
        popCursor()
    }
}
