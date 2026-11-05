import CoreGraphics

/// How a proof sits in the window.
enum Proof {
    /// How far above centre the photograph sits, so it doesn't seem to sink: what
    /// a framer's mat does, with 1.2 × the width below at 8 % of the long edge.
    static func raise(forLongEdge long: CGFloat) -> CGFloat { long * 0.008 }
}

/// The pixels an export would write, in the exporter's order: crop, turn, border, fit.
enum ExportPixels {
    /// The photograph's own pixels inside an export: turned, without its border,
    /// at the scale the export's limit gives the framed file.
    static func photograph(_ photograph: CGSize, rotated: Bool, withBorder: ((CGSize) -> CGSize)?, fit: ExportSize) -> CGSize {
        let turned = rotated ? CGSize(width: photograph.height, height: photograph.width) : photograph
        let framed = size(photograph: photograph, rotated: rotated, withBorder: withBorder, fit: .full)
        let limited = size(photograph: photograph, rotated: rotated, withBorder: withBorder, fit: fit)
        let scale = framed.width > 0 ? limited.width / framed.width : 1
        return CGSize(width: (turned.width * scale).rounded(), height: (turned.height * scale).rounded())
    }

    static func size(photograph: CGSize, rotated: Bool, withBorder: ((CGSize) -> CGSize)?, fit: ExportSize) -> CGSize {
        let turned = rotated ? CGSize(width: photograph.height, height: photograph.width) : photograph
        let framed = withBorder?(turned) ?? turned
        let edge: CGFloat
        switch fit.edge {
        case .full: edge = 0
        case .long: edge = max(framed.width, framed.height)
        case .short: edge = min(framed.width, framed.height)
        }
        // A limit only ever reduces, as the exporter's does.
        let scale = edge > 0 ? min(1, CGFloat(fit.pixels) / edge) : 1
        return CGSize(width: (framed.width * scale).rounded(), height: (framed.height * scale).rounded())
    }
}
