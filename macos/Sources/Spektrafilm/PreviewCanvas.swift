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
    let border: BorderStyle?
    let filmTexture: CGImage?
    let zoomCommand: UUID
    let fit: Bool

    func makeNSView(context: Context) -> ImageScrollView {
        let scroll = ImageScrollView()
        scroll.contentView = CenteringClipView()
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true; scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        scroll.documentView = PhotographView()
        scroll.setAccessibilityLabel("Photograph preview")
        scroll.setAccessibilityHelp("Pinch to zoom, scroll or drag to pan, double-click to fit.")
        return scroll
    }

    func updateNSView(_ scroll: ImageScrollView, context: Context) {
        let photo = scroll.documentView as! PhotographView
        let displaySize = rotation % 2 != 0 ? CGSize(width: size.height, height: size.width) : size
        let inset = border?.inset(for: displaySize) ?? 0
        let documentSize = CGSize(width: displaySize.width + 2 * inset, height: displaySize.height + 2 * inset)
        let oldSize = photo.frame.size, oldPhotoSize = photo.displaySize
        let commanded = scroll.lastCommand != zoomCommand
        photo.show(image, size: size, rotation: rotation, border: border, filmTexture: filmTexture)
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
        } else if oldSize != documentSize, scroll.fitting { scroll.applyZoom() }
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
        let amount = fitting ? min(contentSize.width / size.width, contentSize.height / size.height) : 1 / (window?.backingScaleFactor ?? 2)
        setMagnification(min(max(amount, minMagnification), maxMagnification), centeredAt: NSPoint(x: size.width / 2, y: size.height / 2))
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
    private let photo = CALayer()
    private var borderLayers: BorderLayers!
    private var image: AnyObject?
    private var size = CGSize.zero
    private var rotation = 0
    private var border: BorderStyle?
    private var filmTexture: CGImage?
    /// The photograph's size on the canvas, after rotation.
    var displaySize: CGSize { rotation % 2 != 0 ? CGSize(width: size.height, height: size.width) : size }
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
        photo.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull()]
        layer?.addSublayer(photo)
        borderLayers = BorderLayers(in: layer!, around: photo)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ image: AnyObject, size: CGSize, rotation: Int, border: BorderStyle? = nil, filmTexture: CGImage? = nil) {
        guard image !== self.image || size != self.size || rotation != self.rotation
            || border != self.border || filmTexture !== self.filmTexture else { return }
        self.image = image; self.size = size; self.rotation = rotation
        self.border = border; self.filmTexture = filmTexture
        photo.contents = image
        needsLayout = true
    }

    override func layout() {
        super.layout()
        CATransaction.begin(); CATransaction.setDisableActions(true)
        photo.bounds = CGRect(origin: .zero, size: size)
        photo.position = CGPoint(x: bounds.midX, y: bounds.midY)
        photo.setAffineTransform(CGAffineTransform(rotationAngle: -.pi / 2 * CGFloat(rotation)))
        borderLayers.layout(border, photo: displaySize, host: layer!, texture: filmTexture)
        CATransaction.commit()
    }

    private var grabbing = false
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 2, let scroll = enclosingScrollView as? ImageScrollView {
            scroll.fitting = true; scroll.applyZoom()
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
