import AppKit
import QuartzCore

/// Where the zone panel's tiles sit. The panel spans the canvas's height
/// directly to its left, so a tile's right edge is the canvas's left edge.
enum ZoneGeometry {
    static let panelWidth: CGFloat = 56
    static let chipDiameter: CGFloat = 24

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
    private(set) var pins: [TonePinMarker] = []
    private(set) var dragged: ZoneDrag?
    private(set) var links: [Int: (from: CGPoint, to: CGPoint)] = [:]

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
        selectedFill.fillColor = NSColor.controlAccentColor.withAlphaComponent(0.14).cgColor
        normal.strokeColor = NSColor.white.cgColor
        selected.strokeColor = NSColor.controlAccentColor.cgColor
        ignored.strokeColor = NSColor.systemRed.cgColor
        for shape in [normal, selected, ignored] { shape.lineWidth = 1.75 }
        dragChip.isHidden = true
        layer?.addSublayer(dragChip)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func update(pins: [TonePinMarker], dragged: ZoneDrag?) {
        self.pins = pins
        self.dragged = dragged
        redraw()
    }

    /// The pin whose chip is under `point`. Only the chip takes the pointer,
    /// so a plain drag inside a large circle still pans.
    func pin(at point: CGPoint) -> Int? {
        pins.last { marker in
            chipCentres[marker.index].map { hypot(point.x - $0.x, point.y - $0.y) <= ZoneGeometry.chipDiameter / 2 } ?? false
        }?.index
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
        let all = CGMutablePath(), plain = CGMutablePath(), accent = CGMutablePath(), red = CGMutablePath(), fill = CGMutablePath()
        let scale = window?.backingScaleFactor ?? 2
        func tile(_ zone: Int) -> CGPoint {
            CGPoint(x: 0, y: bounds.height - ZoneGeometry.tileCentreY(zone: zone, height: bounds.height))
        }
        func circle(_ centre: CGPoint, _ radius: CGFloat) -> CGPath {
            CGPath(ellipseIn: CGRect(x: centre.x - radius, y: centre.y - radius, width: radius * 2, height: radius * 2), transform: nil)
        }
        var live = Set<Int>()
        links = [:]
        chipCentres = [:]
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
            if radius > ZoneGeometry.chipDiameter / 2 {
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
            Self.style(chip, zone: marker.zone, at: centre, scale: scale,
                       edge: marker.ignored ? .systemRed : marker.selected ? .controlAccentColor : .white,
                       edgeWidth: marker.selected || marker.ignored ? 2.5 : 1.75)
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
            if radius > ZoneGeometry.chipDiameter / 2 {
                all.addPath(circle(point, radius))
                plain.addPath(circle(point, radius))
            }
            Self.style(dragChip, zone: dragged.zone, at: point, scale: scale, edge: .white, edgeWidth: 1.75)
        }
        dragChip.isHidden = dragged == nil
        casing.path = all.isEmpty ? nil : all
        selectedFill.path = fill.isEmpty ? nil : fill
        normal.path = plain.isEmpty ? nil : plain
        selected.path = accent.isEmpty ? nil : accent
        ignored.path = red.isEmpty ? nil : red
        CATransaction.commit()
    }

    // MARK: Chips

    /// A dark outline holding the zone's grey, its edge and its numeral, so it
    /// reads on any tone of any photograph.
    private static func makeChip() -> CALayer {
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "backgroundColor": NSNull(),
                    "borderWidth": NSNull(), "borderColor": NSNull(), "foregroundColor": NSNull(), "fontSize": NSNull()]
        let outline = CALayer()
        outline.actions = none
        outline.bounds = CGRect(x: 0, y: 0, width: ZoneGeometry.chipDiameter + 3, height: ZoneGeometry.chipDiameter + 3)
        outline.cornerRadius = outline.bounds.width / 2
        outline.backgroundColor = NSColor.black.withAlphaComponent(0.55).cgColor
        let face = CALayer()
        face.actions = none
        face.frame = outline.bounds.insetBy(dx: 1.5, dy: 1.5)
        face.cornerRadius = ZoneGeometry.chipDiameter / 2
        outline.addSublayer(face)
        let label = CATextLayer()
        label.actions = none
        label.alignmentMode = .center
        label.font = NSFont.systemFont(ofSize: 0, weight: .bold)
        face.addSublayer(label)
        return outline
    }

    private static func style(_ outline: CALayer, zone: Int, at centre: CGPoint, scale: CGFloat, edge: NSColor, edgeWidth: CGFloat) {
        outline.position = centre
        guard let face = outline.sublayers?.first, let label = face.sublayers?.first as? CATextLayer else { return }
        face.backgroundColor = ZoneScale.grey(zone).cgColor
        face.borderColor = edge.cgColor
        face.borderWidth = edgeWidth
        let numeral = ZoneLadder.names[zone]
        let size: CGFloat = numeral.count > 3 ? 9.5 : numeral.count > 2 ? 11 : 12.5
        label.string = numeral
        label.fontSize = size
        // CATextLayer draws from its top, so centring the line means dropping
        // it by half the space its height leaves.
        let diameter = ZoneGeometry.chipDiameter
        label.frame = CGRect(x: 0, y: -(diameter - size * 1.2) / 2, width: diameter, height: diameter)
        label.foregroundColor = (ZoneScale.zones[zone].target < 55 ? NSColor.white : NSColor.black).cgColor
        label.contentsScale = scale
    }
}
