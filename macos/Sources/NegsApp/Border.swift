import ImageIO
import QuartzCore
import SwiftUI

/// Presentation borders. They are drawn around the developed photograph, so
/// changing one never re-renders. Sizes match `spektrafilm-io::border`, which
/// adds the same border on export; keep the two in sync.
enum BorderStyle: String, Codable, Sendable, CaseIterable, Identifiable {
    /// Saved as "white", its name before it took a colour.
    case solid = "white"
    case film, keyline
    case film35 = "35mm"

    var id: String { rawValue }
    var title: String {
        switch self {
        case .solid: "Solid"
        case .film: "Film"
        case .keyline: "Keyline"
        case .film35: "35mm"
        }
    }
    var help: String {
        switch self {
        case .solid: "A plain mat around the photograph."
        case .film: "A scanned 120 film frame with an irregular black edge."
        case .keyline: "A mat with a thin line around the photograph."
        case .film35: "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph."
        }
    }

    /// Whether the style fits a photograph of `size`: the 35mm strip only
    /// holds 3:2 and 3:4 photographs.
    func fits(_ size: CGSize) -> Bool { self != .film35 || FilmFormat(size) != nil }

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

/// An 8-bit sRGB colour, saved and sent to the exporter as "#rrggbb", so
/// the preview shows exactly what the exporter fills.
struct BorderColor: Codable, Hashable, Sendable {
    var red: UInt8, green: UInt8, blue: UInt8

    static let white = BorderColor(red: 255, green: 255, blue: 255)
    static let black = BorderColor(red: 0, green: 0, blue: 0)

    init(red: UInt8, green: UInt8, blue: UInt8) { (self.red, self.green, self.blue) = (red, green, blue) }
    /// The nearest sRGB colour to `color`, from any colour space.
    init(_ color: CGColor) {
        let srgb = color.converted(to: CGColorSpace(name: CGColorSpace.sRGB)!, intent: .defaultIntent, options: nil)
        let c = srgb?.components ?? [0, 0, 0]
        let channel = { (i: Int) in UInt8((min(max(c.count > 2 ? c[i] : c[0], 0), 1) * 255).rounded()) }
        self.init(red: channel(0), green: channel(1), blue: channel(2))
    }
    init?(hex: String) {
        let digits = hex.hasPrefix("#") ? hex.dropFirst() : Substring(hex)
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        self.init(red: UInt8(value >> 16 & 0xff), green: UInt8(value >> 8 & 0xff), blue: UInt8(value & 0xff))
    }

    var hex: String { String(format: "#%02x%02x%02x", red, green, blue) }
    var cgColor: CGColor { CGColor(srgbRed: CGFloat(red) / 255, green: CGFloat(green) / 255, blue: CGFloat(blue) / 255, alpha: 1) }

    init(from decoder: Decoder) throws {
        let hex = try decoder.singleValueContainer().decode(String.self)
        guard let color = BorderColor(hex: hex) else {
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Not a #rrggbb colour: \(hex)"))
        }
        self = color
    }
    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(hex)
    }
}

struct BorderSettings: Codable, Equatable, Sendable {
    var enabled = false
    var style = BorderStyle.solid
    var export = true
    /// Extends the border to Instagram's 3:4 portrait: with the mat's
    /// colour around a mat, with white around film.
    var instagram = false
    /// The solid and keyline mat on every side, a fraction of the
    /// photograph's long edge. Both styles share it, and the colour.
    var width = Self.defaultWidth
    /// Black around the film frame, a fraction of the long edge.
    var filmWidth = 0.0
    /// The keyline's line, a fraction of the long edge.
    var line = Self.defaultLine
    var color = BorderColor.white
    var lineColor = BorderColor.black

    static let defaultWidth = 0.03
    static let defaultLine = 0.004

    /// The border around a photograph of `size`, if the style fits it.
    func shown(for size: CGSize) -> BorderSettings? { enabled && style.fits(size) ? self : nil }
    func exported(for size: CGSize) -> BorderSettings? { export ? shown(for: size) : nil }

    /// The style's width setting: the mat, or the black around film.
    var styleWidth: Double? {
        switch style {
        case .solid, .keyline: width
        case .film: filmWidth
        case .film35: nil
        }
    }

    /// The canvas around a photograph of `size` pixels. The 35mm canvas
    /// comes from its `strip`, and is the photograph alone until that loads.
    func canvas(for size: CGSize, strip: FilmStrip? = nil) -> CGSize {
        let long = max(size.width, size.height)
        let inset: CGFloat
        switch style {
        case .solid: inset = (long * width).rounded(.down)
        case .keyline: inset = (long * width).rounded(.down) + keyline(for: size)
        case .film: inset = BorderStyle.film(for: size).inset + (long * filmWidth).rounded(.down)
        case .film35:
            guard let strip, strip.format == FilmFormat(size) else { return size }
            return strip.canvas(for: size)
        }
        return CGSize(width: size.width + 2 * inset, height: size.height + 2 * inset)
    }
    /// The 3:4 portrait canvas Instagram shows whole, around the bordered
    /// `canvas`: the film borders get a white margin first, then the
    /// background extends the short way as little as it can.
    func instagramCanvas(for canvas: CGSize) -> CGSize {
        let margin = style == .film || style == .film35 ? (max(canvas.width, canvas.height) * 0.03).rounded(.down) : 0
        let (w, h) = (canvas.width + 2 * margin, canvas.height + 2 * margin)
        return w * 4 > h * 3 ? CGSize(width: w, height: (w * 4 / 3).rounded(.up)) : CGSize(width: (h * 3 / 4).rounded(.up), height: h)
    }
    func keyline(for size: CGSize) -> CGFloat { max(1, (max(size.width, size.height) * line).rounded()) }

