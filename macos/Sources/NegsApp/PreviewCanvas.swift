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
    /// T is held: the print on screen turns half a turn (`PhotographView.turned`).
    var upsideDown = false
    /// The image is a settled frame, not one rendered at canvas size while a
    /// slider moves or before a discrete edit's export-size render: fitted,
    /// it's shown resampled to the screen's pixels (`FitDownscale`).
    var settled = true
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
        scroll.setAccessibilityHelp("Pinch to zoom, scroll or drag to pan, double-click to fit or, when fitted, to show the grid. Hold T to turn it upside down.")
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
        photo.settled = settled
        photo.show(image, size: size, rotation: rotation, crop: crop, border: border, filmTexture: filmTexture, filmStrip: filmStrip)
        photo.turned = upsideDown
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
    var fitting = true { didSet { if fitting != oldValue { photograph?.refreshSharp() } } }
    private var photograph: PhotographView? { documentView as? PhotographView }
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
        photograph?.refreshSharp()
    }
    /// Every scroll and zoom, gestures included, moves the clip view's bounds.
    override func reflectScrolledClipView(_ clipView: NSClipView) {
        super.reflectScrolledClipView(clipView)
        photograph?.refreshSharp()
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

extension NSWindow {
    /// A point in the window's coordinates, in its backing pixels.
    func backingPoint(_ point: CGPoint) -> CGPoint { convertToBacking(CGRect(origin: point, size: .zero)).origin }
    func pointFromBacking(_ point: CGPoint) -> CGPoint { convertFromBacking(CGRect(origin: point, size: .zero)).origin }
}

final class PhotographView: NSView {
    /// The print, with its border: turned half a turn about the middle of
    /// what's on screen while T is held. Only the layers turn; the
    /// photograph, its layout and the caption and label over it don't change.
    private let turn = CALayer()
    /// The crop: it clips the photograph and takes the quarter turns.
    private let clip = CALayer()
    /// The whole photograph, straightened inside the crop.
    private let photo = CALayer()
    /// Over `photo` once the fitted view settles: the whole photograph
    /// resampled to the physical pixels it covers, drawn 1:1 on the pixel
    /// grid (`FitDownscale`). Hidden whenever it doesn't match the canvas.
    let sharp = CALayer()
    private var borderLayers: BorderLayers!
    private var image: AnyObject?
    private var size = CGSize.zero
    private var rotation = 0
    private(set) var crop = CropSettings()
    private var border: BorderSettings?
    private var filmTexture: CGImage?
    private var filmStrip: FilmStrip?
    /// Upside down, while T is held.
    var turned = false { didSet { if turned != oldValue { placeTurn() } } }
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
        // Drawn 1:1, so no mipmaps.
        sharp.contentsGravity = .resize
        sharp.minificationFilter = .linear
        sharp.magnificationFilter = .linear
        sharp.isOpaque = true
        sharp.isHidden = true
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull(), "hidden": NSNull()]
        photo.actions = none; clip.actions = none; turn.actions = none; sharp.actions = none
        clip.masksToBounds = true
        clip.addSublayer(photo)
        clip.addSublayer(sharp)
        turn.addSublayer(clip)
        layer?.addSublayer(turn)
        borderLayers = BorderLayers(in: turn, around: clip)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ image: AnyObject, size: CGSize, rotation: Int, crop: CropSettings = CropSettings(), border: BorderSettings? = nil, filmTexture: CGImage? = nil, filmStrip: FilmStrip? = nil) {
        guard image !== self.image || size != self.size || rotation != self.rotation || crop != self.crop
            || border != self.border || filmTexture !== self.filmTexture || filmStrip != self.filmStrip else { return }
        self.image = image; self.size = size; self.rotation = rotation; self.crop = crop
        self.border = border; self.filmTexture = filmTexture; self.filmStrip = filmStrip
        photo.contents = image
        needsLayout = true
        // A new picture never shows the last one's resample, not even for a frame.
        refreshSharp()
    }

    override func layout() {
        super.layout()
        CATransaction.begin(); CATransaction.setDisableActions(true)
        turn.bounds = CGRect(origin: .zero, size: bounds.size)
        placeTurn()
        Self.layout(clip: clip, photo: photo, size: size, crop: crop, rotation: rotation, center: CGPoint(x: bounds.midX, y: bounds.midY))
        borderLayers.layout(border, photo: displaySize, host: turn, texture: filmTexture, filmStrip: filmStrip)
        refreshSharp()
        CATransaction.commit()
    }

    // MARK: The fitted photograph, resampled

    /// Set by the canvas: false for frames rendered at canvas size.
    var settled = true { didSet { if settled != oldValue { refreshSharp() } } }
    private struct SharpKey: Equatable {
        let source: ObjectIdentifier, seed: UInt32, width: Int, height: Int
    }
    private struct Resampled {
        weak var source: IOSurface?
        let key: SharpKey
        let surface: IOSurface
    }
    /// The last two resamples (the developed photograph's and the original's,
    /// for Before and After), dropped with their sources.
    private var resampled: [Resampled] = []
    private var shownSharp: SharpKey?
    /// The resample on its way, and the one waiting for the canvas to settle.
    private var resampling: SharpKey?
    private var pendingSharp: DispatchWorkItem?
    private var failedSharp: SharpKey?
    private var lastSharpSize: (width: Int, height: Int)?
    /// The last picture checked for `FitDownscale.canResample`, which reads its tag.
    private weak var checkedSource: IOSurface?
    private var checkedResamples = false
    /// After the canvas changes size, the resample waits this long for it to
    /// stop, as while the inspector slides in.
    static let sharpSettle: TimeInterval = 0.15

    /// What the fitted view would show resampled now, or nil for Core
    /// Animation's own scaling: zoomed, in a live resize, a straightened
    /// photograph, a frame rendered at canvas size, or nothing to downscale.
    private func wantedSharp() -> (source: IOSurface, key: SharpKey, scale: CGFloat)? {
        guard settled, crop.angle == 0, let source = image as? IOSurface, let window, !inLiveResize,
              let scroll = enclosingScrollView as? ImageScrollView, scroll.fitting, scroll.magnification > 0 else { return nil }
        if checkedSource !== source { checkedSource = source; checkedResamples = FitDownscale.canResample(source) }
        guard checkedResamples else { return nil }
        let scale = scroll.magnification * window.backingScaleFactor
        guard let target = FitDownscale.targetSize(photo: size, scale: scale), source.width > target.width, source.height > target.height else { return nil }
        return (source, SharpKey(source: ObjectIdentifier(source), seed: source.seed, width: target.width, height: target.height), scale)
    }

    /// Shows, places, hides or asks for the resampled photograph. Cheap; called
    /// on every change to the picture, its layout, the zoom or the window.
    func refreshSharp() {
        resampled.removeAll { $0.source == nil }
        guard let wanted = wantedSharp() else {
            hideSharp(); pendingSharp?.cancel(); pendingSharp = nil
            return
        }
        let key = wanted.key
        if shownSharp != key {
            if let hit = resampled.first(where: { $0.key == key && $0.source === wanted.source }) {
                CATransaction.begin(); CATransaction.setDisableActions(true)
                sharp.contents = hit.surface
                shownSharp = key
                CATransaction.commit()
            } else {
                hideSharp()
                requestSharp(wanted.source, key: key)
                return
            }
        }
        placeSharp(scale: wanted.scale)
    }

    private func hideSharp() {
        guard shownSharp != nil || !sharp.isHidden else { return }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        sharp.isHidden = true; sharp.contents = nil
        shownSharp = nil
        CATransaction.commit()
    }

    private func requestSharp(_ source: IOSurface, key: SharpKey) {
        guard resampling != key, failedSharp != key else { return }
        let resized = lastSharpSize.map { $0 != (key.width, key.height) } ?? true
        lastSharpSize = (key.width, key.height)
        pendingSharp?.cancel()
        let work = DispatchWorkItem { [weak self, weak source] in
            MainActor.assumeIsolated {
                guard let self, let source else { return }
                self.pendingSharp = nil
                // Still wanted: the canvas may have moved on meanwhile.
                guard let wanted = self.wantedSharp(), wanted.key == key, self.resampling != key else { return }
                self.resampling = key
                FitResampler.shared.resample(source, width: key.width, height: key.height) { [weak self, weak source] surface in
                    guard let self else { return }
                    if self.resampling == key { self.resampling = nil }
                    guard let surface, let source else { if surface == nil { self.failedSharp = key }; return }
                    self.resampled.removeAll { $0.key == key || $0.source == nil }
                    self.resampled.append(Resampled(source: source, key: key, surface: surface))
                    if self.resampled.count > 2 { self.resampled.removeFirst(self.resampled.count - 2) }
                    self.refreshSharp()
                }
            }
        }
        pendingSharp = work
        // A new picture at the same size goes at once; a new size waits for
        // the canvas to stop changing.
        DispatchQueue.main.asyncAfter(deadline: .now() + (resized ? Self.sharpSettle : 0), execute: work)
    }

    /// Sizes the resample to its pixels and puts its corner on a physical
    /// pixel, where the photograph's corner is to within half a pixel.
    private func placeSharp(scale: CGFloat) {
        guard let shownSharp, let window else { return }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let extent = CGSize(width: CGFloat(shownSharp.width) / scale, height: CGFloat(shownSharp.height) / scale)
        let corner = CGPoint(x: photo.position.x - photo.bounds.width / 2, y: photo.position.y - photo.bounds.height / 2)
        let snapped = FitDownscale.snapped(corner, toBacking: { window.backingPoint(self.convert(self.turn.convert($0, from: self.clip), to: nil)) },
                                           fromBacking: { self.clip.convert(self.convert(window.pointFromBacking($0), from: nil), from: self.turn) })
        sharp.bounds = CGRect(origin: .zero, size: extent)
        sharp.position = CGPoint(x: snapped.x + extent.width / 2, y: snapped.y + extent.height / 2)
        sharp.isHidden = false
        CATransaction.commit()
    }

    override func viewDidEndLiveResize() {
        super.viewDidEndLiveResize()
        refreshSharp()
    }
    override func viewWillStartLiveResize() {
        super.viewWillStartLiveResize()
        refreshSharp()
    }
    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        refreshSharp()
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        refreshSharp()
    }

    /// Turns the print about the middle of the part on screen, so zoomed in
    /// the same place stays in view, upside down. At once, as the original shows.
    /// The middle is on a whole or half physical pixel, so the turn keeps the
    /// resampled photograph on the pixel grid.
    private func placeTurn() {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let clipView = enclosingScrollView?.contentView
        let visible = clipView.map { convert($0.bounds, from: $0) }
        var centre = Self.turnCentre(visible: visible, bounds: CGRect(origin: .zero, size: bounds.size))
        if let window {
            centre = FitDownscale.snappedHalf(centre, toBacking: { window.backingPoint(self.convert($0, to: nil)) },
                                              fromBacking: { self.convert(window.pointFromBacking($0), from: nil) })
        }
        turn.anchorPoint = bounds.width > 0 && bounds.height > 0 ? CGPoint(x: centre.x / bounds.width, y: centre.y / bounds.height) : CGPoint(x: 0.5, y: 0.5)
        turn.position = centre
        turn.setAffineTransform(turned ? CGAffineTransform(rotationAngle: .pi) : .identity)
        CATransaction.commit()
    }
    /// The middle of the photograph's part on screen: all of it when fitted.
    static func turnCentre(visible: CGRect?, bounds: CGRect) -> CGPoint {
        let shown = visible.map { $0.intersection(bounds) } ?? bounds
        let area = shown.isNull || shown.isEmpty ? bounds : shown
        return CGPoint(x: area.midX, y: area.midY)
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
