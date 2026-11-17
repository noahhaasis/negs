import SwiftUI

/// The Color Filter module's glass discs (issue #21, design r30 · B): None,
/// then each filter in the catalog's order, the chosen one ringed in pencil.
/// Under them: its name and glass, what it does, and how much light it takes
/// on this film.
struct ColorFilterPicker: View {
    let model: EditorModel
    @State private var hovered: String?
    static let disc: CGFloat = 26

    var body: some View {
        let current = model.colorFilter
        let chosen = model.colorFilters.first { $0.key == current }
        VStack(alignment: .leading, spacing: 0) {
            // Each disc as wide as its name, the room left shared out between them.
            HStack(alignment: .top, spacing: 0) {
                disc(nil, selected: chosen == nil)
                ForEach(model.colorFilters) { filter in
                    Spacer(minLength: 2)
                    disc(filter, selected: filter.key == current)
                }
            }
            .padding(.horizontal, Theme.Size.gutter - 4)
            .padding(.top, 8)
            HStack(alignment: .firstTextBaseline, spacing: 7) {
                Text(chosen?.name ?? "None").font(Theme.serif(15, weight: 600)).foregroundStyle(Theme.paper)
                if let chosen { Text(chosen.glass).font(Theme.serif(13, italic: true)).foregroundStyle(Theme.dim) }
            }
            .padding(.horizontal, Theme.Size.gutter)
            .padding(.top, 10).padding(.bottom, 3)
            HintText(Self.hint(chosen, stops: chosen.flatMap { model.colorFilterStops($0.key) }, film: filmName, metered: metered))
            if chosen != nil, !model.filmIsBW {
                HintText("Taking filters are made for black-and-white film. On colour film this one tints the whole photograph.", warning: true)
            }
        }
    }

    private var filmName: String { model.films.first { $0.id == model.film }?.name ?? "this film" }
    /// Whether the exposure makes up the light the filter takes: auto
    /// exposure meters through it, and so does matching the camera's brightness.
    private var metered: Bool { model.value("camera.auto_exposure").bool || model.matchesBrightness }

    /// What the filter does, then what it costs the film:
    /// "… Takes 1,3 stops on Ilford HP5 Plus; auto exposure gives it."
    static func hint(_ filter: ColorFilter?, stops: Double?, film: String, metered: Bool) -> String {
        guard let filter else { return ColorFilter.effect(ColorFilter.none) }
        var text = ColorFilter.effect(filter.key)
        if let stops {
            let number = stops.formatted(.number.precision(.fractionLength(0...1)))
            text += " Takes \(number) stop\(abs(stops - 1) < 0.05 ? "" : "s") on \(film)"
            text += metered ? "; auto exposure gives it." : "."
        }
        return text
    }

    private func disc(_ filter: ColorFilter?, selected: Bool) -> some View {
        let key = filter?.key ?? ColorFilter.none
        let name = filter?.name ?? "None"
        let lit = selected || hovered == key
        return VStack(spacing: 7) {
            ZStack {
                if let filter {
                    Circle().fill(Theme.color(filter.colour))
                    // A highlight, so it reads as glass.
                    Circle().fill(RadialGradient(colors: [.white.opacity(0.5), .white.opacity(0)],
                                                 center: UnitPoint(x: 0.34, y: 0.3), startRadius: 0, endRadius: 11))
                } else {
                    Circle().fill(Theme.blank)
                    Capsule().fill(Theme.dim).frame(width: 17, height: 1.2).rotationEffect(.degrees(-45))
                }
                Circle().strokeBorder(Color.black.opacity(0.35), lineWidth: 1.5)
            }
            .frame(width: Self.disc, height: Self.disc)
            .overlay {
                if selected {
                    PencilRing(radii: .number).pencil(width: PencilRing.lineWidth)
                        .frame(width: 34, height: 32).rotationEffect(PencilRing.tilt(-8))
                }
            }
            Text(name)
                .font(Theme.serif(11.5, weight: selected ? 600 : 400))
                .foregroundStyle(selected ? Theme.paper : lit ? Theme.label : Theme.dim)
                .lineLimit(1).fixedSize()
        }
        .frame(minWidth: 34)
        .contentShape(Rectangle())
        .onHover { hovered = $0 ? key : (hovered == key ? nil : hovered) }
        .onTapGesture { if !selected { model.setColorFilter(key) } }
        .tooltip(filter.map { "\($0.name) · \($0.glass). \(ColorFilter.effect($0.key))" } ?? "No filter in front of the lens.")
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(filter.map { "\($0.name) filter, \($0.glass)" } ?? "No filter")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction { model.setColorFilter(key) }
    }
}
