import AppKit
import SwiftUI

/// What one compared side draws, and the print's proportions it lays out by.
struct ComparePicture {
    var image: AnyObject
    var size: CGSize
    var rotation = 0
    /// Mirrored left to right after the crop, before `rotation` (`io.flip`).
    var flip = false
    var crop = CropSettings()
    var border: BorderSettings?
    var filmStrip: FilmStrip?
    var settled = true

    var document: CGSize { PreviewCanvas.documentSize(size, rotation: rotation, crop: crop, border: border, filmStrip: filmStrip) }
    var aspect: CGFloat { document.height > 0 && document.width > 0 ? document.width / document.height : 1.5 }
}

extension EditorModel {
    /// The picture for `side`: the open photograph as the photo view shows
    /// it, the other's stand-in or develop, or the original framed as the
    /// developed photograph. Nil while there's nothing to show.
    func comparePicture(_ side: Comparison.Side) -> ComparePicture? {
        if comparedIsOpen(side) { return openPicture }
        guard let other = compareSides[side], let image = other.image else { return nil }
        if side.isOriginal {
            // Cropped, turned and bordered as the developed photograph, as Original shows it.
            return ComparePicture(image: image, size: imageSize, rotation: imageRotation, flip: imageFlip, crop: crop,
                                  border: border.shown(for: crop.size(of: imageSize)), filmStrip: filmStrip)
        }
        if other.finished { return ComparePicture(image: image, size: other.size) }
        let state = other.state ?? EditState()
        return ComparePicture(image: image, size: other.size, rotation: Int(state.params["io.rotate_cw"].double),
                              flip: state.params["io.flip"].bool, crop: state.crop,
                              border: state.border.shown(for: state.crop.size(of: other.size)), filmStrip: other.filmStrip,
                              settled: other.shown == .developed)
    }

    /// The open photograph, as `CanvasArea.photograph` draws it.
    var openPicture: ComparePicture? {
        if original == nil, let trial = trialImage {
            let size = CanvasArea.standInSize(trial, photo: photoSize)
            return ComparePicture(image: trial, size: size, border: border.shown(for: size), filmStrip: filmStrip)
        } else if let image {
            return ComparePicture(image: original ?? image, size: imageSize, rotation: imageRotation, flip: imageFlip, crop: crop,
                                  border: border.shown(for: crop.size(of: imageSize)), filmStrip: filmStrip,
                                  settled: original != nil || showsSettledFrame)
        } else if let standIn {
            return ComparePicture(image: standIn, size: CanvasArea.standInSize(standIn, photo: placeholderSize))
        } else if let placeholder {
            return ComparePicture(image: placeholder, size: placeholderSize, rotation: imageRotation, flip: imageFlip, crop: crop,
                                  border: border.shown(for: crop.size(of: placeholderSize)), filmStrip: filmStrip)
        }
        return nil
    }
}

/// The compare view's canvas (ticket 46): the two photographs side by side,
/// in a row or stacked, or in one frame under a slider. Each keeps its
/// caption and label; what differs between them is underlined in pencil.
struct CompareCanvas: View {
    let model: EditorModel
    @AppStorage(ViewerSheet.key) private var sheet = ViewerSheet.theme
    @State private var link = ZoomLink()

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .topLeading) {
                sheet.color
                if let comparison = model.comparison {
                    switch comparison.layout {
                    case .sideBySide: sideBySide(comparison, canvas: proxy.size)
                    case .slider: CompareSlider(model: model, comparison: comparison, canvas: proxy.size, link: link)
                    }
                }
            }
        }
        .environment(\.sheetInk, sheet.ink)
        .clipped()
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Comparing two photographs")
    }

    @ViewBuilder private func sideBySide(_ comparison: Comparison, canvas: CGSize) -> some View {
        let a = model.comparePicture(comparison.first), b = model.comparePicture(comparison.second)
        let aspects = (a?.aspect ?? b?.aspect ?? 1.5, b?.aspect ?? a?.aspect ?? 1.5)
        let layout = CompareArrangement.layout(canvas: canvas, aspects: aspects)
        let stacked = layout.arrangement == .column
        let differences = model.compareDifferences
        ForEach(0..<2, id: \.self) { index in
            let first = index == 0
            ComparePane(model: model, side: first ? comparison.first : comparison.second, first: first,
                        active: comparison.activeFirst == first, picture: first ? a : b, frame: layout.frames[index],
                        stacked: stacked, differences: differences, link: link)
        }
        .onAppear { model.compareArrangement = layout.arrangement }
        .onChange(of: layout.arrangement) { _, arrangement in model.compareArrangement = arrangement }
    }
}

