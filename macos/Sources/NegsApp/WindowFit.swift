import CoreGraphics

/// The window's content size when it's locked to the export size: the canvas
/// shows the export at `canvas`, and `chrome` is everything around it (toolbar,
/// zone panel, film strip, status bar, sidebar).
enum WindowFit {
    /// A smaller export still gets a usable canvas, on its shorter side.
    static let minimumCanvas: CGFloat = 240
    /// The white mat around the export, on each side.
    static let padding: CGFloat = 32

    /// The export at its size, scaled down in the same shape to stay on `limit`.
    static func contentSize(canvas: CGSize, chrome: CGSize, limit: CGSize) -> CGSize {
        var (w, h) = (max(canvas.width, 1), max(canvas.height, 1))
        let short = min(w, h)
        if short < minimumCanvas { (w, h) = (w * minimumCanvas / short, h * minimumCanvas / short) }
        let mat = padding * 2
        let scale = min(1, (limit.width - chrome.width - mat) / w, (limit.height - chrome.height - mat) / h)
        return CGSize(width: w * scale + mat + chrome.width, height: h * scale + mat + chrome.height)
    }
}

/// The pixels an export would write, in the exporter's order: crop, turn, border, fit.
enum ExportPixels {
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
