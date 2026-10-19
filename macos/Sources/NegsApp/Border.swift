import ImageIO
import QuartzCore
import SwiftUI

/// Presentation borders. They are drawn around the developed photograph, so
/// changing one never re-renders. Sizes match `spektrafilm-io::border`, which
/// adds the same border on export; keep the two in sync.
enum BorderStyle: String, Codable, Sendable, CaseIterable, Identifiable {
    case white, film, keyline
    case film35 = "35mm"

    var id: String { rawValue }
    var title: String {
        switch self {
        case .white: "White"
        case .film: "Film"
        case .keyline: "Keyline"
        case .film35: "35mm"
        }
    }
    var help: String {
        switch self {
        case .white: "A white mat around the photograph."
        case .film: "A scanned 120 film frame with an irregular black edge."
        case .keyline: "A white mat with a thin black line around the photograph."
        case .film35: "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph."
        }
    }

    /// Whether the style fits a photograph of `size`: the 35mm strip only
    /// holds 3:2 and 3:4 photographs.
    func fits(_ size: CGSize) -> Bool { self != .film35 || FilmFormat(size) != nil }

    /// The canvas around a photograph of `size` pixels. The 35mm canvas
    /// comes from its `strip`, and is the photograph alone until that loads.
    func canvas(for size: CGSize, strip: FilmStrip? = nil) -> CGSize {
        let long = max(size.width, size.height)
        let inset: CGFloat
        switch self {
        case .white: inset = (long * 0.03).rounded(.down)
        case .keyline: inset = (long * 0.03).rounded(.down) + Self.keyline(for: size)
        case .film: inset = Self.film(for: size).inset
        case .film35:
            guard let strip, strip.format == FilmFormat(size) else { return size }
            return strip.canvas(for: size)
        }
        return CGSize(width: size.width + 2 * inset, height: size.height + 2 * inset)
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

struct BorderSettings: Codable, Equatable, Sendable {
    var enabled = false
    var style = BorderStyle.white
    var export = true

    /// The border around a photograph of `size`, if the style fits it.
    func shown(for size: CGSize) -> BorderStyle? { enabled && style.fits(size) ? style : nil }
    func exported(for size: CGSize) -> BorderStyle? { export ? shown(for: size) : nil }
}

/// The 35mm frame a photograph fills, chosen by its aspect ratio, like
/// `FilmFormat` in `spektrafilm-io::border`.
enum FilmFormat: String {
    /// 36 × 24 mm, 3:2.
    case full
    /// 18 × 24 mm, 3:4.
    case half

    /// The format whose frame matches `size` in either orientation, to within 1%.
    init?(_ size: CGSize) {
        guard min(size.width, size.height) > 0 else { return nil }
        let ratio = max(size.width, size.height) / min(size.width, size.height)
        if abs(ratio / 1.5 - 1) <= 0.01 { self = .full }
        else if abs(ratio / (4.0 / 3.0) - 1) <= 0.01 { self = .half }
        else { return nil }
    }
}

/// A 35mm strip texture from `data/borders/35mm`, laid out with the strip
/// horizontal, and the window the photograph shows through as fractions of
/// it (origin top left). The exporter composites the same files.
struct FilmStrip: Equatable {
    let film: String
    let format: FilmFormat
    /// Downsampled for the preview; `pixelSize` is the file's.
    let image: CGImage
    let pixelSize: CGSize
    let window: CGRect

    static func == (a: Self, b: Self) -> Bool { a.film == b.film && a.format == b.format && a.image === b.image }

    /// The film's strip, or the format's generic one.
    nonisolated static func load(data: URL, film: String, format: FilmFormat) -> FilmStrip? {
        struct Frame: Decodable { let window: [Double] }
        let root = data.appendingPathComponent("borders/35mm")
        guard let json = try? Data(contentsOf: root.appendingPathComponent("frames.json")),
              let frames = try? JSONDecoder().decode([String: Frame].self, from: json),
              let w = frames[format.rawValue]?.window, w.count == 4 else { return nil }
        let dir = root.appendingPathComponent(format.rawValue)
        var url = dir.appendingPathComponent("\(film).png")
        if !FileManager.default.fileExists(atPath: url.path) { url = dir.appendingPathComponent("generic.png") }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                  kCGImageSourceCreateThumbnailFromImageAlways: true,
                  kCGImageSourceThumbnailMaxPixelSize: 4096,
                  kCGImageSourceShouldCacheImmediately: true,
              ] as CFDictionary) else { return nil }
        return FilmStrip(film: film, format: format, image: image, pixelSize: CGSize(width: width, height: height),
                         window: CGRect(x: w[0], y: w[1], width: w[2], height: w[3]))
    }

