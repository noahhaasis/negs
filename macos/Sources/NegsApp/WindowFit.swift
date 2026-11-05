import CoreGraphics

/// The window's content size when it's locked to the export size: the canvas
/// shows the export at `canvas`, and `chrome` is everything around it (toolbar,
/// zone panel, film strip, status bar, sidebar).
enum WindowFit {
    /// A smaller export still gets a usable canvas, on its shorter side.
    static let minimumCanvas: CGFloat = 240

    /// The white mat around the export: a framer's proportions, smaller because
    /// it's on a screen. 8 % of the long edge, with the bottom 1.2 × that so the
    /// photograph sits at the optical centre rather than seeming to sink.
    static func mat(forLongEdge long: CGFloat) -> (side: CGFloat, top: CGFloat, bottom: CGFloat) {
        let width = long * 0.08
        return (width, width, width * 1.2)
    }

    /// The export at its size, scaled down in the same shape to stay on `limit`.
    static func contentSize(canvas: CGSize, chrome: CGSize, limit: CGSize) -> CGSize {
        var (w, h) = (max(canvas.width, 1), max(canvas.height, 1))
        let short = min(w, h)
        if short < minimumCanvas { (w, h) = (w * minimumCanvas / short, h * minimumCanvas / short) }
        let mat = mat(forLongEdge: max(w, h))
        let (fullWidth, fullHeight) = (w + mat.side * 2, h + mat.top + mat.bottom)
        let scale = min(1, (limit.width - chrome.width) / fullWidth, (limit.height - chrome.height) / fullHeight)
        return CGSize(width: fullWidth * scale + chrome.width, height: fullHeight * scale + chrome.height)
    }
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
