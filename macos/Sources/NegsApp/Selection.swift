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

/// The grid's layout, worked out rather than measured, so moving up or down
/// a row and the rubber band know where every cell is.
struct GridMetrics: Equatable {
    /// The photograph's box; the name sits under it.
    let cell: CGFloat
    let columns: Int
    let inset: CGFloat
    static let spacing: CGFloat = 14
    static let padding: CGFloat = 20
    static let nameHeight: CGFloat = 20
    /// The film strip's thumbnail proportions.
    static let aspect: CGFloat = 68.0 / 96.0
    static let sizes: ClosedRange<Double> = 100...360
    static let defaultSize: Double = 180

    init(width: CGFloat, cell: CGFloat) {
        self.cell = cell
        let usable = max(width - 2 * Self.padding, cell)
        columns = max(1, Int((usable + Self.spacing) / (cell + Self.spacing)))
        let row = CGFloat(columns) * cell + CGFloat(columns - 1) * Self.spacing
        inset = max(Self.padding, (width - row) / 2)
    }

    var imageHeight: CGFloat { (cell * Self.aspect).rounded() }
    var rowHeight: CGFloat { imageHeight + Self.nameHeight }

    func rows(_ count: Int) -> Int { (count + columns - 1) / columns }
    func height(_ count: Int) -> CGFloat {
        let rows = rows(count)
        return 2 * Self.padding + CGFloat(rows) * rowHeight + CGFloat(max(rows - 1, 0)) * Self.spacing
    }

    /// Where the `index`th cell sits, name included.
    func frame(_ index: Int) -> CGRect {
        let row = index / columns, column = index % columns
        return CGRect(x: inset + CGFloat(column) * (cell + Self.spacing),
                      y: Self.padding + CGFloat(row) * (rowHeight + Self.spacing), width: cell, height: rowHeight)
    }

    /// The cells `rect` touches, of `count`.
    func indices(in rect: CGRect, count: Int) -> [Int] {
        guard count > 0, rect.maxY >= Self.padding else { return [] }
        let stride = rowHeight + Self.spacing
        let first = max(0, Int((rect.minY - Self.padding) / stride))
        let last = min(rows(count) - 1, Int((rect.maxY - Self.padding) / stride))
        guard first <= last else { return [] }
        var hit: [Int] = []
        for row in first...last {
            for column in 0..<columns {
                let index = row * columns + column
                if index < count, frame(index).intersects(rect) { hit.append(index) }
            }
        }
        return hit
    }
}