/// One photograph beside the other: its caption, the print, and (in a row) its label.
private struct ComparePane: View {
    let model: EditorModel
    let side: Comparison.Side
    let first: Bool
    let active: Bool
    let picture: ComparePicture?
    let frame: CGRect
    let stacked: Bool
    let differences: CompareDifferences
    let link: ZoomLink
    @Environment(\.sheetInk) private var ink

    var body: some View {
        let open = model.comparedIsOpen(side)
        ZStack(alignment: .topLeading) {
            CompareCanvasPicture(model: model, side: side, picture: picture, open: open, link: link, frame: frame) { _ in
                model.activateCompared(first: first)
            }
            CompareCaption(model: model, side: side, active: active, stacked: stacked, differences: differences) {
                model.activateCompared(first: first)
            }
            .frame(width: max(frame.width, 1), height: stacked ? CompareArrangement.stackedCaption - 6 : 40, alignment: .bottom)
            .offset(x: frame.minX, y: frame.minY - (stacked ? CompareArrangement.stackedCaption : 50))
            if !stacked {
                CompareLabel(model: model, side: side, width: frame.width, differences: differences)
                    .offset(x: frame.minX, y: frame.maxY)
            }
        }
    }
}

/// The print itself, linked to the other's zoom and pan. A click makes it the active one.
private struct CompareCanvasPicture: View {
    let model: EditorModel
    let side: Comparison.Side
    let picture: ComparePicture?
    let open: Bool
    let link: ZoomLink
    let frame: CGRect
    /// The click's place across the frame, 0 to 1.
    let click: (CGFloat) -> Void
    @Environment(\.sheetInk) private var ink

    var body: some View {
        Group {
            if let picture {
                PreviewCanvas(image: picture.image, size: picture.size, rotation: picture.rotation, flip: picture.flip, crop: picture.crop, border: picture.border,
                              filmTexture: model.filmTexture, filmStrip: picture.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage,
                              scale: open ? model.canvasScale : nil, upsideDown: model.upsideDown, settled: picture.settled,
                              doubleClick: { [model] zoomed in model.zoom(fit: zoomed) }, inset: 0, link: link, click: click)
            } else {
                Rectangle().strokeBorder(ink.track, lineWidth: 1)
            }
        }
        .frame(width: max(frame.width, 1), height: max(frame.height, 1))
        // Hidden while its thumbnail flies here from the grid (`SheetFlight`).
        .opacity(model.sheetWaiting.contains(side.item) && !side.isOriginal ? 0 : 1)
        // Where that flight lands. Inside the offset: outside it, the
        // frame reported is the one before the offset.
        .modifier(PlaceReport(image: nil) { [model, side] place in
            guard !side.isOriginal else { return }
            model.places.compare[side.item] = place
        })
        .offset(x: frame.minX, y: frame.minY)
    }
}

/// The ring round a photograph's number: the pencil on the one the panel
/// edits, `track` on the other. A click edits it.
struct CompareRing: View {
    let number: String
    let active: Bool
    let action: () -> Void
    @Environment(\.sheetInk) private var ink

