import AppKit
import CoreText
import SwiftUI

/// The label under the photograph, like the line under a print: the film's
/// name, "printed on", and the paper's name. Each name opens its stocks
/// (IMPLEMENTATION.md §4.2).
///
/// One row when both names fit under the photograph; two when they don't,
/// with the paper's row free to run past the photograph's right edge up to
/// the canvas's margin, where it's cut with an ellipsis.
struct PrintLabel: View {
    let model: EditorModel
    /// The widest the photograph allows (one row) and the canvas allows (two).
    let photoWidth: CGFloat
    let canvasWidth: CGFloat
    /// Compare: the film or paper differs from the other photograph's.
    var markFilm = false
    var markPaper = false
    @Environment(\.sheetInk) private var ink

    var body: some View {
        let scanning = model.value("io.scan_film").bool
        let original = model.original != nil
        let kind = PrintLabelLayout.kind(scanning: scanning)
        let negative = model.isNegative
        // A negative always takes two rows: "Your negative, read as <film>", then the paper.
        let oneRow = !negative && PrintLabelLayout.fitsOneRow(film: model.filmName, paper: model.paperName, kind: kind, width: photoWidth)
        let film = HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.inner) {
            StockName(model: model, paper: false, name: negative ? NegativeScan.name : model.filmName, size: 24, weight: 600,
                      strike: original ? .original : nil, opens: !original, marked: markFilm && !negative)
            if negative {
                Text("read as").font(Theme.serif(14, italic: true)).foregroundStyle(ink.dim).lineLimit(1).fixedSize()
                StockName(model: model, paper: false, name: model.filmName, size: 17.5, weight: 400,
                          strike: original ? .original : nil, opens: !original, readAs: true, marked: markFilm)
            }
        }
        let paper = HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.inner) {
            Text(kind).font(Theme.serif(14, italic: true)).foregroundStyle(ink.dim).lineLimit(1).fixedSize()
            StockName(model: model, paper: true, name: model.paperName, size: 17.5, weight: 400,
                      strike: original ? .original : scanning ? .scanning : nil, opens: !original && !scanning, marked: markPaper)
            if original {
                Text("the original").font(Theme.serif(16, italic: true)).foregroundStyle(ink.paper).lineLimit(1).fixedSize()
                    .padding(.leading, 6 - PrintLabelLayout.inner)
            }
        }
        Group {
            if oneRow {
                HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.gap) { film; paper }
                    .padding(.top, 11)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    film
                    // The canvas is waiting for a click (Prep's pickers): the
                    // prompt takes the paper's row, inside the label's band
                    // (below it, the film strip covers it).
                    if let pick = model.negativePick {
                        Text(pick.prompt).font(Theme.serif(14, italic: true)).foregroundStyle(ink.accent)
                            .lineLimit(1).fixedSize()
                            .padding(.top, 3)
                    } else {
                        paper
                    }
                }
                .padding(.top, 8)
            }
        }
        .frame(width: oneRow ? photoWidth : canvasWidth, alignment: .topLeading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Film and paper")
    }
}

/// The label's sizes and its one-or-two-rows rule, measured with the same
/// fonts the label draws with.
@MainActor enum PrintLabelLayout {
    /// Between the film and the paper on one row, and inside a name.
    static let gap: CGFloat = 9, inner: CGFloat = 7
    /// The space under the photograph: always two rows (§2.3).
    static let band: CGFloat = 68

    static func kind(scanning: Bool) -> String { scanning ? "not printed — scanning the film" : "printed on" }

    static func width(_ text: String, font: CTFont, tracking: CGFloat = 0) -> CGFloat {
        var attributes: [NSAttributedString.Key: Any] = [.font: font]
        if tracking != 0 { attributes[.kern] = tracking }
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attributes))
        return ceil(CTLineGetTypographicBounds(line, nil, nil, nil))
    }
    static var chevron: CGFloat { width("⌄", font: NSFont.systemFont(ofSize: 13)) + 1 }
    /// The film's name at 24 pt, 600, with its `⌄`.
    static func filmWidth(_ name: String) -> CGFloat {
        width(name, font: Theme.serifFont(24, weight: 600), tracking: -0.24) + inner + chevron
    }
    /// "printed on", the paper's name at 17.5 pt and its `⌄`.
    static func paperWidth(_ name: String, kind: String) -> CGFloat {
        width(kind, font: Theme.serifFont(14, italic: true)) + inner + width(name, font: Theme.serifFont(17.5)) + inner + chevron
    }
    static func fitsOneRow(film: String, paper: String, kind: String, width: CGFloat) -> Bool {
        filmWidth(film) + gap + paperWidth(paper, kind: kind) <= width
    }
}

/// A stock's name: a button that opens its tray. Pointing at it draws a
/// faint `track` rule under it; while its tray is open, the pencil.
struct StockName: View {
    enum Strike {
        /// Scanning the film: the paper is struck through at about half its height.
        case scanning
        /// Holding Original: both names are struck, higher up.
        case original
        var height: CGFloat { self == .scanning ? 0.54 : 0.34 }
        var width: CGFloat { self == .scanning ? 0.88 : 1 }
    }

