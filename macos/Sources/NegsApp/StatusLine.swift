import AppKit
import SwiftUI

/// The status line (§4.7): 28 pt of words in SF 11 `dim`, no icons. On the
/// left, what's happening: the photograph's size and develop time, an
/// export's progress, cropping, or for a moment a library note. On the
/// right, the canvas's tools: Original (hold it), Fit and 100%.
struct StatusBar: View {
    let model: EditorModel

    var body: some View {
        HStack(spacing: 16) {
            StatusWords(model: model)
            Spacer(minLength: 12)
            if let run = model.batch, run.running {
                StatusLink(title: "Cancel", help: "Stop exporting", action: run.cancel)
            } else if model.exporting {
                StatusLink(title: "Cancel", help: "Stop exporting", action: model.cancelExport)
            }
            if model.source != nil, model.mode == .photo, !model.exporting, model.batch?.running != true {
                StatusTools(model: model)
            }
        }
        .font(Theme.ui(11))
        .foregroundStyle(Theme.dim)
        .lineLimit(1)
        .padding(.horizontal, 18)
        .frame(height: Theme.Size.statusLine)
        .background(Theme.bg)
        .accessibilityElement(children: .contain)
    }
}

/// The left-hand words, whichever applies first.
private struct StatusWords: View {
    let model: EditorModel

    var body: some View {
        if let note = model.libraryNote {
            NoteWords(note: note)
        } else if let run = model.batch, run.running {
            HStack(spacing: 14) {
                Text(StatusText.exporting(run.finished, of: run.total))
                ProgressRule(fraction: run.fraction)
            }
            .foregroundStyle(Theme.label)
        } else if let run = model.batch, run.needsAttention {
            BatchOutcome(model: model, run: run)
        } else if model.exporting {
            Text("Exporting \(model.sourceItem.map { model.library.name(of: $0) } ?? "the photograph")")
                .foregroundStyle(Theme.label)
        } else if let session = model.cropSession {
            Text(StatusText.cropping(session.aspect.title, size: session.exportedSize(working: model.workingSize)))
        } else if model.renderFailed {
            HStack(spacing: 5) {
                Text("Couldn’t develop").fontWeight(.semibold).foregroundStyle(Theme.paper)
                Text(model.sourceItem.map { model.library.name(of: $0) } ?? "the photograph")
            }
            .foregroundStyle(Theme.label)
            .accessibilityElement(children: .combine)
        } else {
            Text(model.status).textSelection(.enabled)
        }
    }
}

/// The status line's words, apart from the views so they can be tested.
enum StatusText {
    /// "Exporting 5 of 12 photographs": the one under way, counted from 1.
    static func exporting(_ finished: Int, of total: Int) -> String {
        "Exporting \(min(finished + 1, total)) of \(total) " + (total == 1 ? "photograph" : "photographs")
    }

    /// "Cropping · 3:2 · 3936 × 2624 px".
    static func cropping(_ ratio: String, size: CGSize) -> String {
        "Cropping · \(ratio) · \(Int(size.width.rounded())) × \(Int(size.height.rounded())) px"
    }

    /// A note's action as the line says it: "Edits Pasted" becomes "Edits
    /// pasted"; stars and one-word notes stay as they are.
    static func action(_ text: String) -> String {
        guard let space = text.firstIndex(of: " ") else { return text }
        return String(text[..<space]) + text[space...].lowercased()
    }

    /// What follows the action: how many photographs, and why.
    static func detail(_ note: EditorModel.LibraryNote) -> String {
        var parts: [String] = []
        if note.count > 1 { parts.append("\(note.count) photographs") }
        if let detail = note.detail { parts.append(detail) }
        return parts.joined(separator: " · ")
    }
}

/// A library note in place of the left-hand words: the action in 600
/// `paper` with the pencil under it, then the rest (§4.7, round 9).
struct NoteWords: View {
    let note: EditorModel.LibraryNote

    var body: some View {
        HStack(spacing: 5) {
            Text(StatusText.action(note.text))
                .fontWeight(.semibold)
                .foregroundStyle(Theme.paper)
                .overlay(alignment: .bottom) { PencilStroke().pencil().frame(height: 5).offset(y: 5) }
            let detail = StatusText.detail(note)
            if !detail.isEmpty { Text(detail).foregroundStyle(Theme.label) }
        }
        .accessibilityElement(children: .combine)
    }
}

/// Hides a library note after 1 s, or 2.5 s when it gives a reason, and
/// reads it to VoiceOver as it comes.
struct LibraryNoteTimer: ViewModifier {
    let model: EditorModel

    func body(content: Content) -> some View {
        content.task(id: model.libraryNote?.serial) {
            guard let note = model.libraryNote else { return }
            let words = [StatusText.action(note.text), StatusText.detail(note)].filter { !$0.isEmpty }.joined(separator: ", ")
            NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested,
                                 userInfo: [.announcement: words, .priority: NSAccessibilityPriorityLevel.high.rawValue])
            try? await Task.sleep(for: .seconds(note.detail == nil ? 1 : 2.5))
            guard !Task.isCancelled else { return }
            withAnimation(.easeOut(duration: 0.25)) { model.hideLibraryNote(note.serial) }
        }
    }
}

