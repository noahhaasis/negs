import ImageIO
import QuartzCore
import SwiftUI

/// Presentation borders. They are drawn around the developed photograph, so
/// changing one never re-renders. Sizes match `spektrafilm-io::border`, which
/// adds the same border on export; keep the two in sync.
enum BorderStyle: String, CaseIterable, Identifiable {
    case white, film, keyline

    var id: String { rawValue }
    var title: String {
        switch self {
        case .white: "White"
        case .film: "Film"
        case .keyline: "Keyline"
        }
    }
    var help: String {
        switch self {
        case .white: "A white mat around the photograph."
        case .film: "A scanned 120 film frame with an irregular black edge."
        case .keyline: "A white mat with a thin black line around the photograph."
        }
    }

    /// The border's width on each side of a photograph of `size` pixels.
    func inset(for size: CGSize) -> CGFloat {
        let long = max(size.width, size.height)
        switch self {
        case .white: return (long * 0.03).rounded(.down)
        case .keyline: return (long * 0.03).rounded(.down) + Self.keyline(for: size)
        case .film: return Self.film(for: size).inset
        }
    }
    static func keyline(for size: CGSize) -> CGFloat { max(1, (max(size.width, size.height) * 0.004).rounded()) }
    /// The film frame's canvas inset and nine-patch corner size.
    static func film(for size: CGSize) -> (inset: CGFloat, pad: CGFloat) {
        let short = min(size.width, size.height)
        return (max(1, (short * 0.003).rounded()), max(2, (short * 0.025).rounded()))
    }

    /// The film frame texture, decoded once.
    nonisolated static func loadFilmTexture(data: URL) -> CGImage? {
        let url = data.appendingPathComponent("borders/film.png")
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
    }
}

struct BorderSettings: Equatable {
    var enabled = false
    var style = BorderStyle.white
    var export = false

    var shown: BorderStyle? { enabled ? style : nil }
    var exported: BorderStyle? { enabled && export ? style : nil }
}

/// Core Animation layers that draw a border around the photograph layer.
/// The film texture is a nine-patch (`contentsCenter`), so resizing the
/// canvas or zooming never redraws anything.
final class BorderLayers {
    private let keyline = CALayer()
    private let film = CALayer()

    init(in host: CALayer, around photo: CALayer) {
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull(), "backgroundColor": NSNull(), "hidden": NSNull()]
        keyline.backgroundColor = .black
        keyline.actions = none
        film.actions = none
        film.minificationFilter = .trilinear
        film.magnificationFilter = .linear
        host.insertSublayer(keyline, below: photo)
        host.addSublayer(film)
        host.actions = none
    }

    /// Lays the border out around a photograph of `size` (as displayed,
    /// after rotation), centred in `host`.
    func layout(_ style: BorderStyle?, photo size: CGSize, host: CALayer, texture: CGImage?) {
        let center = CGPoint(x: host.bounds.midX, y: host.bounds.midY)
        host.backgroundColor = switch style {
        case .white, .keyline: .white
        case .film: .black
        case nil: nil
        }
        keyline.isHidden = style != .keyline
        if style == .keyline {
            let line = BorderStyle.keyline(for: size)
            keyline.bounds = CGRect(x: 0, y: 0, width: size.width + 2 * line, height: size.height + 2 * line)
            keyline.position = center
        }
        film.isHidden = style != .film || texture == nil
        guard style == .film, let texture else { film.contents = nil; return }
        // Lay the texture out landscape and turn it for portrait photographs,
        // like the exporter.
        let (inset, pad) = BorderStyle.film(for: size)
        let portrait = size.height > size.width
        let canvas = CGSize(width: size.width + 2 * inset, height: size.height + 2 * inset)
        let landscape = portrait ? CGSize(width: canvas.height, height: canvas.width) : canvas
        let (tw, th) = (CGFloat(texture.width), CGFloat(texture.height))
        let scale = min(landscape.width, landscape.height) / min(tw, th)
        if (film.contents as AnyObject?) !== texture { film.contents = texture }
        // Corners keep `pad` canvas pixels; edges and centre stretch.
        film.contentsScale = 1 / scale
        let (u, v) = (pad / scale / tw, pad / scale / th)
        film.contentsCenter = CGRect(x: u, y: v, width: 1 - 2 * u, height: 1 - 2 * v)
        film.bounds = CGRect(origin: .zero, size: landscape)
        film.position = center
        film.setAffineTransform(portrait ? CGAffineTransform(rotationAngle: .pi / 2) : .identity)
    }
}

/// Three small drawn samples to pick a style from.
struct BorderStylePicker: View {
    @Binding var selection: BorderStyle

    var body: some View {
        HStack(spacing: 10) {
            ForEach(BorderStyle.allCases) { style in
                Button { selection = style } label: {
                    VStack(spacing: 5) {
                        BorderSwatch(style: style)
                            .frame(width: 62, height: 44)
                            .padding(3)
                            .overlay {
                                RoundedRectangle(cornerRadius: 6)
                                    .strokeBorder(selection == style ? Color.accentColor : .clear, lineWidth: 2)
                            }
                        Text(style.title).font(.caption)
                            .foregroundStyle(selection == style ? Color.accentColor : .secondary)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(style.help)
                .accessibilityLabel("\(style.title) border")
                .accessibilityAddTraits(selection == style ? .isSelected : [])
            }
        }
        .frame(maxWidth: .infinity)
    }
}

private struct BorderSwatch: View {
    let style: BorderStyle
    private let photo = LinearGradient(colors: [Color(red: 0.55, green: 0.68, blue: 0.78), Color(red: 0.86, green: 0.74, blue: 0.56)], startPoint: .top, endPoint: .bottom)

    var body: some View {
        ZStack {
            switch style {
            case .white:
                Color.white
                Rectangle().fill(photo).padding(5)
            case .keyline:
                Color.white
                Rectangle().fill(photo).padding(1).background(Color.black).padding(4)
            case .film:
                Color.black
                RoundedRectangle(cornerRadius: 2).fill(photo).padding(2)
            }
        }
        .overlay(Rectangle().strokeBorder(.separator, lineWidth: 0.5))
        .accessibilityHidden(true)
    }
}