    let model: EditorModel
    let paper: Bool
    let name: String
    let size: CGFloat
    let weight: CGFloat
    var strike: Strike?
    /// False while scanning (the paper) or holding Original: the name is only words.
    var opens = true
    /// The film a negative is read as: opens the colour negatives.
    var readAs = false
    /// Compare: it differs from the other photograph's, so the pencil is under it.
    var marked = false
    @State private var hovered = false
    @State private var anchor = MenuAnchor()
    @Environment(\.sheetInk) private var ink

    /// Its tray is open.
    private var open: Bool { model.tray.map { $0.paper == paper && $0.readAs == readAs } ?? false }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.inner + 1) {
            Text(name)
                .font(Theme.serif(size, weight: weight))
                .tracking(paper ? 0 : -0.24)
                .foregroundStyle(strike == nil ? ink.paper : ink.dim)
                .lineLimit(1).truncationMode(.tail)
                .padding(.bottom, 5)
                .overlay(alignment: .bottom) { underline }
                .overlay { struck }
                .background(AnchorView(anchor: anchor))
            if opens {
                Text("⌄").font(.system(size: 13)).foregroundStyle(ink.dim).fixedSize()
            }
        }
        .contentShape(Rectangle())
        // A click here opens or closes its tray, so it isn't a click outside the tray.
        .background(TrayHitArea(model: model))
        .onHover { hovered = $0 && opens }
        .onTapGesture { if opens { activate() } }
        .help(opens ? (paper ? ControlHelp.paper : readAs ? "The film this negative is read as: its dyes undo the scan. Unknown stock? Portra 400 is a fair guess." : ControlHelp.film) : name)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(paper ? "Paper" : readAs ? "Read as" : "Film"): \(name)")
        .accessibilityValue(strike == .scanning ? "Not printed, scanning the film" : strike == .original ? "Showing the original" : "")
        .accessibilityAddTraits(opens ? .isButton : [])
        .accessibilityAction { if opens { activate() } }
    }

    @ViewBuilder private var underline: some View {
        if strike == nil {
            if open {
                PencilStroke().pencil(ink.highlighter ? nil : ink.accent, highlighter: ink.highlighter).frame(height: 6).allowsHitTesting(false)
            } else if hovered {
                Rectangle().fill(ink.track).frame(height: 1.5).offset(y: -1).allowsHitTesting(false)
            } else if marked {
                PencilStroke().pencil(ink.highlighter ? nil : ink.accent, highlighter: ink.highlighter).frame(height: 5).allowsHitTesting(false)
            }
        }
    }

    @ViewBuilder private var struck: some View {
        if let strike {
            // As CSS places it: `left 54% / 88% 6px` over the name and its 5 pt below.
            GeometryReader { proxy in
                let width = proxy.size.width * strike.width
                PencilStroke().strike(ink.accent)
                    .frame(width: width, height: 6)
                    .position(x: width / 2, y: (proxy.size.height - 6) * strike.height + 3)
            }
            .allowsHitTesting(false)
        }
    }

    private func activate() {
        // Settings can turn the preview tiles off: then the plain menu.
        if model.previews.enabled {
            model.toggleTray(paper: paper, readAs: readAs)
        } else {
            let keep: (Stock) -> Bool = { !readAs || NegativeScan.readable($0) }
            let stocks = (paper ? model.papers : model.listedFilms).filter(keep)
            let groups = (paper ? model.paperGroups : model.filmGroups).compactMap { group -> StockGroup? in
                let stocks = group.stocks.filter(keep)
                return stocks.isEmpty ? nil : StockGroup(title: group.title, stocks: stocks)
            }
            let favorites = model.previews.favorites(in: stocks, paper: paper)
            var items: [ChoiceItem] = []
            // Your negative first, as in the tray.
            if !paper, !readAs {
                items.append(.header("Scan"))
                items.append(.option(NegativeScan.name, tag: NegativeScan.stockID))
            }
            if !favorites.isEmpty {
                items.append(.header("Favourites"))
                items += favorites.map { .option($0.name, tag: StockMenu.favoriteID($0.id)) }
            }
            for group in groups {
                items.append(.header(group.title))
                items += group.stocks.map { .option($0.name, tag: $0.id) }
            }
            let selected = paper ? model.paper : model.isNegative && !readAs ? NegativeScan.stockID : model.film
            popUpChoiceMenu(items, selected: selected, under: anchor.view) { tag in
                let id = StockMenu.stockID(tag: tag)
                if id == NegativeScan.stockID { model.chooseNegative() }
                else if paper { model.choosePaper(id) } else if readAs { model.chooseNegativeFilm(id) } else { model.chooseFilm(id) }
            }
        }
    }
}
