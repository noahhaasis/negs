import SwiftUI

/// What a tab's count stands for: the controls that differ from their
/// defaults, each able to go back alone or all together (Reset Prep).
enum ResetTarget: Hashable {
    case param(String)
    case stock(String, Int)
    /// Prep › Match camera brightness, back to its setting for new photographs.
    case matchBrightness
    /// Prep › Exposure, back to the match (or 0).
    case exposure
    /// Lab › Scan film directly, back to what the film is (negative or slide).
    case scanFilm
    /// Lab › Output › Colour space, back to the first of the spaces.
    case previewSpace
    case border
    /// Camera › Film format: the value, and the Custom pick.
    case filmFormat
}

struct TabEdit: Identifiable, Equatable {
    let title: String
    let value: String
    let target: ResetTarget
    var id: ResetTarget { target }
}

@MainActor extension InspectorEdits {
    /// The controls on `tab` that are off their defaults, in the order the
    /// tab shows them. Always as many as `count(tab, model)`.
    static func edits(_ tab: InspectorTab, _ model: EditorModel) -> [TabEdit] {
        switch tab {
        case .prep: prepEdits(model)
        case .lab: InspectorSection.allCases.flatMap { labEdits($0, model) }
        case .output: outputEdits(model)
        }
    }

    private static func prepEdits(_ model: EditorModel) -> [TabEdit] {
        var out: [TabEdit] = []
        func path(_ title: String, _ path: String) {
            if model.isModified(path) { out.append(TabEdit(title: title, value: display(model.value(path), path), target: .param(path))) }
        }
        if model.isRaw {
            path("White balance", "io.raw_white_balance.mode")
            path("Temperature (K)", "io.raw_white_balance.temperature")
            path("Tint", "io.raw_white_balance.tint")
        }
        if model.matchesBrightness != model.autoPlaceNewImages {
            out.append(TabEdit(title: "Match camera brightness", value: model.matchesBrightness ? "On" : "Off", target: .matchBrightness))
        }
        if model.exposureOffsetModified {
            out.append(TabEdit(title: "Exposure", value: signed(model.exposureOffset.wrappedValue), target: .exposure))
        }
        path("Auto exposure", "camera.auto_exposure")
        path("Metering", "camera.auto_exposure_method")
        for control in prepToneControls + prepColorControls + prepDetailControls { append(control, group: nil, model, &out) }
        if model.isRaw {
            if model.lensCorrectionSupported { path("Lens corrections", EditorModel.lensCorrectionPath) }
        } else {
            for inputPath in inputPaths { path("Input " + words(inputPath), inputPath) }
        }
        return out
    }

    private static func labEdits(_ section: InspectorSection, _ model: EditorModel) -> [TabEdit] {
        var out: [TabEdit] = []
        func path(_ title: String, _ path: String, unit: String = "") {
            if model.isModified(path) { out.append(TabEdit(title: title, value: display(model.value(path), path) + unit, target: .param(path))) }
        }
        switch section {
        case .camera:
            path("Exposure compensation", "camera.exposure_compensation_ev")
            if model.isModified(FilmFormatPicker.path) {
                let mm = model.value(FilmFormatPicker.path).double
                out.append(TabEdit(title: "Film format", value: FilmFormatPicker.format(mm: mm) ?? "\(number(mm)) mm", target: .filmFormat))
            }
            path("Lens blur (µm)", "camera.lens_blur_um")
        case .film:
            path("Film development", filmDevelopmentPath, unit: " min")
        case .print:
            if scanFilmModified(model) {
                out.append(TabEdit(title: "Scan film directly", value: model.value("io.scan_film").bool ? "On" : "Off", target: .scanFilm))
            }
            path("Print development", printDevelopmentPath, unit: " min")
        case .scan:
            if model.previewSpace != SaveSettings.colorSpaces[0] {
                out.append(TabEdit(title: "Colour space", value: model.previewSpace, target: .previewSpace))
            }
            path("Encode with the colour space’s curve", "io.output_cctf_encoding")
        }
        for group in adjustmentGroups where group.section == section {
            if let enable = group.enable, model.isModified(enable) {
                out.append(TabEdit(title: group.title, value: model.value(enable).bool ? "On" : "Off", target: .param(enable)))
            }
            for control in group.controls { append(control, group: group.title, model, &out) }
        }
        return out
    }