    var body: some View {
        Button(action: action) {
            Text(number)
                .font(Theme.serif(14, weight: 600))
                .foregroundStyle(active ? ink.paper : ink.dim)
                .padding(.horizontal, 6)
                .frame(minWidth: 28, minHeight: 25)
                .overlay {
                    PencilRing(radii: .number)
                        .stroke(active ? ink.accent : ink.track, style: StrokeStyle(lineWidth: active ? 1.8 : 1.4, lineCap: .round))
                        .rotationEffect(PencilRing.tilt)
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(active ? "The panel edits this photograph" : "Edit this photograph")
        .accessibilityLabel("Photograph \(number)")
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

/// A word in a caption or label, with the pencil under it when it differs.
private struct Marked: View {
    let text: String
    let marked: Bool
    var font = Theme.serif(15, italic: true)
    @Environment(\.sheetInk) private var ink

    var body: some View {
        Text(text).font(font).foregroundStyle(marked ? ink.paper : ink.label).lineLimit(1).fixedSize()
            .overlay(alignment: .bottom) {
                if marked { PencilStroke().pencil(ink.highlighter ? nil : ink.accent, highlighter: ink.highlighter).frame(height: 5).offset(y: 5).allowsHitTesting(false) }
            }
    }
}

/// The caption over a compared photograph: its ring and the camera's facts,
/// what differs underlined. Stacked, the camera and ISO go and the film and
/// paper come up from the label to its right.
private struct CompareCaption: View {
    let model: EditorModel
    let side: Comparison.Side
    let active: Bool
    let stacked: Bool
    let differences: CompareDifferences
    let activate: () -> Void
    @Environment(\.sheetInk) private var ink

    var body: some View {
        HStack(alignment: .center, spacing: 9) {
            CompareRing(number: model.compareNumber(side), active: active, action: activate)
            if side.isOriginal {
                Text(model.showsCameraJPEG ? "Camera JPEG · as the camera developed it" : "Original · as shot, undeveloped").font(Theme.serif(15, italic: true)).foregroundStyle(ink.label).lineLimit(1)
            } else {
                ViewThatFits(in: .horizontal) {
                    facts(dropping: stacked ? [.camera, .iso] : [])
                    facts(dropping: stacked ? [.camera, .iso, .focalLength] : [.camera])
                    facts(dropping: [.camera, .iso, .focalLength])
                    // At least the shutter, or what differs.
                    facts(dropping: Set(PhotoFacts.Part.allCases).subtracting(differences.facts.isEmpty ? [.shutter] : differences.facts))
                    Color.clear.frame(width: 0, height: 0)
                }
            }
            Spacer(minLength: 6)
            if stacked { stocks.layoutPriority(1) }
        }
        .accessibilityElement(children: .contain)
    }

    private func facts(dropping dropped: Set<PhotoFacts.Part>) -> some View {
        let parts = model.compareFacts(side)?.parts ?? [:]
        let shown = PhotoFacts.Part.allCases.filter { parts[$0] != nil && !dropped.contains($0) }
        return HStack(alignment: .firstTextBaseline, spacing: 9) {
            ForEach(Array(shown.enumerated()), id: \.offset) { index, part in
                if index > 0 { Text("·").font(Theme.serif(15)).foregroundStyle(ink.track) }
                Marked(text: parts[part] ?? "", marked: differences.facts.contains(part))
            }
        }
        .fixedSize()
    }

    /// "Kodak Gold 200 on Portra Endura": the open photograph's names open the tray.
    @ViewBuilder private var stocks: some View {
        if side.isOriginal {
            Text(model.showsCameraJPEG ? model.cameraJPEG?.lastPathComponent ?? "" : "nothing developed yet").font(Theme.serif(14, italic: true)).foregroundStyle(ink.dim).lineLimit(1)
        } else if model.comparedIsOpen(side) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                StockName(model: model, paper: false, name: model.filmName, size: 16, weight: 600, marked: differences.film)
                Text(model.value("io.scan_film").bool ? "scanned" : "on").font(Theme.serif(13, italic: true)).foregroundStyle(ink.dim).fixedSize()
                if !model.value("io.scan_film").bool {
                    StockName(model: model, paper: true, name: model.paperName, size: 15, weight: 400, marked: differences.paper)
                }
            }
        } else if let stocks = model.compareStocks(side) {
            let scanning = model.compareSides[side]?.state?.params["io.scan_film"].bool ?? false
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Marked(text: model.stockName(stocks.film, paper: false), marked: differences.film, font: Theme.serif(16, weight: 600))
                Text(scanning ? "scanned" : "on").font(Theme.serif(13, italic: true)).foregroundStyle(ink.dim).fixedSize()
                if !scanning { Marked(text: model.stockName(stocks.paper, paper: true), marked: differences.paper, font: Theme.serif(15)) }
            }
            .padding(.bottom, 5)
        }
    }
}

/// The label under a compared photograph in a row: the print label for the
/// open one (its names open the tray), the same words for the other.
private struct CompareLabel: View {
    let model: EditorModel
    let side: Comparison.Side
    let width: CGFloat
    let differences: CompareDifferences
    @Environment(\.sheetInk) private var ink