    /// Whether the strip turns to hold a photograph of `size`.
    func turned(for size: CGSize) -> Bool {
        (window.width * pixelSize.width > window.height * pixelSize.height) != (size.width > size.height)
    }

    /// The strip scaled so its window covers a photograph of `size`, the
    /// same canvas the exporter makes.
    func canvas(for size: CGSize) -> CGSize {
        var (texture, frame) = (pixelSize, window.size)
        if turned(for: size) {
            texture = CGSize(width: texture.height, height: texture.width)
            frame = CGSize(width: frame.height, height: frame.width)
        }
        let k = max(size.width / (frame.width * texture.width), size.height / (frame.height * texture.height))
        return CGSize(width: (texture.width * k).rounded(), height: (texture.height * k).rounded())
    }
}

/// Core Animation layers that draw a border around the photograph layer.
/// The film texture is a nine-patch (`contentsCenter`), so resizing the
/// canvas or zooming never redraws anything.
final class BorderLayers {
    private let keyline = CALayer()
    private let film = CALayer()
    private let strip = CALayer()

    init(in host: CALayer, around photo: CALayer) {
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull(), "backgroundColor": NSNull(), "hidden": NSNull()]
        keyline.backgroundColor = .black
        keyline.actions = none
        for layer in [film, strip] {
            layer.actions = none
            layer.minificationFilter = .trilinear
            layer.magnificationFilter = .linear
            host.addSublayer(layer)
        }
        host.insertSublayer(keyline, below: photo)
        host.actions = none
    }

    /// Lays the border out around a photograph of `size` (as displayed,
    /// after rotation), centred in `host`.
    func layout(_ style: BorderStyle?, photo size: CGSize, host: CALayer, texture: CGImage?, filmStrip: FilmStrip?) {
        let center = CGPoint(x: host.bounds.midX, y: host.bounds.midY)
        host.backgroundColor = switch style {
        case .white, .keyline: .white
        case .film, .film35: .black
        case nil: nil
        }
        keyline.isHidden = style != .keyline
        if style == .keyline {
            let line = BorderStyle.keyline(for: size)
            keyline.bounds = CGRect(x: 0, y: 0, width: size.width + 2 * line, height: size.height + 2 * line)
            keyline.position = center
        }
        layoutStrip(style == .film35 ? filmStrip.flatMap { $0.format == FilmFormat(size) ? $0 : nil } : nil, photo: size, center: center)
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

    /// Scales the strip so its window covers the photograph, and turns it
    /// when the photograph's orientation needs it, like the exporter.
    private func layoutStrip(_ filmStrip: FilmStrip?, photo size: CGSize, center: CGPoint) {
        strip.isHidden = filmStrip == nil
        guard let filmStrip else { strip.contents = nil; return }
        if (strip.contents as AnyObject?) !== filmStrip.image { strip.contents = filmStrip.image }
        let canvas = filmStrip.canvas(for: size)
        let turned = filmStrip.turned(for: size)
        strip.bounds = CGRect(origin: .zero, size: turned ? CGSize(width: canvas.height, height: canvas.width) : canvas)
        strip.position = center
        strip.setAffineTransform(turned ? CGAffineTransform(rotationAngle: .pi / 2) : .identity)
    }
}

/// Small drawn samples to pick a style from. Styles that don't fit the
/// photograph are disabled.
struct BorderStylePicker: View {
    @Binding var selection: BorderStyle
    var photo: CGSize?

    var body: some View {
        HStack(spacing: 8) {
            ForEach(BorderStyle.allCases) { style in
                Button { selection = style } label: {
                    VStack(spacing: 5) {
                        BorderSwatch(style: style)
                            .frame(width: 50, height: 36)
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
                .disabled(photo.map { !style.fits($0) } ?? false)
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
    @Environment(\.isEnabled) private var enabled
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
            case .film35:
                Color.black
                VStack(spacing: 0) {
                    perforations
                    Rectangle().fill(photo).padding(.horizontal, 1).padding(.vertical, 2)
                    perforations
                }
            }
        }
        .overlay(Rectangle().strokeBorder(.separator, lineWidth: 0.5))
        .opacity(enabled ? 1 : 0.35)
        .accessibilityHidden(true)
    }

    private var perforations: some View {
        HStack(spacing: 2.5) {
            ForEach(0..<8, id: \.self) { _ in RoundedRectangle(cornerRadius: 0.8).fill(Color(white: 0.95)).frame(height: 3.5) }
        }
        .padding(.horizontal, 1.5).padding(.vertical, 1.5)
    }
}
