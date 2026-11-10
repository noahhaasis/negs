import AppKit
import SwiftUI

// The small states (§4.14): an empty window, a file dragged over it, the
// Developing tag, and a photograph that couldn't be developed.

/// Where the print will go when there's no photograph: 3:2, fitted between
/// the caption's and the label's bands and the canvas's side margins, as a
/// landscape photograph would be.
enum EmptyPrint {
    static func place(in size: CGSize) -> CGRect {
        let margin = ImageScrollView.fitInset
        let top = CanvasArea.captionBand, bottom = CanvasArea.labelBand
        let room = CGSize(width: max(size.width - margin * 2, 1), height: max(size.height - top - bottom, 1))
        let scale = min(room.width / 3, room.height / 2)
        let print = CGSize(width: (3 * scale).rounded(), height: (2 * scale).rounded())
        return CGRect(x: ((size.width - print.width) / 2).rounded(), y: (top + (room.height - print.height) / 2).rounded(),
                      width: print.width, height: print.height)
    }
}

/// Four printer's trim marks beside the print's corners (§4.3): 18 × 1.5 pt,
/// reaching to 26 pt outside each corner, level with the print's top and
/// bottom edges (`status.css`).
struct TrimMarks: Shape {
    var print: CGRect
    static let length: CGFloat = 18, reach: CGFloat = 26, weight: CGFloat = 1.5

    static func marks(beside print: CGRect) -> [CGRect] {
        [print.minY, print.maxY - weight].flatMap { y in
            [CGRect(x: print.minX - reach, y: y, width: length, height: weight),
             CGRect(x: print.maxX + reach - length, y: y, width: length, height: weight)]
        }
    }

    func path(in rect: CGRect) -> Path {
        var path = Path()
        for mark in Self.marks(beside: print) { path.addRect(mark) }
        return path
    }
}

/// No photograph: the trim marks where the print will go, "Open a
/// photograph or a folder" and the recent files where the label goes. With
/// a file over the window, only "drop to open it" and a yellow line round
/// the print's place.
struct EmptyCanvas: View {
    let model: EditorModel
    /// A file is being dragged over the window.
    var dropping = false
    /// The print's place, if a photograph is on screen; else the empty one.
    var print: CGRect?

    var body: some View {
        GeometryReader { proxy in
            let place = print ?? EmptyPrint.place(in: proxy.size)
            ZStack(alignment: .topLeading) {
                if dropping {
                    RoundedRectangle(cornerRadius: 4)
                        .stroke(Theme.accent, lineWidth: 1.5)
                        .frame(width: place.width + 24, height: place.height + 24)
                        .offset(x: place.minX - 12, y: place.minY - 12)
                } else {
                    TrimMarks(print: place).fill(Theme.track)
                    RecentFiles(model: model, width: proxy.size.width - place.minX - ImageScrollView.fitInset)
                        .offset(x: place.minX, y: place.maxY + 14)
                }
                words.frame(width: proxy.size.width, height: proxy.size.height)
            }
        }
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private var words: some View {
        if dropping {
            Text("drop to open it").font(Theme.serif(22, italic: true)).foregroundStyle(Theme.paper)
        } else {
            VStack(spacing: 10) {
                HStack(alignment: .firstTextBaseline, spacing: 0) {
                    OpenWord(action: model.openPanel)
                    Text(" a photograph or a folder")
                }
                .font(Theme.serif(26, weight: 600))
                .foregroundStyle(Theme.paper)
                Text("Explore film stocks, shape the print, and make it your own.")
                    .font(Theme.serif(16, italic: true)).foregroundStyle(Theme.label)
                    .padding(.bottom, 6)
                Text("or drop a RAW, TIFF, PNG or JPEG file, or a folder of them, here")
                    .font(Theme.serif(14, italic: true)).foregroundStyle(Theme.dim)
            }
            .multilineTextAlignment(.center)
            .padding(.horizontal, 24)
        }
    }
}

/// "Open", the clickable word, underlined in `track` (`dim` on hover).
private struct OpenWord: View {
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text("Open")
                .overlay(alignment: .bottom) {
                    Rectangle().fill(hovered ? Theme.dim : Theme.track).frame(height: 1.5).offset(y: 1)
                }
        }
        .buttonStyle(InkPressStyle())
        .onHover { hovered = $0 }
        // ⌘O is the keyboard's way; a focusable word here took the
        // window's first focus and showed its ring at launch.
        .focusable(false)
        .help("Open a photograph or a folder (⌘O)")
    }
}

/// "*Recent* DSC_2650.NEF IMG_0412.CR3 …" where the label goes, as many as fit.
private struct RecentFiles: View {
    let model: EditorModel
    let width: CGFloat

    var body: some View {
        let recent = Array(model.recentFiles.prefix(5))
        if !recent.isEmpty {
            ViewThatFits(in: .horizontal) {
                ForEach((1...recent.count).reversed(), id: \.self) { count in
                    row(Array(recent.prefix(count)))
                }
            }
            .frame(width: max(width, 1), alignment: .leading)
        }
    }