    private static func outputEdits(_ model: EditorModel) -> [TabEdit] {
        var out: [TabEdit] = []
        if model.isModified(ImageSize.targetPath) {
            out.append(TabEdit(title: "Image size", value: "\(number(model.value(ImageSize.targetPath).double)) px", target: .param(ImageSize.targetPath)))
        }
        if model.border.enabled { out.append(TabEdit(title: "Border", value: "On", target: .border)) }
        return out
    }

    private static func append(_ control: Adjustment, group: String?, _ model: EditorModel, _ out: inout [TabEdit]) {
        guard isModified(control, model) else { return }
        func titled(_ title: String) -> String { group.map { "\($0) · \(title)" } ?? title }
        switch control {
        case .slider(let title, let path, let range, _):
            let value = model.value(path).double
            out.append(TabEdit(title: titled(title), value: range.lowerBound < 0 ? signed(value) : number(value), target: .param(path)))
        case .toggle(let title, let path):
            out.append(TabEdit(title: titled(title), value: model.value(path).bool ? "On" : "Off", target: .param(path)))
        case .choice(let title, let path, _):
            out.append(TabEdit(title: titled(title), value: display(model.value(path), path), target: .param(path)))
        case .stockSlider(let title, let path, let index, _, _, _):
            out.append(TabEdit(title: titled(title), value: number(model.stockValue(path, index)), target: .stock(path, index)))
        case .balancePrint, .filterLink: break
        }
    }

    private static func number(_ value: Double) -> String { value.formatted(.number.precision(.fractionLength(0...2)).grouping(.never)) }
    private static func signed(_ value: Double) -> String {
        value.formatted(.number.precision(.fractionLength(0...2)).grouping(.never).sign(strategy: .always(includingZero: false)))
    }
    /// "as_shot" as "As shot".
    private static func words(_ text: String) -> String {
        let spaced = (text.split(separator: ".").last.map(String.init) ?? text).replacingOccurrences(of: "_", with: " ")
        return spaced.prefix(1).uppercased() + spaced.dropFirst()
    }
    private static func display(_ value: JSONValue, _ path: String) -> String {
        switch value {
        case .number(let v): number(v)
        case .bool(let v): v ? "On" : "Off"
        case .string(let s): words(s)
        default: "Changed"
        }
    }
}

// MARK: The list

/// The popover under a tab's count: what changed on the tab, each with its
/// own ↺, and Reset for all of them.
struct TabEditsList: View {
    let model: EditorModel
    let tab: InspectorTab
    let dismiss: () -> Void

    var body: some View {
        let edits = InspectorEdits.edits(tab, model)
        VStack(alignment: .leading, spacing: 0) {
            Text(edits.count == 1 ? "1 edit in \(tab.rawValue)" : "\(edits.count) edits in \(tab.rawValue)")
                .font(Theme.serif(15, weight: 600, italic: true)).foregroundStyle(Theme.paper)
                .padding(.bottom, 8)
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(edits) { edit in
                        VStack(spacing: 0) {
                            InkRule()
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text(edit.title).font(Theme.ui(12)).foregroundStyle(Theme.label).lineLimit(2)
                                Spacer(minLength: 8)
                                Text(edit.value).font(Theme.ui(12)).monospacedDigit().foregroundStyle(Theme.dim).lineLimit(1)
                                Button { model.resetEdits([edit.target], action: "Reset \(edit.title)") } label: {
                                    Text("↺").font(Theme.serif(15)).foregroundStyle(Theme.label).frame(width: 18, height: 18)
                                }
                                .buttonStyle(InkPressStyle())
                                                                .tooltip("Reset \(edit.title)")
                                .accessibilityLabel("Reset \(edit.title)")
                            }
                            .padding(.vertical, 5)
                        }
                    }
                }
            }
            .frame(maxHeight: 320)
            .fixedSize(horizontal: false, vertical: true)
            InkRule()
            HStack {
                WordButton(title: "Reset \(tab.rawValue)", help: "Put every control on \(tab.rawValue) back to its default. You can undo this.") {
                    model.resetEdits(tab: tab)
                    dismiss()
                }
                Spacer()
            }
            .padding(.top, 8)
        }
        .padding(14)
        .frame(width: 300)
        .background(Theme.panel)
        .presentationBackground(Theme.panel)
        .onChange(of: edits.isEmpty) { _, empty in if empty { dismiss() } }
    }
}
