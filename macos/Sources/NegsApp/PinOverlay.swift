import AppKit
import QuartzCore

/// Where the zone panel sits on the canvas, from its top left.
struct ZonePlacement: Equatable {
    let frame: CGRect
    /// Dark to light along the top of a landscape photograph; otherwise light
    /// to dark down the left of a portrait one.
    let horizontal: Bool

    /// The middle of a zone's tile, on the edge that faces the photograph.
    func tile(zone: Int) -> CGPoint {
        horizontal
            ? CGPoint(x: frame.minX + frame.width * (CGFloat(zone) + 0.5) / CGFloat(ZoneScale.zones.count), y: frame.maxY)
            : CGPoint(x: frame.maxX, y: frame.minY + ZoneGeometry.tileCentreY(zone: zone, height: frame.height))
    }
}

/// Where the zone panel's tiles sit: flush against the photograph.
enum ZoneGeometry {
    /// How far the panel stands off the photograph.
    static let panelDepth: CGFloat = 56

    /// Against the top of a landscape photograph, or the left of a portrait one,
    /// and as long as it is. Whatever the photograph runs past the canvas by,
    /// the panel stops at the canvas's edge, staying within reach.
    static func placement(photo: CGRect, canvas: CGSize) -> ZonePlacement {
        let horizontal = photo.width >= photo.height
        let (minX, maxX) = (max(photo.minX, 0), min(photo.maxX, canvas.width))
        let (minY, maxY) = (max(photo.minY, 0), min(photo.maxY, canvas.height))
        return ZonePlacement(
            frame: horizontal
                ? CGRect(x: minX, y: max(photo.minY - panelDepth, 0), width: max(maxX - minX, 0), height: panelDepth)
                : CGRect(x: max(photo.minX - panelDepth, 0), y: minY, width: panelDepth, height: max(maxY - minY, 0)),
            horizontal: horizontal
        )
    }

    /// What the canvas keeps clear around a document of `size`.
    static func insets(document size: CGSize, zones: Bool) -> NSEdgeInsets {
        let landscape = size.width >= size.height
        return NSEdgeInsets(top: zones && landscape ? panelDepth : 0, left: zones && !landscape ? panelDepth : 0,
                            bottom: PinOverlayView.exifMargin, right: 0)
    }

    /// The dot on the exact spot a pin measures.
    static let markerDiameter: CGFloat = 8
    /// How close a click on the small marker has to be.
    static let markerReach: CGFloat = 10
    static let labelHeight: CGFloat = 18
    /// The gap between the marker's centre and its label.
    static let labelOffset: CGFloat = 11
    /// A circle has to clear the marker to be seen.
    static let smallestCircle: CGFloat = 5

    static func labelWidth(_ numeral: String) -> CGFloat { numeral.count > 1 ? 26 : 20 }

    /// A tile's row centre, from the top: zone 0 in the bottom row.
    static func tileCentreY(zone: Int, height: CGFloat) -> CGFloat {
        let count = ZoneScale.zones.count
        return height * (CGFloat(count - 1 - zone) + 0.5) / CGFloat(count)
    }
}

