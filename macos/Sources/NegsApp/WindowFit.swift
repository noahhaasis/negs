import CoreGraphics

/// The window's content size when it's locked to the photograph: the canvas
/// takes the shape of the photograph with its border, and `chrome` is everything
/// around the canvas (toolbar, zone panel, film strip, status bar, sidebar).
enum WindowFit {
    /// The canvas's shorter side never goes below this while it fits on screen.
    static let minimumCanvas: CGFloat = 240

    /// While dragging the window's edge: the axis that moved most sets the size.
    static func resized(document: CGSize, chrome: CGSize, proposed: CGSize, current: CGSize, limit: CGSize) -> CGSize {
        let aspect = document.width / document.height
        let byHeight = abs(proposed.height - current.height) > abs(proposed.width - current.width)
        let width = byHeight ? (proposed.height - chrome.height) * aspect : proposed.width - chrome.width
        return bounded(width: width, aspect: aspect, chrome: chrome, limit: limit)
    }

    /// When the photograph's shape changes: the same canvas area in the new shape,
    /// so turning a photograph neither collapses the window nor grows it.
    static func refitted(document: CGSize, chrome: CGSize, current: CGSize, limit: CGSize) -> CGSize {
        let aspect = document.width / document.height
        let area = max(current.width - chrome.width, 1) * max(current.height - chrome.height, 1)
        return bounded(width: (area * aspect).squareRoot(), aspect: aspect, chrome: chrome, limit: limit)
    }

    private static func bounded(width: CGFloat, aspect: CGFloat, chrome: CGSize, limit: CGSize) -> CGSize {
        var (w, h) = (max(width, 1), max(width, 1) / aspect)
        let short = min(w, h)
        if short < minimumCanvas { (w, h) = (w * minimumCanvas / short, h * minimumCanvas / short) }
        let scale = min(1, (limit.width - chrome.width) / w, (limit.height - chrome.height) / h)
        return CGSize(width: w * scale + chrome.width, height: h * scale + chrome.height)
    }
}
