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
    /// Where the print is on screen, for the caption and label (§4.1, §4.2).
    var geometry: CanvasGeometry?
    /// A double-click on the photograph; true if it's zoomed from fitting.
    var doubleClick: ((_ zoomed: Bool) -> Void)?

    func makeNSView(context: Context) -> ImageScrollView {
        let scroll = ImageScrollView()
        scroll.contentView = CenteringClipView()
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true; scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        scroll.documentView = PhotographView()
        scroll.setAccessibilityLabel("Photograph preview")
        scroll.setAccessibilityHelp("Pinch to zoom, scroll or drag to pan, double-click to fit or, when fitted, to show the grid.")
        return scroll
    }

    func updateNSView(_ scroll: ImageScrollView, context: Context) {
        scale?.view = scroll
        scroll.geometry = geometry
        scroll.doubleClick = doubleClick
        let photo = scroll.documentView as! PhotographView
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
    /// Stands in for the canvas in tests, which have no window.
    var fixed: CGFloat?
    var screenPixelsPerPhotoPixel: CGFloat? {
        if let fixed { return fixed }
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

/// The print's rectangle on the canvas, cut to what's on screen, in the
/// scroll view's top-left coordinates. Zoomed in, it's the whole viewport,
/// so scrolling doesn't move the caption or the label.
@MainActor @Observable final class CanvasGeometry {
    private(set) var print = CGRect.zero
    /// The scroll view's size.
    private(set) var viewport = CGSize.zero
    private(set) var zoomed = false

    func update(print: CGRect, viewport: CGSize, zoomed: Bool) {
        // Whole points: sub-point scroll jitter never reaches SwiftUI.
        let print = print.integral
        if print != self.print { self.print = print }
        if viewport != self.viewport { self.viewport = viewport }
        if zoomed != self.zoomed { self.zoomed = zoomed }
    }
}

final class ImageScrollView: NSScrollView {
    var lastCommand: UUID?
    var fitting = true
    var doubleClick: ((_ zoomed: Bool) -> Void)?
    var geometry: CanvasGeometry? { didSet { if geometry !== oldValue { observeBounds(); reportGeometry() } } }
    /// Space left and right of a fitted photograph (§2.3). The caption and
    /// label bands above and below are outside the scroll view.
    static let fitInset: CGFloat = 48
    private var lastViewport = CGSize.zero
    override func layout() {
        super.layout()
        if lastViewport != contentSize {
            lastViewport = contentSize
            if fitting { applyZoom() }
        }
        reportGeometry()
    }
    /// The viewport a fitted photograph fills.
    var fitViewport: CGSize { Self.fitViewport(contentSize) }
    static func fitViewport(_ size: CGSize) -> CGSize { CGSize(width: max(size.width - fitInset * 2, 1), height: size.height) }
    func applyZoom() {
        guard let documentView, documentView.frame.width > 0, contentSize.width > 0 else { return }
        let size = documentView.frame.size
        let amount = Self.magnification(fitting: fitting, document: size, viewport: fitViewport, backingScale: window?.backingScaleFactor ?? 2)
        setMagnification(min(max(amount, minMagnification), maxMagnification), centeredAt: NSPoint(x: size.width / 2, y: size.height / 2))
        reportGeometry()
    }

    private var boundsObserver: NSObjectProtocol?
    private func observeBounds() {
        if let boundsObserver { NotificationCenter.default.removeObserver(boundsObserver) }
        boundsObserver = nil
        guard geometry != nil else { return }
        contentView.postsBoundsChangedNotifications = true
        boundsObserver = NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: contentView, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.reportGeometry() }
        }
    }
    deinit { if let boundsObserver { NotificationCenter.default.removeObserver(boundsObserver) } }

    func reportGeometry() {
        guard let geometry, let documentView, documentView.frame.width > 0 else { return }
        var print = documentView.convert(documentView.bounds, to: self)
        if !isFlipped { print.origin.y = bounds.height - print.maxY }
        geometry.update(print: print.intersection(CGRect(origin: .zero, size: bounds.size)), viewport: bounds.size, zoomed: zoomedFromFit)
    }
    /// 1:1 is one photograph pixel per screen pixel. Fitting shows the whole
    /// photograph, but never enlarges a small one past 1:1.
    static func magnification(fitting: Bool, document: CGSize, viewport: CGSize, backingScale: CGFloat) -> CGFloat {
        let oneToOne = 1 / backingScale
        guard fitting else { return oneToOne }
        return min(viewport.width / document.width, viewport.height / document.height, oneToOne)
    }
    /// Zoomed in or out from fitting the window. A zoom that happens to
    /// match the fit, such as 1:1 on a small photograph, isn't.
    var zoomedFromFit: Bool {
        guard !fitting, let documentView else { return false }
        return Self.zoomed(magnification, minimum: minMagnification, maximum: maxMagnification, document: documentView.frame.size,
                           viewport: fitViewport, backingScale: window?.backingScaleFactor ?? 2)
    }
    static func zoomed(_ magnification: CGFloat, minimum: CGFloat, maximum: CGFloat, document: CGSize, viewport: CGSize, backingScale: CGFloat) -> Bool {
        guard document.width > 0, document.height > 0, viewport.width > 0, viewport.height > 0 else { return false }
        let fit = min(max(Self.magnification(fitting: true, document: document, viewport: viewport, backingScale: backingScale), minimum), maximum)
        return abs(magnification / fit - 1) > 0.005
    }
    func doubleClicked() {
        if let doubleClick { doubleClick(zoomedFromFit) } else { fitting = true; applyZoom() }
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
    override func magnify(with event: NSEvent) { fitting = false; super.magnify(with: event) }
    override func scrollWheel(with event: NSEvent) {
        if event.modifierFlags.contains(.option) {
            fitting = false
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
    private var borderLayers: BorderLayers!
    private var image: AnyObject?
    private var size = CGSize.zero
    private var rotation = 0
    private(set) var crop = CropSettings()
    private var border: BorderSettings?
    private var filmTexture: CGImage?
    private var filmStrip: FilmStrip?
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

    override func layout() {
        super.layout()
        CATransaction.begin(); CATransaction.setDisableActions(true)
        Self.layout(clip: clip, photo: photo, size: size, crop: crop, rotation: rotation, center: CGPoint(x: bounds.midX, y: bounds.midY))
        borderLayers.layout(border, photo: displaySize, host: layer!, texture: filmTexture, filmStrip: filmStrip)
        CATransaction.commit()
    }

    /// Places a photograph of `size` in `clip`, cropped, straightened and
    /// turned as the exporter does, centred on `center`. The edited
    /// thumbnails use it too. `cut`: the photograph's image is the crop
    /// already, straightened.
    static func layout(clip: CALayer, photo: CALayer, size: CGSize, crop: CropSettings, rotation: Int, center: CGPoint, cut: Bool = false) {
        // The crop's pixels, as the exporter cuts them. Layers are y-up, so
        // the rectangle's top-left origin flips, and clockwise is negative.
        let pixels = crop.pixels(in: size)
        clip.bounds = CGRect(origin: .zero, size: pixels.size)
        clip.position = center
        clip.setAffineTransform(CGAffineTransform(rotationAngle: -.pi / 2 * CGFloat(rotation)))
        if cut {
            photo.bounds = clip.bounds
            photo.position = CGPoint(x: pixels.width / 2, y: pixels.height / 2)
            photo.setAffineTransform(.identity)
            return
        }
        photo.bounds = CGRect(origin: .zero, size: size)
        photo.position = CGPoint(x: size.width / 2 - pixels.minX, y: pixels.height - (size.height / 2 - pixels.minY))
        photo.setAffineTransform(CGAffineTransform(rotationAngle: -crop.angle * .pi / 180))
    }

    private var grabbing = false
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 2, let scroll = enclosingScrollView as? ImageScrollView {
            scroll.doubleClicked()
        } else if !grabbing { grabbing = true; NSCursor.closedHand.push() }
    }
    override func mouseDragged(with event: NSEvent) {
        guard let scroll = enclosingScrollView else { return }
        var point = scroll.contentView.bounds.origin
        point.x -= event.deltaX / scroll.magnification
        point.y += event.deltaY / scroll.magnification
        scroll.contentView.scroll(to: point); scroll.reflectScrolledClipView(scroll.contentView)
    }
    override func mouseUp(with event: NSEvent) { if grabbing { grabbing = false; NSCursor.pop() } }
}
