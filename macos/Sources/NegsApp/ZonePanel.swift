import AppKit
import SwiftUI

/// Adams' zones, white at the top. Drag one onto the photograph and that spot's
/// tone becomes that zone. It sits directly left of the canvas, so
/// `ZoneGeometry` and the overlay's lines line up with its rows.
struct ZonePanel: View {
    let model: EditorModel
    /// The space shared with the canvas, for a drag that crosses into it.
    static let coordinateSpace = "zoneCanvas"

    static func label(for zone: Zone, placed: Bool) -> String {
        "Zone \(zone.numeral), \(zone.name), \(placed ? "placed" : "not placed")"
    }

    static func tooltip(for zone: Zone) -> String {
        "Zone \(zone.numeral): \(zone.name)\n\(zone.note)\nDrag onto the photograph to make that tone this zone."
    }

    var body: some View {
        let pins = model.pins
        let selected = model.selectedPin.flatMap { pins.indices.contains($0) ? pins[$0].zone : nil }
        VStack(spacing: 0) {
            ForEach(ZoneScale.zones.reversed()) { zone in
                ZoneTile(zone: zone, placed: pins.contains { $0.zone == zone.index }, selected: selected == zone.index) { point in
                    model.zoneDrag = ZoneDrag(zone: zone.index, point: canvasPoint(point))
                } onDrop: { point in
                    model.zoneDrag = nil
                    guard let unit = model.canvasScale.unitPoint(canvasPoint: canvasPoint(point)) else { return }
                    Task { await model.placeZone(zone.index, at: unit) }
                }
            }
        }
        .frame(width: ZoneGeometry.panelWidth)
        .allowsHitTesting(model.canPlaceZones)
        .opacity(model.canPlaceZones ? 1 : 0.5)
        // A mid grey, so it shows against a white mat and the darkest tile alike.
        .overlay(alignment: .trailing) { Rectangle().fill(Color(white: 0.5)).frame(width: 1).allowsHitTesting(false) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Zones")
    }

    /// The panel's own width comes off a drag's position, as the canvas starts
    /// where the panel ends.
    private func canvasPoint(_ point: CGPoint) -> CGPoint {
        CGPoint(x: point.x - ZoneGeometry.panelWidth, y: point.y)
    }
}

/// One zone: its grey, its numeral, and a dot once it's placed.
private struct ZoneTile: View {
    let zone: Zone
    let placed: Bool
    let selected: Bool
    let onDrag: (CGPoint) -> Void
    let onDrop: (CGPoint) -> Void
    @State private var hovering = false

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
                    .fill(selected ? Color.accentColor : ink)
                    .frame(width: 6, height: 6)
                    .padding(6)
            }
        }
        .frame(maxHeight: .infinity)
        .overlay { if selected { Rectangle().strokeBorder(Color.accentColor, lineWidth: 2.5) } }
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 2, coordinateSpace: .named(ZonePanel.coordinateSpace))
                .onChanged { onDrag($0.location) }
                .onEnded { onDrop($0.location) }
        )
        .onHover { inside in
            guard inside != hovering else { return }
            hovering = inside
            if inside { NSCursor.openHand.push() } else { NSCursor.pop() }
        }
        // Hiding the panel under the pointer sends no hover event.
        .onDisappear { if hovering { hovering = false; NSCursor.pop() } }
        .help(ZonePanel.tooltip(for: zone))
        .accessibilityElement()
        .accessibilityLabel(ZonePanel.label(for: zone, placed: placed))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