/// The pins and the lines linking them to their zones, over the canvas. It
/// sits above the document, so the photograph's zoom, rotation and crop never
/// touch it, and it takes no mouse events.
final class PinOverlayView: NSView {
    /// The camera details, along the bottom of the photograph: the exposure at
    /// its left and the camera at its right.
    private let exifCamera = CATextLayer()
    private let exifExposure = CATextLayer()
    /// The lines' height, and their gap from the photograph.
    static let exifHeight: CGFloat = 18, exifGap: CGFloat = 4
    /// How much room a fitted photograph leaves above and below for them.
    static var exifMargin: CGFloat { exifHeight + exifGap * 2 }
    private(set) var exifLines: (camera: String, exposure: String)?
    private let casing = CAShapeLayer()
    private let selectedFill = CAShapeLayer()
    private let normal = CAShapeLayer()
    private let selected = CAShapeLayer()
    private let ignored = CAShapeLayer()
    /// One chip per pin, by the pin's index.
    private var chips: [Int: CALayer] = [:]
    /// The chip of the zone being dragged.
    private let dragChip = PinOverlayView.makeChip()
    private var chipCentres: [Int: CGPoint] = [:]
    private var labelFrames: [Int: CGRect] = [:]
    private var circles: [Int: (centre: CGPoint, radius: CGFloat)] = [:]
    private(set) var drawnCircles = 0
    private(set) var pins: [TonePinMarker] = []
    private(set) var dragged: ZoneDrag?
    private(set) var links: [Int: (from: CGPoint, to: CGPoint)] = [:]
    /// Where the zone panel goes against the photograph as it stands.
    private(set) var zonePlacement: ZonePlacement?
    var onZonePlacement: ((ZonePlacement) -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        autoresizingMask = [.width, .height]
        let none = ["path": NSNull(), "bounds": NSNull(), "position": NSNull()]
        for shape in [casing, selectedFill, normal, selected, ignored] {
            shape.actions = none
            shape.fillColor = nil
            shape.lineCap = .round
            layer?.addSublayer(shape)
        }
        casing.strokeColor = NSColor.black.withAlphaComponent(0.6).cgColor
        casing.lineWidth = 4
        selectedFill.fillColor = EdgePrint.colour.withAlphaComponent(0.14).cgColor
        normal.strokeColor = NSColor.white.cgColor
        selected.strokeColor = EdgePrint.colour.cgColor
        ignored.strokeColor = NSColor.systemRed.cgColor
        for shape in [normal, selected, ignored] { shape.lineWidth = 1.75 }
        dragChip.isHidden = true
        layer?.addSublayer(dragChip)
        exifCamera.alignmentMode = .right
        for line in [exifCamera, exifExposure] {
            line.actions = none.merging(["string": NSNull(), "contents": NSNull(), "hidden": NSNull()]) { $1 }
            line.foregroundColor = EdgePrint.colour.cgColor
            line.isHidden = true
            layer?.insertSublayer(line, at: 0)
        }
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func update(pins: [TonePinMarker], dragged: ZoneDrag?) {
        self.pins = pins
        self.dragged = dragged
        redraw()
    }

    /// The pin whose marker or label is under `point`. Only those take the
    /// pointer, so a plain drag inside a large circle still pans.
    func pin(at point: CGPoint) -> Int? {
        pins.last { marker in
            if let label = labelFrames[marker.index], label.contains(point) { return true }
            return chipCentres[marker.index].map { hypot(point.x - $0.x, point.y - $0.y) <= ZoneGeometry.markerReach } ?? false
        }?.index
    }

    func labelFrame(_ index: Int) -> CGRect? { labelFrames[index] }

    func showExif(_ exif: PhotoExif?) {
        let lines = exif.map { (camera: $0.cameraLine, exposure: $0.exposureLine) }
        guard lines?.camera != exifLines?.camera || lines?.exposure != exifLines?.exposure else { return }
        exifLines = lines
        redraw()
    }

    /// The two lines' frames, for tests.
    var exifFrames: (camera: CGRect, exposure: CGRect)? { exifLines == nil ? nil : (exifCamera.frame, exifExposure.frame) }

    /// The lines sit just under the photograph, and stay on the canvas when
    /// it's zoomed past the edges.
    private func layoutExif(photo: PhotographView) {
        let rect = convert(photo.bounds, from: photo)
        let scale = window?.backingScaleFactor ?? 2
        let (height, gap) = (Self.exifHeight, Self.exifGap)
        let font = EdgePrint.font(size: 14)
        let left = max(rect.minX, gap * 2), right = max(min(rect.maxX, bounds.width - gap * 2), left + 1)
        let (camera, exposure) = (exifLines?.camera ?? "", exifLines?.exposure ?? "")
        func width(_ text: String) -> CGFloat { (text as NSString).size(withAttributes: [.font: font]).width }
        // On one row where they fit, or the camera under the exposure on a narrow photograph.
        let fits = camera.isEmpty || exposure.isEmpty || width(camera) + width(exposure) + gap * 4 <= right - left
        let row = max(rect.minY - gap - height, gap)
        for (line, text, y, x, wide) in [(exifExposure, exposure, row, left, max(bounds.width - left - gap * 2, 1)),
                                         (exifCamera, camera, fits ? row : max(row - height, gap), min(left, right - 1), right - min(left, right - 1))] {
            line.isHidden = text.isEmpty
            line.string = text
            line.font = font
            line.fontSize = 14
            line.contentsScale = scale
            line.frame = CGRect(x: x, y: y, width: wide, height: height)
        }
    }

    /// The pin whose circle holds `point`; the smallest, where they overlap.
    func pinCircle(at point: CGPoint) -> Int? {
        circles.filter { hypot(point.x - $0.value.centre.x, point.y - $0.value.centre.y) <= $0.value.radius }
            .min { $0.value.radius < $1.value.radius }?.key
    }

    /// A chip's frame in this view, for tests.
    func chipLayer(_ index: Int) -> CALayer? { chips[index] }
    var isShowingIgnoredLink: Bool { ignored.path != nil }
    var dragChipCentre: CGPoint? { dragChip.isHidden ? nil : dragChip.position }

    /// Draws again from the photograph's current zoom, scroll and crop.
    func redraw() {
        guard let scroll = enclosingScrollView ?? superview as? NSScrollView,
              let photo = scroll.documentView as? PhotographView else { return }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        layoutExif(photo: photo)
        let all = CGMutablePath(), plain = CGMutablePath(), accent = CGMutablePath(), red = CGMutablePath(), fill = CGMutablePath()
        let scale = window?.backingScaleFactor ?? 2
        let rect = convert(photo.bounds, from: photo)
        let placement = ZoneGeometry.placement(
            photo: CGRect(x: rect.minX, y: bounds.height - rect.maxY, width: rect.width, height: rect.height),
            canvas: bounds.size
        )
        if placement != zonePlacement {
            zonePlacement = placement
            onZonePlacement?(placement)
        }
        func tile(_ zone: Int) -> CGPoint {
            let point = placement.tile(zone: zone)
            return CGPoint(x: point.x, y: bounds.height - point.y)
        }
        func circle(_ centre: CGPoint, _ radius: CGFloat) -> CGPath {
            CGPath(ellipseIn: CGRect(x: centre.x - radius, y: centre.y - radius, width: radius * 2, height: radius * 2), transform: nil)
        }
        var live = Set<Int>()
        links = [:]
        chipCentres = [:]
        labelFrames = [:]
        circles = [:]
        drawnCircles = 0
        for marker in pins {
            // A pin outside the crop has no spot on screen to point at.
            guard let inPhoto = photo.centre(of: marker) else { continue }
            let centre = convert(inPhoto, from: photo)
            live.insert(marker.index)
            links[marker.index] = (tile(marker.zone), centre)
            chipCentres[marker.index] = centre
            let path = marker.ignored ? red : marker.selected ? accent : plain
            for target in [all, path] {
                target.move(to: tile(marker.zone))
                target.addLine(to: centre)
            }
            let radius = photo.radiusInPoints(marker.source.radius) * scroll.magnification
            if radius >= ZoneGeometry.smallestCircle {
                drawnCircles += 1
                circles[marker.index] = (centre, radius)
                all.addPath(circle(centre, radius))
                path.addPath(circle(centre, radius))
                if marker.selected { fill.addPath(circle(centre, radius)) }
            }
            let chip = chips[marker.index] ?? {
                let chip = Self.makeChip()
                layer?.addSublayer(chip)
                chips[marker.index] = chip
                return chip
            }()
            labelFrames[marker.index] = Self.style(
                chip, zone: marker.zone, at: centre, scale: scale, canvasWidth: bounds.width,
                edge: marker.ignored ? .systemRed : marker.selected ? EdgePrint.colour : .white,
                edgeWidth: marker.selected || marker.ignored ? 2 : 1.5
            )
        }
        for (index, chip) in chips where !live.contains(index) {
            chip.removeFromSuperlayer()
            chips.removeValue(forKey: index)
        }
        if let dragged {
            let point = CGPoint(x: dragged.point.x, y: bounds.height - dragged.point.y)
            for target in [all, plain] {
                target.move(to: tile(dragged.zone))
                target.addLine(to: point)
            }
            let radius = photo.radiusInPoints(photo.draggedRadius) * scroll.magnification
            if radius >= ZoneGeometry.smallestCircle {
                all.addPath(circle(point, radius))
                plain.addPath(circle(point, radius))
            }
            _ = Self.style(dragChip, zone: dragged.zone, at: point, scale: scale, canvasWidth: bounds.width, edge: .white, edgeWidth: 1.5)
        }
        dragChip.isHidden = dragged == nil
        casing.path = all.isEmpty ? nil : all
        selectedFill.path = fill.isEmpty ? nil : fill
        normal.path = plain.isEmpty ? nil : plain
        selected.path = accent.isEmpty ? nil : accent
        ignored.path = red.isEmpty ? nil : red
        CATransaction.commit()
    }

    // MARK: Markers

    /// A small dot with a dark outline on the exact spot, and the zone's number
    /// beside it, so the circle can shrink without the label hiding it. Both are
    /// filled with the zone's grey and read on any tone of any photograph.
    private static func makeChip() -> CALayer {
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "backgroundColor": NSNull(),
                    "borderWidth": NSNull(), "borderColor": NSNull(), "foregroundColor": NSNull(), "fontSize": NSNull()]
        func layer(_ made: CALayer = CALayer()) -> CALayer { made.actions = none; return made }
        let outline = layer()
        let side = ZoneGeometry.markerDiameter + 3
        outline.bounds = CGRect(x: 0, y: 0, width: side, height: side)
        outline.cornerRadius = side / 2
        outline.backgroundColor = NSColor.black.withAlphaComponent(0.55).cgColor
        let dot = layer()
        dot.frame = outline.bounds.insetBy(dx: 1.5, dy: 1.5)
        dot.cornerRadius = ZoneGeometry.markerDiameter / 2
        outline.addSublayer(dot)
        let casing = layer()
        casing.backgroundColor = NSColor.black.withAlphaComponent(0.55).cgColor
        casing.cornerRadius = ZoneGeometry.labelHeight / 2 + 1.5
        outline.addSublayer(casing)
        let tag = layer()
        tag.cornerRadius = ZoneGeometry.labelHeight / 2
        casing.addSublayer(tag)
        let label = CATextLayer()
        _ = layer(label)
        label.alignmentMode = .center
        label.font = NSFont.systemFont(ofSize: 0, weight: .bold)
        tag.addSublayer(label)
        return outline
    }

