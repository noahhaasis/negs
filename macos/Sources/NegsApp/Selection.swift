import CoreGraphics
import Foundation

/// The edit view shows one photograph; the grid shows the folder.
enum LibraryMode: Sendable { case photo, grid }

/// The photographs selected in the film strip or the grid, and the current
/// one: the photograph the edit view shows. A plain click selects one
/// photograph and makes it current; modifier clicks change only the
/// selection, so ⌘-clicking through a strip of RAWs never develops each one.
/// The current photograph is always selected.
struct LibrarySelection: Equatable, Sendable {
    private(set) var current: URL?
    private(set) var selected: Set<URL> = []
    /// Where a range starts (the last plain or ⌘-click) and where it ends
    /// now (the last ⇧-click or ⇧-arrow).
    private(set) var anchor: URL?
    private(set) var lead: URL?
    /// The selection a range is added to: as it was when the anchor was set.
    private var base: Set<URL> = []
    /// Photographs a rating or flag just changed: they stay shown, even if
    /// the filter now leaves them out, until the selection changes. Rating a
    /// run of photographs never makes the selection jump.
    private(set) var kept: Set<URL> = []

    /// The photographs a filter must still show.
    var visible: Set<URL> { current.map { kept.union([$0]) } ?? kept }

    /// Plain click: only `photo`, and it's current.
    mutating func select(_ photo: URL) {
        current = photo; selected = [photo]; anchor = photo; lead = photo; base = [photo]; kept = []
    }

    /// `photos` alone, the first current: new duplicates.
    mutating func select(_ photos: [URL]) {
        guard let first = photos.first else { return }
        select(first)
        selected = Set(photos); base = selected
    }

    /// ⌘-click: adds or removes `photo`. The current photograph stays.
    mutating func toggle(_ photo: URL) {
        guard current != nil else { return select(photo) }
        guard photo != current else { return }
        if selected.contains(photo) { selected.remove(photo) } else { selected.insert(photo) }
        anchor = photo; lead = photo; base = selected; kept = []
    }

    /// ⇧-click or ⇧-arrow: the photographs from the anchor to `photo`, in
    /// `order`, added to the selection as it was when the anchor was set.
    mutating func extend(to photo: URL, in order: [URL]) {
        guard let current else { return select(photo) }
        let start = anchor.flatMap { order.firstIndex(of: $0) } ?? order.firstIndex(of: current)
        guard let start, let end = order.firstIndex(of: photo) else { return }
        selected = base.union(order[min(start, end)...max(start, end)]).union([current])
        lead = photo; kept = []
    }

    /// ⇧-arrow: moves the range's end `offset` places on in `order`.
    mutating func extend(by offset: Int, in order: [URL]) {
        guard let from = (lead ?? current).flatMap({ order.firstIndex(of: $0) }) else { return }
        let to = min(max(from + offset, 0), order.count - 1)
        extend(to: order[to], in: order)
    }

    /// ⌘A: everything shown.
    mutating func selectAll(_ shown: [URL]) {
        guard let current else { return }
        selected = Set(shown).union([current]); anchor = current; lead = current; base = selected; kept = []
    }

    /// ⌥⌘A: all but the current photograph.
    mutating func deselectAll() {
        guard let current else { return }
        select(current)
    }

    /// Rubber band: `base` and the photographs inside the band.
    mutating func band(_ photos: some Sequence<URL>, adding base: Set<URL>) {
        guard let current else { return }
        selected = base.union(photos).union([current]); lead = current; self.base = selected; kept = []
    }

    /// Keeps only the photographs in `shown`, such as after the filter changed.
    mutating func prune(to shown: Set<URL>) {
        let pruned = selected.filter { shown.contains($0) || $0 == current }
        if pruned != selected { selected = pruned; base = base.intersection(pruned) }
        kept = kept.intersection(shown)
        if let anchor, !selected.contains(anchor) { self.anchor = current }
        if let lead, !selected.contains(lead) { self.lead = current }
    }

    /// Keeps `photos` shown until the selection changes.
    mutating func keep(_ photos: some Sequence<URL>) { kept.formUnion(photos) }

    /// The selected photographs in `order`.
    func ordered(_ order: [URL]) -> [URL] { order.filter { selected.contains($0) } }
}