    /// The exporter's options for this border, as `BorderOptions` in
    /// `spektrafilm-io::border` reads them.
    var options: [String: JSONValue] {
        var options: [String: JSONValue] = ["line": .number(line), "color": .string(color.hex), "line_color": .string(lineColor.hex)]
        if let styleWidth { options["width"] = .number(styleWidth) }
        return options
    }
    /// The same options as `spektrafilm process` arguments.
    var arguments: [String] {
        var arguments = ["--border", style.rawValue, "--keyline-width", "\(line)", "--border-color", color.hex, "--keyline-color", lineColor.hex]
        if let styleWidth { arguments += ["--border-width", "\(styleWidth)"] }
        if instagram { arguments.append("--instagram") }
        return arguments
    }
}

extension BorderSettings {
    /// Sessions saved before a setting existed restore it at its default.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let defaults = BorderSettings()
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? defaults.enabled
        style = try c.decodeIfPresent(BorderStyle.self, forKey: .style) ?? defaults.style
        export = try c.decodeIfPresent(Bool.self, forKey: .export) ?? defaults.export
        instagram = try c.decodeIfPresent(Bool.self, forKey: .instagram) ?? defaults.instagram
        width = try c.decodeIfPresent(Double.self, forKey: .width) ?? defaults.width
        filmWidth = try c.decodeIfPresent(Double.self, forKey: .filmWidth) ?? defaults.filmWidth
        line = try c.decodeIfPresent(Double.self, forKey: .line) ?? defaults.line
        color = try c.decodeIfPresent(BorderColor.self, forKey: .color) ?? defaults.color
        lineColor = try c.decodeIfPresent(BorderColor.self, forKey: .lineColor) ?? defaults.lineColor
    }
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
    /// The keyline, or the black under a film border on a white canvas.
    private let line = CALayer()
    private let film = CALayer()
    private let strip = CALayer()

    init(in host: CALayer, around photo: CALayer) {
        let none = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull(), "backgroundColor": NSNull(), "hidden": NSNull()]
        line.actions = none
        for layer in [film, strip] {
            layer.actions = none
            layer.minificationFilter = .trilinear
            layer.magnificationFilter = .linear
            host.addSublayer(layer)
        }
        host.insertSublayer(line, below: photo)
        host.actions = none
    }

    /// Lays the border out around a photograph of `size` (as displayed,
    /// after rotation), centred in `host`. The mat fills the whole canvas,
    /// Instagram's extension too; there the film borders sit on white,
    /// like the exporter's.
    func layout(_ border: BorderSettings?, photo size: CGSize, host: CALayer, texture: CGImage?, filmStrip: FilmStrip?) {
        let center = CGPoint(x: host.bounds.midX, y: host.bounds.midY)
        let style = border?.style
        let filmStrip = style == .film35 ? filmStrip.flatMap { $0.format == FilmFormat(size) ? $0 : nil } : nil
        host.backgroundColor = switch style {
        case .solid?, .keyline?: border?.color.cgColor
        case .film?, .film35?: border?.instagram == true ? .white : .black
        case nil: nil
        }
        line.isHidden = style == nil || style == .solid
        if let border, border.style != .solid {
            let width = border.keyline(for: size)
            line.backgroundColor = border.style == .keyline ? border.lineColor.cgColor : .black
            line.bounds.size = border.style == .keyline ? CGSize(width: size.width + 2 * width, height: size.height + 2 * width)
                                                        : border.canvas(for: size, strip: filmStrip)
            line.position = center
        }
        layoutStrip(filmStrip, photo: size, center: center)
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
    /// The mat and line colours the samples show.
    var colors = (mat: BorderColor.white, line: BorderColor.black)

    var body: some View {
        HStack(spacing: 8) {
            ForEach(BorderStyle.allCases) { style in
                Button { selection = style } label: {
                    VStack(spacing: 5) {
                        BorderSwatch(style: style, colors: colors)
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
    let colors: (mat: BorderColor, line: BorderColor)
    @Environment(\.isEnabled) private var enabled
    private let photo = LinearGradient(colors: [Color(red: 0.55, green: 0.68, blue: 0.78), Color(red: 0.86, green: 0.74, blue: 0.56)], startPoint: .top, endPoint: .bottom)

    var body: some View {
        ZStack {
            switch style {
            case .solid:
                Color(cgColor: colors.mat.cgColor)
                Rectangle().fill(photo).padding(5)
            case .keyline:
                Color(cgColor: colors.mat.cgColor)
                Rectangle().fill(photo).padding(1).background(Color(cgColor: colors.line.cgColor)).padding(4)
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