    var body: some View {
        if side.isOriginal {
            HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.gap) {
                Text(model.originalWord).font(Theme.serif(24, weight: 600)).tracking(-0.24).foregroundStyle(ink.paper)
                Text(model.showsCameraJPEG ? model.cameraJPEG?.lastPathComponent ?? "" : "nothing developed yet").font(Theme.serif(14, italic: true)).foregroundStyle(ink.dim)
            }
            .lineLimit(1)
            .padding(.top, 11)
            .frame(width: max(width, 1), alignment: .leading)
        } else if model.comparedIsOpen(side) {
            PrintLabel(model: model, photoWidth: max(width, 1), canvasWidth: max(width, 1), markFilm: differences.film, markPaper: differences.paper)
        } else if let stocks = model.compareStocks(side) {
            let scanning = model.compareSides[side]?.state?.params["io.scan_film"].bool ?? false
            let film = model.stockName(stocks.film, paper: false), paper = model.stockName(stocks.paper, paper: true)
            let kind = PrintLabelLayout.kind(scanning: scanning)
            let oneRow = PrintLabelLayout.fitsOneRow(film: film, paper: paper, kind: kind, width: width)
            let filmWord = Marked(text: film, marked: differences.film, font: Theme.serif(24, weight: 600)).padding(.bottom, 5)
            let paperWords = HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.inner) {
                Text(kind).font(Theme.serif(14, italic: true)).foregroundStyle(ink.dim).lineLimit(1).fixedSize()
                if !scanning { Marked(text: paper, marked: differences.paper, font: Theme.serif(17.5)).padding(.bottom, 5) }
            }
            Group {
                if oneRow {
                    HStack(alignment: .firstTextBaseline, spacing: PrintLabelLayout.gap) { filmWord; paperWords }.padding(.top, 11)
                } else {
                    VStack(alignment: .leading, spacing: 0) { filmWord; paperWords }.padding(.top, 8)
                }
            }
            .frame(width: max(width, 1), alignment: .topLeading)
            .accessibilityElement(children: .combine)
        }
    }
}

/// Slider: one frame, the first photograph left of the line and the second
/// right of it. At Fit a drag anywhere moves the line; at 100 % the knob
/// moves it and a drag elsewhere pans both.
private struct CompareSlider: View {
    let model: EditorModel
    let comparison: Comparison
    let canvas: CGSize
    let link: ZoomLink
    @State private var lastTap: Date?
    @Environment(\.sheetInk) private var ink