    /// Styles a marker and returns its label's frame in the overlay.
    @discardableResult
    private static func style(_ outline: CALayer, zone: Int, at centre: CGPoint, scale: CGFloat, canvasWidth: CGFloat, edge: NSColor, edgeWidth: CGFloat) -> CGRect {
        outline.position = centre
        guard let parts = outline.sublayers, parts.count == 2, let casing = parts.last,
              let tag = casing.sublayers?.first, let label = tag.sublayers?.first as? CATextLayer else { return .zero }
        let grey = ZoneScale.grey(zone).cgColor
        parts[0].backgroundColor = grey
        parts[0].borderColor = edge.cgColor
        parts[0].borderWidth = edgeWidth
        let numeral = ZoneScale.zones[zone].numeral
        let (width, height) = (ZoneGeometry.labelWidth(numeral), ZoneGeometry.labelHeight)
        // Beside the marker, or on its left where the canvas ends.
        let side = centre.x + ZoneGeometry.labelOffset + width > canvasWidth ? -ZoneGeometry.labelOffset - width : ZoneGeometry.labelOffset
        let mid = outline.bounds.width / 2
        casing.frame = CGRect(x: mid + side - 1.5, y: mid - height / 2 - 1.5, width: width + 3, height: height + 3)
        tag.frame = casing.bounds.insetBy(dx: 1.5, dy: 1.5)
        tag.backgroundColor = grey
        tag.borderColor = edge.cgColor
        tag.borderWidth = edgeWidth
        let size: CGFloat = 12
        label.string = numeral
        label.fontSize = size
        // CATextLayer draws from its top, so centring the line means dropping
        // it by half the space its height leaves.
        label.frame = CGRect(x: 0, y: -(height - size * 1.2) / 2, width: width, height: height)
        label.foregroundColor = (ZoneScale.zones[zone].target < 55 ? NSColor.white : NSColor.black).cgColor
        label.contentsScale = scale
        return CGRect(x: centre.x + side, y: centre.y - height / 2, width: width, height: height)
    }
}