/// The sheet's layout (§4.6): thumbnails packed edge to edge in rows with
/// gaps of 6, each at its own proportions, every full row stretched or
/// shrunk a little to the sheet's width, and the last row left short at the
/// chosen height. Worked out rather than measured, so moving up or down a
/// row, the rubber band and the scroll position know where every cell is.
struct SheetLayout: Equatable {
    struct Row: Equatable {
        let range: Range<Int>
        let y: CGFloat
        let height: CGFloat
    }

    static let gap = Theme.Size.gridGap
    /// The grid's padding: 24 at the top and bottom, room for a box (round
    /// 20) on the first and last rows, whose ends run up to 24 pt past the
    /// picture; 16 at the sides.
    static let top: CGFloat = 24, side: CGFloat = 16, bottom: CGFloat = 24
    /// The chosen row height (the foot's Size), 176 by default.
    static let sizes: ClosedRange<Double> = 100...360
    static let defaultSize = Double(Theme.Size.gridThumbnail)

    let width: CGFloat
    let frames: [CGRect]
    let rows: [Row]
    /// The whole grid's height, padding included.
    let height: CGFloat

    /// `ratios`: each photograph's width ÷ height, in order.
    init(ratios: [CGFloat], width: CGFloat, target: CGFloat) {
        let usable = max(width - 2 * Self.side, 1)
        let target = max(target, 1)
        var frames: [CGRect] = [], rows: [Row] = []
        frames.reserveCapacity(ratios.count)
        var y = Self.top, start = 0
        func ratio(_ index: Int) -> CGFloat { min(max(ratios[index], 0.1), 10) }
        while start < ratios.count {
            var end = start, sum: CGFloat = 0, full = false
            while end < ratios.count {
                let next = sum + ratio(end)
                let gaps = Self.gap * CGFloat(end - start)
                guard next * target + gaps >= usable else { sum = next; end += 1; continue }
                full = true
                // Take this one too if that leaves the row nearer the
                // chosen height than stopping short of it does.
                if end == start || abs(log((usable - gaps) / next / target)) <= abs(log((usable - gaps + Self.gap) / sum / target)) {
                    sum = next; end += 1
                }
                break
            }
            let count = end - start
            let height = (full ? (usable - Self.gap * CGFloat(count - 1)) / sum : target).rounded()
            var x = Self.side
            for index in start..<end {
                // Whole points; a full row's last cell ends flush at the right.
                let cellWidth = full && index == end - 1 ? Self.side + usable - x : max(1, (ratio(index) * height).rounded())
                frames.append(CGRect(x: x, y: y, width: cellWidth, height: height))
                x += cellWidth + Self.gap
            }
            rows.append(Row(range: start..<end, y: y, height: height))
            y += height + Self.gap
            start = end
        }
        self.width = width
        self.frames = frames
        self.rows = rows
        height = rows.isEmpty ? Self.top + Self.bottom : y - Self.gap + Self.bottom
    }

    /// Whether a photograph is drawn as selected, boxed in yellow: the
    /// current one and each one selected.
    static func drawnSelected(_ url: URL, selection: LibrarySelection) -> Bool {
        url == selection.current || selection.selected.contains(url)
    }

    /// The row holding the `index`th photograph.
    func row(of index: Int) -> Int? {
        guard frames.indices.contains(index) else { return nil }
        var low = 0, high = rows.count - 1
        while low < high {
            let middle = (low + high + 1) / 2
            if rows[middle].range.lowerBound <= index { low = middle } else { high = middle - 1 }
        }
        return low
    }

    /// The photograph `by` rows up (−) or down (+) from `index`: the one
    /// nearest it across, as in Finder. Nil past the first or last row.
    func vertical(from index: Int, by: Int) -> Int? {
        guard let row = row(of: index), rows.indices.contains(row + by) else { return nil }
        let x = frames[index].midX
        return rows[row + by].range.min { abs(frames[$0].midX - x) < abs(frames[$1].midX - x) }
    }

    /// The cells `rect` touches.
    func indices(in rect: CGRect) -> [Int] {
        rows.filter { $0.y <= rect.maxY && $0.y + $0.height >= rect.minY }
            .flatMap { row in row.range.filter { frames[$0].intersects(rect) } }
    }
}