/// "Exported 10 of 12 · **2 failed** Show", after a batch that needs a look.
private struct BatchOutcome: View {
    let model: EditorModel
    let run: BatchRun
    @State private var anchor = MenuAnchor()

    var body: some View {
        HStack(spacing: 5) {
            Text("Exported \(run.exported.count) of \(run.total)" + (run.cancelled ? " · cancelled" : ""))
            if !run.failures.isEmpty {
                Text("·")
                Text("\(run.failures.count) failed").fontWeight(.semibold).foregroundStyle(Theme.paper)
            }
            StatusLink(title: "Show", help: "Show what was exported", underlined: true) {
                if let view = anchor.view { model.presentBatch(view) }
            }
            .background(AnchorView(anchor: anchor))
        }
        .foregroundStyle(Theme.label)
    }
}

/// The export's progress: a 150 pt line in `track` with the pencil's yellow along it.
private struct ProgressRule: View {
    let fraction: Double

    var body: some View {
        ZStack(alignment: .leading) {
            Rectangle().fill(Theme.track).frame(height: 1)
            Rectangle().fill(Theme.accent).frame(width: 150 * min(max(fraction, 0), 1), height: 2)
        }
        .frame(width: 150, height: 2)
        .accessibilityElement()
        .accessibilityLabel("Export progress")
        .accessibilityValue("\(Int((fraction * 100).rounded())) percent")
    }
}

/// A word that acts: `dim`, `label` under the pointer.
private struct StatusLink: View {
    let title: String
    let help: String
    var underlined = false
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text(title)
                .foregroundStyle(hovered ? Theme.label : Theme.dim)
                .overlay(alignment: .bottom) {
                    if underlined { Rectangle().fill(Theme.track).frame(height: 1).offset(y: 2) }
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help(help)
    }
}

/// Original (hold it, or \), a 1 × 10 rule, Fit (⌘0) and 100% (⌘1). The
/// current zoom is in `paper`. While cropping the canvas doesn't zoom, so
/// Fit and 100% fade.
private struct StatusTools: View {
    let model: EditorModel

    var body: some View {
        let cropping = model.cropping
        // Nothing to compare or zoom when the photograph couldn't be developed.
        let failed = model.renderFailed
        HStack(spacing: 14) {
            OriginalWord(model: model)
            Rectangle().fill(Theme.track).frame(width: 1, height: 10)
            HStack(spacing: 14) {
                ZoomWord(title: "Fit", help: "Fit photograph (⌘0)", on: model.fitImage) { model.zoom(fit: true) }
                ZoomWord(title: "100%", help: "Actual size (⌘1)", on: !model.fitImage) { model.zoom(fit: false) }
            }
            .opacity(cropping && !failed ? 0.38 : 1)
            .disabled(cropping)
        }
        .opacity(failed ? 0.38 : 1)
        .disabled(failed)
    }
}

private struct ZoomWord: View {
    let title: String
    let help: String
    let on: Bool
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text(title)
                .foregroundStyle(on ? Theme.paper : hovered ? Theme.label : Theme.dim)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help(help)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// Press and hold to compare with the photograph before developing, like
/// Lightroom's Show Original. The decode starts when the pointer arrives.
/// While it's held the word is in `paper` with the pencil under it.
private struct OriginalWord: View {
    let model: EditorModel
    @State private var pressed = false
    @State private var hovered = false

    var body: some View {
        let enabled = model.canShowOriginal
        let on = model.holdingOriginal
        Text("Original")
            .foregroundStyle(on ? Theme.paper : hovered && enabled ? Theme.label : Theme.dim)
            .overlay(alignment: .bottom) {
                if on { PencilStroke().pencil().frame(height: 5).offset(y: 5) }
            }
            .contentShape(Rectangle())
            .opacity(enabled ? 1 : 0.38)
            .onHover { over in hovered = over; if over { model.prepareOriginal() } }
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { _ in if !pressed { pressed = true; model.holdOriginal(true) } }
                .onEnded { _ in pressed = false; model.holdOriginal(false) })
            .allowsHitTesting(enabled)
            .help("Hold to show the original (\\)")
            .accessibilityElement()
            .accessibilityLabel("Show Original")
            .accessibilityAddTraits(.isButton)
            .accessibilityValue(on ? "On" : "Off")
            // VoiceOver can't hold, so activating it toggles.
            .accessibilityAction { model.holdOriginal(!model.holdingOriginal) }
    }
}

extension CropSession {
    /// The exported photograph's size as cropped, before the border.
    func exportedSize(working: CGSize) -> CGSize {
        if let projected = projectedWorking { return settings.size(of: projected) }
        return working == .zero ? shown.size : settings.size(of: working)
    }
}
