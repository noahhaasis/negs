import AppKit
import SwiftUI

/// Where a SwiftUI scroll view is scrolled to, read from the AppKit scroll
/// view under it, for the strip and the grid: they build only the frames in
/// or near view and place each where their own layout puts it. Lazy stacks
/// placed the frames they built at their own estimates, inside a content
/// sized from the exact layout, so far from the start the frames were drawn
/// thousands of points from where the layout (the scroll target, the rubber
/// band, the flights) expected them, and the last ones couldn't be reached.
/// The offset is published in steps of `step`, so a scroll redraws its view
/// only now and then.
@MainActor @Observable final class ScrollWatch {
    static let step: CGFloat = 100

    /// The visible content's top left corner, rounded down to `step`.
    private(set) var origin: CGPoint = .zero
    @ObservationIgnored private(set) weak var scrollView: NSScrollView?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    /// Asked for before the scroll view was found, or before the content
    /// was laid out big enough to scroll that far.
    @ObservationIgnored private var pending: CGPoint?

    deinit { observers.forEach(NotificationCenter.default.removeObserver) }

    /// The visible content's rect, exactly; nil until the scroll view is found.
    var visible: CGRect? { scrollView?.contentView.bounds }

    func attach(_ scrollView: NSScrollView?) {
        guard scrollView !== self.scrollView else { return }
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        self.scrollView = scrollView
        guard let scrollView else { return }
        let clip = scrollView.contentView
        clip.postsBoundsChangedNotifications = true
        observers.append(NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: clip, queue: nil) { [weak self] _ in
            MainActor.assumeIsolated { self?.read() }
        })
        // The content growing lets a scroll asked for too soon through.
        clip.postsFrameChangedNotifications = true
        scrollView.documentView?.postsFrameChangedNotifications = true
        for view in [clip, scrollView.documentView].compactMap({ $0 }) {
            observers.append(NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: view, queue: nil) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, let pending = self.pending else { return }
                    self.pending = nil
                    self.scroll(to: pending)
                }
            })
        }
        if let pending {
            self.pending = nil
            scroll(to: pending)
        } else {
            read()
        }
    }

    private func read() {
        guard let clip = scrollView?.contentView else { return }
        publish(clip.bounds.origin)
    }

    private func publish(_ point: CGPoint) {
        let step = Self.step
        let rounded = CGPoint(x: (point.x / step).rounded(.down) * step, y: (point.y / step).rounded(.down) * step)
        if rounded != origin { origin = rounded }
    }

    /// Scrolls so `point` of the content is at the top left (the caller
    /// keeps it inside the content), at once; before the scroll view is
    /// found, once it is. What shows there is built straight away.
    func scroll(to point: CGPoint) {
        publish(point)
        pending = nil
        guard let scrollView else { pending = point; return }
        let clip = scrollView.contentView
        // Too far for the content as it's laid out now: once it has grown.
        if let document = scrollView.documentView?.frame.size,
           point.x + clip.bounds.width > document.width + 0.5 || point.y + clip.bounds.height > document.height + 0.5 {
            pending = point
        }
        guard clip.bounds.origin != point else { return }
        clip.setBoundsOrigin(point)
        scrollView.reflectScrolledClipView(clip)
    }

    /// Scrolls across only, or up and down only.
    func scroll(x: CGFloat) { scroll(to: CGPoint(x: x, y: visible?.minY ?? pending?.y ?? 0)) }
    func scroll(y: CGFloat) { scroll(to: CGPoint(x: visible?.minX ?? pending?.x ?? 0, y: y)) }
}

/// The film strip's frames laid out: each one's left edge and width, edge to
/// edge but for the gaps. Worked out, never measured, like `SheetLayout`.
struct StripLayout: Equatable {
    let xs: [CGFloat]
    let widths: [CGFloat]
    /// The whole row's width.
    let width: CGFloat

    init(widths: [CGFloat]) {
        var xs: [CGFloat] = [], x: CGFloat = 0
        xs.reserveCapacity(widths.count)
        for width in widths {
            xs.append(x)
            x += width + Theme.Size.stripGap
        }
        self.xs = xs
        self.widths = widths
        width = PhotoBrowser.rowWidth(widths)
    }

    var count: Int { xs.count }

    func middle(of index: Int) -> CGFloat { xs[index] + widths[index] / 2 }

    /// The frames reaching into `minX...maxX`.
    func indices(from minX: CGFloat, to maxX: CGFloat) -> Range<Int> {
        // The first frame ending at or after minX, then the first starting after maxX.
        let start = partition { xs[$0] + widths[$0] >= minX }
        let end = partition { xs[$0] > maxX }
        return start..<max(start, end)
    }

    /// The frame at `x`, or the nearest one; nil without frames.
    func index(at x: CGFloat) -> Int? {
        guard count > 0 else { return nil }
        return min(partition { xs[$0] + widths[$0] >= x }, count - 1)
    }

    /// The first index for which `test` holds, `count` if none; `test` must
    /// be false and then true along the row.
    private func partition(_ test: (Int) -> Bool) -> Int {
        var low = 0, high = count
        while low < high {
            let middle = (low + high) / 2
            if test(middle) { high = middle } else { low = middle + 1 }
        }
        return low
    }
}

extension SheetLayout {
    /// The photographs in the rows reaching into `minY...maxY`.
    func indices(from minY: CGFloat, to maxY: CGFloat) -> Range<Int> {
        func partition(_ test: (Row) -> Bool) -> Int {
            var low = 0, high = rows.count
            while low < high {
                let middle = (low + high) / 2
                if test(rows[middle]) { high = middle } else { low = middle + 1 }
            }
            return low
        }
        let first = partition { $0.y + $0.height >= minY }
        let last = partition { $0.y > maxY }
        guard first < last else { return 0..<0 }
        return rows[first].range.lowerBound..<rows[last - 1].range.upperBound
    }
}