    var body: some View {
        let a = model.comparePicture(comparison.first), b = model.comparePicture(comparison.second)
        let frame = CompareArrangement.sliderFrame(canvas: canvas, aspect: a?.aspect ?? b?.aspect ?? 1.5)
        let split = comparison.split
        let line = frame.width * split
        let differences = model.compareDifferences
        ZStack(alignment: .topLeading) {
            CompareCanvasPicture(model: model, side: comparison.first, picture: a, open: model.comparedIsOpen(comparison.first), link: link, frame: frame) { _ in }
            // The second, fitted into the first's frame: black where its proportions leave room.
            Color.black
                .frame(width: max(frame.width - line, 0), height: frame.height)
                .offset(x: frame.minX + line, y: frame.minY)
                .allowsHitTesting(false)
            CompareCanvasPicture(model: model, side: comparison.second, picture: b, open: model.comparedIsOpen(comparison.second), link: link, frame: frame) { x in
                model.activateCompared(first: x < split)
            }
            .mask(alignment: .topLeading) {
                Rectangle().frame(width: max(frame.width - line, 0), height: frame.height).offset(x: frame.minX + line, y: frame.minY)
            }
            if model.fitImage {
                // Fitted: the whole frame moves the line.
                Color.clear
                    .contentShape(Rectangle())
                    .frame(width: frame.width, height: frame.height)
                    .offset(x: frame.minX, y: frame.minY)
                    // Measured in the slider's space, as the knob is: a gesture
                    // outside `.offset` would count the photograph's left margin twice.
                    .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("compareSlider"))
                        .onChanged { drag in
                            if abs(drag.translation.width) + abs(drag.translation.height) > 2 {
                                model.moveSplit(to: (drag.location.x - frame.minX) / max(frame.width, 1))
                            }
                        }
                        .onEnded { drag in
                            guard abs(drag.translation.width) + abs(drag.translation.height) <= 2 else { return }
                            tapped(at: (drag.location.x - frame.minX) / max(frame.width, 1))
                        })
                    .onHover { inside in if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() } }
            }
            // The line, and its knob.
            Rectangle().fill(ink.paper).frame(width: 1.5, height: frame.height)
                .offset(x: frame.minX + line - 0.75, y: frame.minY)
                .allowsHitTesting(false)
            Knob()
                .offset(x: frame.minX + line - 16, y: frame.midY - 16)
                .gesture(DragGesture(coordinateSpace: .named("compareSlider")).onChanged { drag in
                    model.moveSplit(to: (drag.location.x - frame.minX) / max(frame.width, 1))
                })
                .allowsHitTesting(!model.fitImage)
            // One caption row: the first's facts at the left, the second's at the right.
            HStack(alignment: .center, spacing: 9) {
                CompareRing(number: model.compareNumber(comparison.first), active: comparison.activeFirst) { model.activateCompared(first: true) }
                SliderFacts(model: model, side: comparison.first, differences: differences)
                Spacer(minLength: 12)
                SliderFacts(model: model, side: comparison.second, differences: differences)
                CompareRing(number: model.compareNumber(comparison.second), active: !comparison.activeFirst) { model.activateCompared(first: false) }
            }
            .frame(width: frame.width, height: 40, alignment: .bottom)
            .offset(x: frame.minX, y: frame.minY - 50)
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                CompareLabel(model: model, side: comparison.first, width: frame.width * 0.62, differences: differences)
                Spacer(minLength: 0)
                SliderSecondLabel(model: model, side: comparison.second, differences: differences)
            }
            .frame(width: frame.width, alignment: .topLeading)
            .offset(x: frame.minX, y: frame.maxY)
        }
        .coordinateSpace(.named("compareSlider"))
    }

    /// A click without a drag edits the photograph on its side of the line;
    /// a second click soon after zooms to 100 %.
    private func tapped(at x: CGFloat) {
        if let lastTap, Date().timeIntervalSince(lastTap) < NSEvent.doubleClickInterval {
            self.lastTap = nil
            model.zoom(fit: false)
            return
        }
        lastTap = Date()
        model.activateCompared(first: x < comparison.split)
    }
}

/// The slider's round knob, ‹ › in it.
private struct Knob: View {
    @Environment(\.sheetInk) private var ink
    var body: some View {
        Text("‹›").font(Theme.serif(14, weight: 600)).tracking(3).foregroundStyle(ink.paper)
            .frame(width: 32, height: 32)
            .background(Circle().fill(ink.sheet.opacity(0.85)))
            .overlay(Circle().strokeBorder(ink.paper, lineWidth: 1.5))
            .contentShape(Circle())
            .accessibilityLabel("Slider")
    }
}

/// One side's facts in the slider's caption row.
private struct SliderFacts: View {
    let model: EditorModel
    let side: Comparison.Side
    let differences: CompareDifferences
    @Environment(\.sheetInk) private var ink

    var body: some View {
        if side.isOriginal {
            Text(model.showsCameraJPEG ? "Camera JPEG · the camera’s own" : "Original · as shot").font(Theme.serif(15, italic: true)).foregroundStyle(ink.label).lineLimit(1)
        } else {
            let parts = model.compareFacts(side)?.parts ?? [:]
            ViewThatFits(in: .horizontal) {
                line(parts, [.iso, .shutter, .aperture, .focalLength])
                line(parts, [.shutter, .aperture])
                Color.clear.frame(width: 0, height: 0)
            }
        }
    }

    private func line(_ parts: [PhotoFacts.Part: String], _ wanted: [PhotoFacts.Part]) -> some View {
        let shown = wanted.filter { parts[$0] != nil }
        return HStack(alignment: .firstTextBaseline, spacing: 9) {
            ForEach(Array(shown.enumerated()), id: \.offset) { index, part in
                if index > 0 { Text("·").font(Theme.serif(15)).foregroundStyle(ink.track) }
                Marked(text: parts[part] ?? "", marked: differences.facts.contains(part))
            }
        }
        .fixedSize()
    }
}