    private func row(_ files: [URL]) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 18) {
            Text("Recent").font(Theme.serif(14, italic: true)).foregroundStyle(Theme.dim)
            ForEach(files, id: \.self) { url in RecentFile(url: url) { model.open(url) } }
        }
        .fixedSize()
    }
}

private struct RecentFile: View {
    let url: URL
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text(url.lastPathComponent)
                .font(Theme.serif(17.5))
                .foregroundStyle(hovered ? Theme.paper : Theme.label)
                .overlay(alignment: .bottom) {
                    if hovered { Rectangle().fill(Theme.track).frame(height: 1).offset(y: 1) }
                }
        }
        .buttonStyle(InkPressStyle())
        .onHover { hovered = $0 }
        // File › Open Recent is the keyboard's way.
        .focusable(false)
        .help(url.path)
    }
}

/// The Developing or Updating tag at the top centre of the photograph,
/// 16 pt below its top edge (§4.14): the sheet's dark, the word in italic
/// `paper`, and a short yellow mark running along a line under it.
struct DevelopTag: View {
    let title: String

    var body: some View {
        VStack(spacing: 5) {
            Text(title).font(Theme.serif(17, italic: true)).foregroundStyle(Theme.paper).fixedSize()
            RunningLine().frame(width: 64)
        }
        .fixedSize()
        .padding(EdgeInsets(top: 7, leading: 16, bottom: 9, trailing: 16))
        .background(RoundedRectangle(cornerRadius: 3).fill(Color(red: 12 / 255, green: 11 / 255, blue: 10 / 255).opacity(0.78)))
        .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(Theme.paper.opacity(0.08), lineWidth: 1))
        .shadow(color: .black.opacity(0.4), radius: 8, y: 4)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityAddTraits(.updatesFrequently)
    }
}

/// What the tag says: Developing until the photograph's first develop is
/// in, Updating once an edit takes long enough to notice, else nothing.
enum DevelopTagTitle {
    static func title(source: Bool, failed: Bool, developed: Bool, slow: Bool) -> String? {
        guard source, !failed else { return nil }
        if !developed { return "Developing" }
        return slow ? "Updating" : nil
    }
}

/// A photograph that couldn't be developed, said where it would be (§4.14):
/// "DSC_2650.NEF couldn’t be developed", the app's reason, and Try Again.
struct DevelopFailure: View {
    let name: String
    let reason: String
    let retry: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            Text("\(name) couldn’t be developed")
                .font(Theme.serif(24, weight: 600)).foregroundStyle(Theme.paper)
            Text(reason)
                .font(Theme.serif(16, italic: true)).foregroundStyle(Theme.label)
                .fixedSize(horizontal: false, vertical: true)
            Button("Try Again", action: retry)
                .buttonStyle(OutlinedButtonStyle())
                .padding(.top, 12)
                .help("Develop the photograph again")
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, 60)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }
}

/// The trim marks beside the photograph, which are also crop (§4.3): `track`,
/// `label` while the pointer is over the photograph, `paper` on the mark
/// under it. A click on one, or a drag inward, starts cropping. Each takes
/// clicks ±9 pt round its line.
struct CropTrimMarks: View {
    let print: CGRect
    /// The pointer is over the photograph.
    let lit: Bool
    let crop: () -> Void

    var body: some View {
        let marks = TrimMarks.marks(beside: print)
        ZStack(alignment: .topLeading) {
            ForEach(marks.indices, id: \.self) { index in
                let mark = marks[index]
                // Left or right of the print, top or bottom.
                let handle = CropGeometry.Handle(x: mark.midX < print.midX ? -1 : 1, y: mark.midY < print.midY ? -1 : 1)
                TrimMark(lit: lit, handle: handle, crop: crop)
                    .frame(width: mark.width + 8, height: mark.height + 18)
                    .offset(x: mark.minX - 4, y: mark.minY - 9)
            }
        }
    }
}

private struct TrimMark: View {
    let lit: Bool
    let handle: CropGeometry.Handle
    let crop: () -> Void
    @State private var hovered = false
    @State private var started = false

    var body: some View {
        Rectangle()
            .fill(hovered ? Theme.paper : lit ? Theme.label : Theme.track)
            .frame(width: TrimMarks.length, height: TrimMarks.weight)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
            .animation(.easeOut(duration: 0.2), value: lit)
            .onHover { inside in
                hovered = inside
                (inside ? CropEditor.cursor(for: handle) : NSCursor.arrow).set()
            }
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { value in
                    // Inward: towards the print's centre from this corner.
                    let inward = -value.translation.width * CGFloat(handle.x) - value.translation.height * CGFloat(handle.y)
                    if !started, inward > 4 { started = true; crop() }
                }
                .onEnded { value in
                    defer { started = false }
                    if !started, hypot(value.translation.width, value.translation.height) < 4 { crop() }
                })
            .help("Crop (⌘K)")
            .accessibilityElement()
            .accessibilityLabel("Crop")
            .accessibilityAddTraits(.isButton)
            .accessibilityAction(.default, crop)
    }
}
