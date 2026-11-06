import AppKit
import SwiftUI

/// The panel where `ZoneGeometry` says, reading the placement here so the
/// photograph's every pan redraws only this.
struct PlacedZonePanel: View {
    let model: EditorModel

    var body: some View {
        if let placement = model.zonePlacement, placement.frame.width > 0, placement.frame.height > 0 {
            ZonePanel(model: model, horizontal: placement.horizontal)
                .frame(width: placement.frame.width, height: placement.frame.height)
                .offset(x: placement.frame.minX, y: placement.frame.minY)
        }
    }
}

/// Adams' zones. Drag one onto the photograph and that spot's tone becomes that
/// zone. It sits flush against the photograph, so `ZonePlacement` and the
/// overlay's lines line up with its tiles.
struct ZonePanel: View {
    let model: EditorModel
    /// Black at the left, along a landscape photograph; otherwise white at the top.
    var horizontal = false
    static func label(for zone: Zone, placed: Bool) -> String {
        "Zone \(zone.numeral), \(zone.name), \(placed ? "placed" : "not placed")"
    }

    static func tooltip(for zone: Zone) -> String {
        "Zone \(zone.numeral): \(zone.name)\n\(zone.note)\nDrag onto the photograph to make that tone this zone."
    }

    var body: some View {
        let pins = model.pins
        let selected = model.selectedPin.flatMap { pins.indices.contains($0) ? pins[$0].zone : nil }
        let layout = horizontal ? AnyLayout(HStackLayout(spacing: 0)) : AnyLayout(VStackLayout(spacing: 0))
        layout {
            ForEach(horizontal ? ZoneScale.zones : ZoneScale.zones.reversed()) { zone in
                ZoneTile(zone: zone, placed: pins.contains { $0.zone == zone.index }, selected: selected == zone.index) { point in
                    guard model.canPlaceZones, let point = model.canvasScale.canvasPoint(window: point) else { return }
                    model.zoneDrag = ZoneDrag(zone: zone.index, point: point)
                } onDrop: { point in
                    model.zoneDrag = nil
                    guard model.canPlaceZones, let point = model.canvasScale.canvasPoint(window: point),
                          let unit = model.canvasScale.unitPoint(canvasPoint: point) else { return }
                    Task { await model.placeZone(zone.index, at: unit) }
                }
            }
        }
        .allowsHitTesting(model.canPlaceZones)
        .opacity(model.canPlaceZones ? 1 : 0.5)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Zones")
    }
}

/// One zone: its grey, its numeral, and a dot once it's placed.
private struct ZoneTile: View {
    let zone: Zone
    let placed: Bool
    let selected: Bool
    let onDrag: (CGPoint) -> Void
    let onDrop: (CGPoint) -> Void

    private var ink: Color { zone.target < 55 ? .white : .black }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Rectangle().fill(Color(nsColor: ZoneScale.grey(zone.index)))
            Text(zone.numeral)
                .font(.system(size: 14, weight: zone.index == 5 ? .heavy : .medium))
                .foregroundStyle(ink)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            if placed {
                Circle()
                    .fill(selected ? EdgePrint.amber : ink)
                    .frame(width: 6, height: 6)
                    .padding(6)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay { if selected { Rectangle().strokeBorder(EdgePrint.amber, lineWidth: 2.5) } }
        // AppKit, like the strip and the curve: the drag runs on over the canvas.
        .overlay { ZoneMouseSurface(tooltip: ZonePanel.tooltip(for: zone), onDrag: onDrag, onDrop: onDrop) }
        .accessibilityElement()
        .accessibilityLabel(ZonePanel.label(for: zone, placed: placed))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// Drags from a tile, with the points in window coordinates, and its open-hand
/// cursor and tooltip.
private struct ZoneMouseSurface: NSViewRepresentable {
    let tooltip: String
    let onDrag: (CGPoint) -> Void
    let onDrop: (CGPoint) -> Void

    final class Surface: NSView {
        var onDrag: (CGPoint) -> Void = { _ in }
        var onDrop: (CGPoint) -> Void = { _ in }
        private var start: CGPoint?
        private var dragging = false

        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
        override func mouseDown(with event: NSEvent) { start = event.locationInWindow; dragging = false }
        override func mouseDragged(with event: NSEvent) {
            guard let start else { return }
            let point = event.locationInWindow
            if !dragging, hypot(point.x - start.x, point.y - start.y) < 2 { return }
            dragging = true
            onDrag(point)
        }
        override func mouseUp(with event: NSEvent) {
            if dragging { onDrop(event.locationInWindow) }
            start = nil; dragging = false
        }
    }

    func makeNSView(context: Context) -> Surface { Surface() }
    func updateNSView(_ view: Surface, context: Context) {
        view.onDrag = onDrag; view.onDrop = onDrop
        view.toolTip = tooltip
    }
}