/// The slider's label at the right: the second photograph's film.
private struct SliderSecondLabel: View {
    let model: EditorModel
    let side: Comparison.Side
    let differences: CompareDifferences
    @Environment(\.sheetInk) private var ink

    var body: some View {
        if side.isOriginal {
            Text(model.originalWord).font(Theme.serif(24, weight: 600)).foregroundStyle(ink.paper).padding(.top, 11)
        } else if model.comparedIsOpen(side) {
            StockName(model: model, paper: false, name: model.filmName, size: 24, weight: 600, marked: differences.film).padding(.top, 11)
        } else if let stocks = model.compareStocks(side) {
            Marked(text: model.stockName(stocks.film, paper: false), marked: differences.film, font: Theme.serif(24, weight: 600))
                .padding(.bottom, 5).padding(.top, 11)
        }
    }
}

/// Near the top of the panel in compare: which photograph it edits, keeping
/// the pane header's controls above it. Against the original with the
/// original active, it says that can't be edited and offers the other.
struct CompareEditingNote: View {
    let model: EditorModel
    let comparison: Comparison

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            if comparison.originalActive {
                Text("The original can’t be edited.").font(Theme.serif(14, italic: true)).foregroundStyle(Theme.label)
                Spacer(minLength: 8)
                WordButton(title: "Edit \(model.compareNumber(comparison.inactive))") { model.activateCompared(first: !comparison.activeFirst) }
            } else {
                Text("Editing").font(Theme.ui(12)).foregroundStyle(Theme.dim)
                CompareRing(number: model.compareNumber(comparison.active), active: true) {}
                    .allowsHitTesting(false)
                Text("the \(Self.place(comparison, arrangement: model.compareArrangement)) one")
                    .font(Theme.serif(14, italic: true)).foregroundStyle(Theme.label)
                Spacer(minLength: 0)
            }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: 36)
        .accessibilityElement(children: .combine)
    }

    /// "top", "bottom", "left" or "right": where the active one is.
    static func place(_ comparison: Comparison, arrangement: CompareArrangement) -> String {
        if comparison.layout == .sideBySide, arrangement == .column { return comparison.activeFirst ? "top" : "bottom" }
        return comparison.activeFirst ? "left" : "right"
    }
}

/// The title bar's view switch, in every view with a photograph:
/// Grid · Photo · Compare, the one shown in `paper` over the pencil. Compare
/// acts with exactly two photographs selected and is faint otherwise, its
/// tooltip saying why; Grid and Photo leave compare for them.
struct ViewSwitch: View {
    enum Place: Equatable { case grid, photo, compare }
    let model: EditorModel

    static func place(of model: EditorModel) -> Place {
        if model.comparison != nil, model.mode == .photo { return .compare }
        return model.mode == .grid ? .grid : .photo
    }

    var body: some View {
        let place = Self.place(of: model)
        let compare = model.compareWord
        HStack(spacing: 16) {
            word("Grid", on: place == .grid, help: place == .compare ? "Stop comparing and show all photographs (G)" : "All Photographs (G)") {
                model.showGrid()
            }
            word("Photo", on: place == .photo, enabled: place != .grid || model.selection.current != nil,
                 help: place == .compare ? "Stop comparing and edit the photograph" : "Edit Photograph (E)") {
                switch place {
                case .compare: model.leaveCompare(toGrid: false)
                case .grid: model.editCurrent()
                case .photo: break
                }
            }
            word("Compare", on: place == .compare, enabled: place == .compare || compare.enabled,
                 help: place == .compare ? "Comparing two photographs (C to leave)" : compare.help) {
                if place != .compare { model.toggleCompare() }
            }
        }
        .font(Theme.serif(17, italic: true))
        .padding(.trailing, 18)
        .frame(height: 28)
    }

    private func word(_ title: String, on: Bool, enabled: Bool = true, help: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).foregroundStyle(on ? Theme.paper : Theme.dim)
                .padding(.bottom, 3)
                .overlay(alignment: .bottom) { if on { PencilStroke().pencil().frame(height: 5) } }
                .opacity(enabled ? 1 : 0.38)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        // A disabled word still says why.
        .help(help)
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityHint(help)
    }
}
