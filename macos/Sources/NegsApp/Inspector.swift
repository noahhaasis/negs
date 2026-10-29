import SwiftUI

enum Adjustment: Identifiable {
    case slider(String, String, ClosedRange<Double>, Double)
    case toggle(String, String)
    case choice(String, String, [String])
    /// One lane (index) of an engine array that stays null until edited, so
    /// the film's grain preset from the catalog applies. B&W films have one
    /// emulsion: they show the lane under `bw`, or hide it when that is nil.
    case stockSlider(String, String, Int, ClosedRange<Double>, Double, bw: String?)
    /// A button that solves for parameters: title, help.
    case balancePrint(String, String)

    var id: String {
        switch self {
        case .slider(_, let path, _, _), .toggle(_, let path), .choice(_, let path, _): return path
        case .stockSlider(_, let path, let index, _, _, _): return "\(path).\(index)"
        case .balancePrint(let title, _): return title
        }
    }
}

enum InspectorSection: String, CaseIterable {
    case camera = "Camera", film = "Film", print = "Print", scan = "Scan & Color"
}

/// The inspector's two tabs: developing the film, then finishing the scan.
enum InspectorTab: String, CaseIterable {
    case lab = "Lab", post = "Post"
}

struct AdjustmentGroup: Identifiable {
    let title: String
    let symbol: String
    let section: InspectorSection
    /// Switches the whole effect on and off from the group's header.
    var enable: String?
    let controls: [Adjustment]
    var id: String { title }
}

let adjustmentGroups: [AdjustmentGroup] = [
    .init(title: "Lens Diffusion", symbol: "camera.filters", section: .camera, enable: "camera.diffusion_filter.active", controls: [
        .choice("Filter", "camera.diffusion_filter.filter_family", ["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"]),
        .slider("Strength", "camera.diffusion_filter.strength", 0...2, 0.01),
        .slider("Spatial scale", "camera.diffusion_filter.spatial_scale", 0.1...3, 0.01),
        .slider("Halo warmth", "camera.diffusion_filter.halo_warmth", -1.5...1.5, 0.01),
        .slider("Core intensity", "camera.diffusion_filter.core_intensity", 0...2, 0.01),
        .slider("Halo intensity", "camera.diffusion_filter.halo_intensity", 0...2, 0.01),
        .slider("Bloom intensity", "camera.diffusion_filter.bloom_intensity", 0...2, 0.01),
        .slider("Halo size", "camera.diffusion_filter.halo_size", 0.1...3, 0.01),
        .slider("Bloom size", "camera.diffusion_filter.bloom_size", 0.1...3, 0.01)
    ]),
    .init(title: "Film Curves", symbol: "chart.xyaxis.line", section: .film, enable: "film_render.density_curves_morph.active", controls: [
        .slider("Gamma", "film_render.density_curves_morph.gamma_factor", 0.5...2, 0.01),
        .slider("Fast gamma", "film_render.density_curves_morph.gamma_factor_fast", 0.5...2, 0.01),
        .slider("Slow gamma", "film_render.density_curves_morph.gamma_factor_slow", 0.5...2, 0.01),
        .slider("Red gamma", "film_render.density_curves_morph.gamma_factor_red", 0.5...2, 0.01),
        .slider("Green gamma", "film_render.density_curves_morph.gamma_factor_green", 0.5...2, 0.01),
        .slider("Blue gamma", "film_render.density_curves_morph.gamma_factor_blue", 0.5...2, 0.01),
        .slider("Developer exhaustion", "film_render.density_curves_morph.developer_exhaustion", 0...1, 0.01)
    ]),
    .init(title: "Grain", symbol: "circle.dotted", section: .film, enable: "film_render.grain.active", controls: [
        .stockSlider("Red granularity", "film_render.grain.rms_granularity", 0, 0...80, 0.1, bw: "Granularity"),
        .stockSlider("Green granularity", "film_render.grain.rms_granularity", 1, 0...80, 0.1, bw: nil),
        .stockSlider("Blue granularity", "film_render.grain.rms_granularity", 2, 0...80, 0.1, bw: nil),
        .stockSlider("Red uniformity", "film_render.grain.uniformity", 0, 0...1, 0.01, bw: "Uniformity"),
        .stockSlider("Green uniformity", "film_render.grain.uniformity", 1, 0...1, 0.01, bw: nil),
        .stockSlider("Blue uniformity", "film_render.grain.uniformity", 2, 0...1, 0.01, bw: nil),
        .stockSlider("Coarse grain size", "film_render.grain.particle_scale_sublayers", 0, 0.05...2, 0.01, bw: "Coarse grain size"),
        .stockSlider("Medium grain size", "film_render.grain.particle_scale_sublayers", 1, 0.05...2, 0.01, bw: "Medium grain size"),
        .stockSlider("Fine grain size", "film_render.grain.particle_scale_sublayers", 2, 0.05...2, 0.01, bw: "Fine grain size"),
        .slider("Dye clouds (µm)", "film_render.grain.blur_dye_clouds_um", 0...5, 0.01),
        .slider("Blur (px)", "film_render.grain.blur", 0...3, 0.01),
        .slider("Sharpen amount", "film_render.grain.mult_usm_amount", 0...3, 0.01),
        .slider("Sharpen radius (px)", "film_render.grain.mult_usm_sigma", 0...3, 0.01),
        .slider("Clump blur (µm)", "film_render.grain.micro_structure.0", 0...1, 0.01),
        .slider("Clump size (nm)", "film_render.grain.micro_structure.1", 0...100, 1)
    ]),
    .init(title: "Halation", symbol: "sun.haze", section: .film, enable: "film_render.halation.active", controls: [
        .slider("Amount", "film_render.halation.halation_amount", 0...3, 0.01),
        .slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1...5, 0.01),
        .slider("Scatter amount", "film_render.halation.scatter_amount", 0...3, 0.01),
        .slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1...5, 0.01),
        .slider("Bounces", "film_render.halation.halation_n_bounces", 1...5, 1),
        .slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0...1, 0.01)
    ]),
    .init(title: "DIR Couplers", symbol: "circle.hexagongrid", section: .film, enable: "film_render.dir_couplers.active", controls: [
        .slider("Amount", "film_render.dir_couplers.amount", 0...2, 0.01),
        .slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0...100, 0.1),
        .slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0...400, 1),
        .slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0...1, 0.01)
    ]),
    .init(title: "Film Base", symbol: "square.stack.3d.down.right", section: .film, enable: "film_render.base.active", controls: [
        .slider("Density", "film_render.base.scale", 0...2, 0.01),
        .slider("Tilt", "film_render.base.tilt", -1...1, 0.01),
        .slider("Cyan", "film_render.base.cyan", 0...2, 0.01),
        .slider("Magenta", "film_render.base.magenta", 0...2, 0.01),
        .slider("Yellow", "film_render.base.yellow", 0...2, 0.01)
    ]),
    .init(title: "Enlarger", symbol: "light.overhead.left", section: .print, controls: [
        .slider("Print exposure", "enlarger.print_exposure", 0.1...5, 0.01),
        .toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
        .slider("Magenta shift", "enlarger.m_filter_shift", -50...50, 0.1),
        .slider("Yellow shift", "enlarger.y_filter_shift", -50...50, 0.1),
        .slider("Preflash exposure", "enlarger.preflash_exposure", 0...0.5, 0.01),
        .slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50...50, 0.1),
        .slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50...50, 0.1),
        .balancePrint("Balance Print", "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base.")
    ]),
    .init(title: "Print Curves", symbol: "chart.xyaxis.line", section: .print, enable: "print_render.density_curves_morph.active", controls: [
        .slider("Gamma", "print_render.density_curves_morph.gamma_factor", 0.5...2, 0.01),
        .slider("Fast gamma", "print_render.density_curves_morph.gamma_factor_fast", 0.5...2, 0.01),
        .slider("Slow gamma", "print_render.density_curves_morph.gamma_factor_slow", 0.5...2, 0.01),
        .slider("Red gamma", "print_render.density_curves_morph.gamma_factor_red", 0.5...2, 0.01),
        .slider("Green gamma", "print_render.density_curves_morph.gamma_factor_green", 0.5...2, 0.01),
        .slider("Blue gamma", "print_render.density_curves_morph.gamma_factor_blue", 0.5...2, 0.01),
        .slider("Developer exhaustion", "print_render.density_curves_morph.developer_exhaustion", 0...1, 0.01)
    ]),
    .init(title: "Print Base", symbol: "square.stack.3d.down.right", section: .print, enable: "print_render.base.active", controls: [
        .slider("Density", "print_render.base.scale", 0...2, 0.01),
        .slider("Cyan", "print_render.base.cyan", 0...2, 0.01),
        .slider("Magenta", "print_render.base.magenta", 0...2, 0.01),
        .slider("Yellow", "print_render.base.yellow", 0...2, 0.01)
    ]),
    .init(title: "Glare", symbol: "light.max", section: .print, enable: "print_render.glare.active", controls: [
        .slider("Amount", "print_render.glare.percent", 0...0.2, 0.001),
        .slider("Roughness", "print_render.glare.roughness", 0...2, 0.01),
        .slider("Blur (px)", "print_render.glare.blur", 0...5, 0.01)
    ]),
    .init(title: "Scanner", symbol: "scanner", section: .scan, controls: [
        .slider("Lens blur (px)", "scanner.lens_blur", 0...5, 0.01),
        .slider("Unsharp radius (px)", "scanner.unsharp_mask.0", 0...3, 0.01),
        .slider("Unsharp amount", "scanner.unsharp_mask.1", 0...2, 0.01),
        .toggle("White correction", "scanner.white_correction"),
        .slider("White level", "scanner.white_level", 0.5...1, 0.01),
        .toggle("Black correction", "scanner.black_correction"),
        .slider("Black level", "scanner.black_level", 0...0.5, 0.01)
    ]),
    .init(title: "Color Management", symbol: "paintpalette", section: .scan, controls: [
        .choice("Output gamut", "io.output_gamut_compress.algorithm", ["off", "oklch", "oklrab", "cam16ucs", "aces_rgc"]),
        .choice("Input gamut", "io.input_gamut_compress.algorithm", ["off", "xy"]),
        .toggle("CAT16 adaptation", "settings.use_cat16"),
        .choice("Spectral upsampling", "settings.rgb_to_raw_method", ["hanatos2025", "arctic2026beta04", "mallett2019"])
    ])
]

struct AdjustmentControl: View {
    let model: EditorModel
    let adjustment: Adjustment

    var body: some View {
        switch adjustment {
        case .toggle(let title, let path):
            Toggle(title, isOn: model.toggle(path)).resettable(model, path)
        case .choice(let title, let path, let options):
            Picker(title, selection: model.choice(path)) {
                ForEach(options, id: \.self) { Text(optionTitle($0)).tag($0) }
            }.resettable(model, path)
        case .slider(let title, let path, let range, let step):
            SliderRow(model: model, title: title, path: path, range: range, step: step)
        case .stockSlider(let title, let path, let index, let range, let step, let bwTitle):
            if !model.filmIsBW || bwTitle != nil {
                LabeledSlider(title: model.filmIsBW ? bwTitle ?? title : title,
                              value: model.stockNumber(path, index, range: range, step: step), range: range, step: step,
                              defaultValue: model.stockDefault(path, index), modified: model.isStockModified(path, index),
                              reset: { model.resetStock(path, index) },
                              editing: { $0 ? model.beginInteraction() : model.endInteraction() })
            }
        case .balancePrint(let title, let help):
            HStack {
                Spacer()
                Button(model.balancingPrint ? "Balancing…" : title) { model.balancePrint() }
                    .disabled(!model.canBalancePrint)
                    .help(help)
            }
        }
    }
}

struct SliderRow: View {
    let model: EditorModel
    let title: String
    let path: String
    let range: ClosedRange<Double>
    let step: Double

    var body: some View {
        LabeledSlider(title: title, value: model.number(path, range: range, step: step), range: range, step: step,
                      defaultValue: model.defaultValue(path).double, modified: model.isModified(path),
                      reset: { model.resetValue(path) },
                      editing: { $0 ? model.beginInteraction() : model.endInteraction() })
    }
}

/// A slider under its title and value. Clicking the value types one; off
/// its default, a button resets it.
struct LabeledSlider: View {
    let title: String
    let value: Binding<Double>
    let range: ClosedRange<Double>
    let step: Double
    /// Shown after the value, such as "%".
    var unit = ""
    let defaultValue: Double
    let modified: Bool
    let reset: () -> Void
    let editing: (Bool) -> Void

    @State private var typing = false
    private var decimals: Int { max(0, Int(-log10(step).rounded(.down))) }
    private var format: FloatingPointFormatStyle<Double> { .number.precision(.fractionLength(0...decimals)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                Text(title).lineLimit(1)
                    .onTapGesture(count: 2) { reset() }
                Spacer(minLength: 8)
                if modified {
                    Button { reset() } label: { Image(systemName: "arrow.counterclockwise") }
                        .buttonStyle(.borderless).foregroundStyle(.secondary).imageScale(.small)
                        .help("Reset to \(defaultValue.formatted(format))\(unit)")
                        .accessibilityLabel("Reset \(title)")
                }
                // A field only while typing, so the window never focuses one by default.
                if typing {
                    ValueField(title: title, value: value, decimals: decimals) { typing = false }
                        .frame(width: 64)
                } else {
                    Text("\(value.wrappedValue.formatted(format))\(unit)")
                        .monospacedDigit().foregroundStyle(.secondary)
                        .frame(minWidth: 40, alignment: .trailing)
                        .contentShape(Rectangle())
                        .onTapGesture { typing = true }
                        .help("Click to type a value")
                }
            }
            Slider(value: value, in: range) { Text(title) } onEditingChanged: { editing($0) }
            .labelsHidden().controlSize(.small)
        }
        .padding(.vertical, 1)
        .resettable(modified: modified, reset: reset)
        .accessibilityElement(children: .contain)
    }
}

/// The number field shown while typing a slider's value. AppKit, because
/// SwiftUI's focus never reaches a field in the inspector column; the field
/// takes focus as it appears, which selects the value so typing replaces it.
private struct ValueField: NSViewRepresentable {
    let title: String
    @Binding var value: Double
    let decimals: Int
    let onEnd: () -> Void

    private var formatter: NumberFormatter {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.usesGroupingSeparator = false
        formatter.maximumFractionDigits = decimals
        formatter.isLenient = true
        return formatter
    }

    func makeNSView(context: Context) -> NSTextField {
        let field = FocusingTextField()
        field.controlSize = .small
        field.bezelStyle = .roundedBezel
        field.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize(for: .small), weight: .regular)
        field.alignment = .right
        field.setAccessibilityLabel(title)
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.currentEditor() == nil { field.stringValue = formatter.string(from: value as NSNumber) ?? "" }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: ValueField
        private var cancelled = false
        init(_ parent: ValueField) { self.parent = parent }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
            cancelled = true
            control.window?.makeFirstResponder(nil)
            return true
        }

        /// Return, Tab, clicking elsewhere and Escape all end here; only Escape discards the typing.
        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            if !cancelled, let number = parent.formatter.number(from: field.stringValue) { parent.value = number.doubleValue }
            parent.onEnd()
        }
    }
}

/// Takes focus as it appears, selecting its text, in the inspector column
/// where SwiftUI focus doesn’t reach.
final class FocusingTextField: NSTextField {
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.async { [weak self] in
            guard let self, let window = self.window else { return }
            window.makeFirstResponder(self)
        }
    }
}

/// Image Size, in the Lab between the film and the camera: the photograph's
/// resolution before the film develops it. Reads only its own parameters,
/// the crop and the source's size, so other edits don't redraw it.
struct ImageSizeSection: View {
    let model: EditorModel
    @State private var fieldError: String?

    var body: some View {
        let params: JSONValue = .object(["io": .object(["photo_long_side_px": model.value(ImageSize.targetPath)])])
        let mode = ImageSize.mode(of: params)
        let source = model.sourceSize
        let working = source == .zero ? CGSize.zero : ImageSize.working(source: source, params: params, crop: model.crop)
        let photograph = model.crop.size(of: working)
        Section {
            Picker("Size", selection: Binding(get: { mode }, set: { fieldError = nil; model.chooseImageSize($0) })) {
                Text("Original").tag(ImageSize.Mode.original)
                Text("Longest side").tag(ImageSize.Mode.longestSide)
            }
            .help("Original develops the photograph’s own pixels. Longest side enlarges or reduces it first, so the cropped photograph has that many pixels on its longest side.")
            if mode == .longestSide {
                LabeledContent("Longest side") {
                    HStack(spacing: 4) {
                        PixelField(title: "Longest side", value: ImageSize.target(of: params) ?? 0) { pixels in
                            fieldError = model.setLongestSide(pixels)
                            return fieldError == nil
                        }
                        .frame(width: 72)
                        Text("px").foregroundStyle(.secondary)
                    }
                }
                .help("The cropped photograph’s longest side, before any border. Exports can still be reduced in the save panel.")
                if let fieldError {
                    Text(fieldError).font(.caption).foregroundStyle(.red)
                }
            }
            LabeledContent("Photograph") {
                // Verbatim, so pixel counts read like the status bar's, without grouping.
                Text(verbatim: photograph == .zero ? "—" : "\(Int(photograph.width)) × \(Int(photograph.height)) · \(ImageSize.megapixels(photograph)) MP")
                    .monospacedDigit()
            }
            .help("The exported photograph, cropped, before rotation, the border, and any export size.")
            if ImageSize.exceedsLimit(working: working, source: source) {
                Label(ImageSize.limitMessage(working: working), systemImage: "exclamationmark.triangle.fill")
                    .font(.caption).foregroundStyle(.red)
            } else if model.previewScale < 1, model.image != nil {
                Text("The preview shows \(EditorModel.percent(model.previewScale)) of these pixels. Exports develop all of them; inspect grain in an exported file.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        } header: { Text("Image Size") } footer: {
            Text("Resizes before film simulation, including halation and grain.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// A whole number of pixels, committed on Return or when focus leaves. An
/// invalid value stays in the field until corrected; Escape restores the
/// current one. AppKit, like `ValueField`, so the inspector column can focus it.
private struct PixelField: NSViewRepresentable {
    let title: String
    let value: Int
    /// Applies a typed value; false keeps the typing for correction.
    let commit: (Int) -> Bool

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.controlSize = .small
        field.bezelStyle = .roundedBezel
        field.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize(for: .small), weight: .regular)
        field.alignment = .right
        field.setAccessibilityLabel(title)
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.currentEditor() == nil, !context.coordinator.rejected { field.stringValue = "\(value)" }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: PixelField
        /// The field holds a value that wasn't applied.
        var rejected = false
        init(_ parent: PixelField) { self.parent = parent }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
            rejected = false
            (control as? NSTextField)?.stringValue = "\(parent.value)"
            control.window?.makeFirstResponder(nil)
            return true
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            let text = field.stringValue.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: ",", with: "")
            guard text != "\(parent.value)" || rejected else { return }
            rejected = !parent.commit(Int(text) ?? -1)
        }
    }
}

private extension View {
    func resettable(_ model: EditorModel, _ path: String) -> some View {
        resettable(modified: model.isModified(path)) { model.resetValue(path) }
    }
    func resettable(modified: Bool, reset: @escaping () -> Void) -> some View {
        contextMenu {
            Button("Reset to Default", action: reset).disabled(!modified)
        }
    }
}

struct AdjustmentInspector: View {
    let model: EditorModel
    @AppStorage("expandedAdjustments") private var expanded = "Enlarger"
    @AppStorage("inspectorTab") private var tab = InspectorTab.lab

    var body: some View {
        // The histogram and tab switch stay put above the tab's scrolling sections.
        VStack(spacing: 0) {
            VStack(spacing: 10) {
                HistogramView(histogram: model.histogram).frame(height: 88)
                Picker("Tab", selection: $tab) {
                    ForEach(InspectorTab.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented).labelsHidden()
                .frame(maxWidth: .infinity)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 10)
            Divider()
            Form {
                switch tab {
                case .lab: lab
                case .post: post
                }
            }
            .formStyle(.grouped)
            // Pinned below the sections, like the Reset Adjustments button in Photos.
            Button(action: model.reset) {
                Text("Reset Adjustments").frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.extraLarge)
            .help("Restore defaults on both tabs while keeping the film, paper, rotation, crop, border, and Image Size. You can undo this.")
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
        .disabled(!model.hasCatalog)
    }

    @ViewBuilder private var lab: some View {
        let scanning = model.value("io.scan_film").bool
        Section("Film & Paper") {
            Picker("Film", selection: Binding(get: { model.film }, set: model.chooseFilm)) {
                ForEach(model.filmGroups) { group in
                    Section(group.title) { ForEach(group.stocks) { Text($0.name).tag($0.id) } }
                }
            }
            developmentTime(stock: model.films.first { $0.id == model.film }, path: "film_render.development_time", label: "Development")
            Toggle("Scan film directly", isOn: model.toggle("io.scan_film"))
                .help("Skip printing and scan the developed film, as for slides.")
            if !scanning {
                Picker("Paper", selection: Binding(get: { model.paper }, set: model.choosePaper)) {
                    ForEach(model.paperGroups) { group in
                        Section(group.title) { ForEach(group.stocks) { Text($0.name).tag($0.id) } }
                    }
                }
                developmentTime(stock: model.papers.first { $0.id == model.paper }, path: "print_render.development_time", label: "Print development")
            }
        }
        ImageSizeSection(model: model)
        Section("Camera") {
            Toggle("Auto exposure", isOn: model.toggle("camera.auto_exposure"))
            if model.value("camera.auto_exposure").bool {
                control(.choice("Metering", "camera.auto_exposure_method", ["average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"]))
            }
            control(.slider("Exposure (EV)", "camera.exposure_compensation_ev", -5...5, 0.1))
            control(.slider("Film format (mm)", "camera.film_format_mm", 4...120, 0.1))
            control(.slider("Lens blur (µm)", "camera.lens_blur_um", 0...100, 0.1))
            if !model.isRaw { inputColour }
            if model.isRaw {
                disclosure("RAW White Balance", symbol: "thermometer.sun") {
                    control(.choice("White balance", "io.raw_white_balance.mode", ["as_shot", "daylight", "tungsten", "custom"]))
                    if model.value("io.raw_white_balance.mode").string == "custom" {
                        control(.slider("Temperature (K)", "io.raw_white_balance.temperature", 1667...25000, 1))
                        control(.slider("Tint", "io.raw_white_balance.tint", 0.01...4, 0.01))
                    }
                }
                if model.lensCorrectionSupported {
                    control(.toggle("Lens corrections", EditorModel.lensCorrectionPath))
                        .help("Corrects distortion and vignetting with the data the camera recorded for its lens.")
                }
            }
            groups(.camera)
        }
        Section("Film") { groups(.film) }
        Section {
            groups(.print).disabled(scanning)
        } header: { Text("Print") } footer: {
            if scanning { Text("Printing is skipped while scanning film directly.").font(.caption).foregroundStyle(.secondary) }
        }
        Section("Scan & Color") {
            groups(.scan)
            disclosure("Output", symbol: "square.and.arrow.down.on.square") {
                Toggle("Limit preview to 12 MP", isOn: Binding(get: { model.limitsPreview }, set: model.limitPreview))
                    .help("Develops the preview faster on large photographs. Export keeps the full Image Size.")
                Picker("Colour space", selection: Binding(get: { model.previewSpace }, set: model.choosePreviewSpace)) {
                    ForEach(SaveSettings.colorSpaces, id: \.self) { Text($0).tag($0) }
                }
                .help("The preview develops in this colour space, so it shows the colours an export in it can hold. Save and Export use it next, and switch it when you pick another.")
                Toggle("Encode with the colour space’s curve", isOn: model.toggle("io.output_cctf_encoding"))
                    .help("Off writes linear values, for further processing.")
                Toggle("Use less memory for export", isOn: model.toggle("settings.serial_grain"))
                    .help("Processes grain channels one at a time. Same result, less memory, longer export.")
            }
        }
    }

    /// How a non-RAW photograph's values are read: automatically from its
    /// profile and tags, as an assumed colour space, or (Manual) by the input
    /// colour space and curve, as Negs 0.8 read every file.
    @ViewBuilder private var inputColour: some View {
        disclosure("Input Colour", symbol: "eyedropper") {
            Picker("Read as", selection: model.inputInterpretation) {
                Text("Automatic").tag("auto")
                Section("Assume") {
                    ForEach(["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020"], id: \.self) { Text($0).tag("assume:" + $0) }
                }
                Text("Manual").tag("manual")
            }
            .help("Automatic reads the embedded profile or tags; untagged files are read as sRGB. Assume overrides them. Manual uses the colour space and curve below, as Negs 0.8 did.")
            switch model.value("io.input_interpretation.mode").string {
            case "assume":
                Toggle("Linear values", isOn: model.toggle("io.input_interpretation.linear"))
            case "manual":
                control(.choice("Colour space", "io.input_color_space", ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020", "ACES2065-1", "ACEScg"]))
                Toggle("Decode the curve", isOn: model.toggle("io.input_cctf_decoding"))
            default: EmptyView()
            }
            let colour = model.inputColour
            if colour != .null {
                VStack(alignment: .leading, spacing: 2) {
                    Text(colour["summary"].string)
                    if colour["automatic"].string != colour["summary"].string { Text("Automatic: \(colour["automatic"].string)") }
                    if !colour["warning"].string.isEmpty { Text(colour["warning"].string).foregroundStyle(.orange) }
                }
                .font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    /// Finishing edits on the scan, after the film. They shape the tones the
    /// film produced; detail the paper clipped comes back only in the Lab.
    @ViewBuilder private var post: some View {
        Section {
            control(.slider("Brightness", "post.brightness", -100...100, 1))
            control(.slider("Contrast", "post.contrast", -100...100, 1))
            control(.slider("Whites", "post.whites", -100...100, 1))
            control(.slider("Blacks", "post.blacks", -100...100, 1))
        } header: { Text("Tone") } footer: {
            Text("Adjusts the finished scan/print. To recover a clipped highlight, use exposure in the Lab.")
                .font(.caption).foregroundStyle(.secondary)
        }
        Section("Color") {
            control(.slider("Saturation", "post.saturation", -100...100, 1))
        }
        Section("Border") {
            Toggle("Add border", isOn: model.border(\.enabled))
            Group {
                let photo = model.photoSize == .zero ? nil : model.photoSize
                BorderStylePicker(selection: model.border(\.style), photo: photo, colors: (model.border.color, model.border.lineColor))
                if let photo, !BorderStyle.film35.fits(photo) {
                    Text("35mm needs a 3:2 or 3:4 photograph. This one is \(Self.ratio(photo)); crop it to 3:2 or 4:3 to use it.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                borderControls
                Toggle("Extend to 3:4 for Instagram", isOn: model.border(\.instagram))
                    .help("Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post.")
                Toggle("Include in export", isOn: model.border(\.export))
                    .help("Adds the border to exported photographs and saved previews.")
            }
            .disabled(!model.border.enabled)
        }
    }

    /// The chosen style's width and colours. Solid and Keyline share the
    /// mat; the film frame's width adds black around it.
    @ViewBuilder private var borderControls: some View {
        switch model.border.style {
        case .solid, .keyline:
            let keyline = model.border.style == .keyline
            borderSlider("Width", \.width, 0...15, step: 0.1)
            if keyline { borderSlider("Line width", \.line, 0.05...2, step: 0.05) }
            ColorPicker("Color", selection: model.borderColor(\.color), supportsOpacity: false)
            if keyline { ColorPicker("Line color", selection: model.borderColor(\.lineColor), supportsOpacity: false) }
        case .film:
            borderSlider("Width", \.filmWidth, 0...15, step: 0.1)
                .help("Adds black around the film frame.")
        case .film35:
            EmptyView()
        }
    }

    /// A border width in percent of the photograph's long edge.
    private func borderSlider(_ title: String, _ setting: WritableKeyPath<BorderSettings, Double>, _ range: ClosedRange<Double>, step: Double) -> some View {
        let initial = BorderSettings()[keyPath: setting]
        return LabeledSlider(title: title, value: model.borderPercent(setting, range: range, step: step), range: range, step: step, unit: "%",
                             defaultValue: initial * 100, modified: abs(model.border[keyPath: setting] - initial) > 1e-9,
                             reset: { model.border(setting).wrappedValue = initial },
                             editing: { $0 ? model.beginInteraction() : model.endInteraction(action: "Change Border") })
    }

    private func control(_ adjustment: Adjustment) -> some View { AdjustmentControl(model: model, adjustment: adjustment) }
    /// A photograph's aspect ratio as people name it, such as 16:9.
    static func ratio(_ size: CGSize) -> String {
        let ratio = max(size.width, size.height) / min(size.width, size.height)
        let named = [(1.0, "1:1"), (5.0 / 4, "5:4"), (7.0 / 5, "7:5"), (16.0 / 9, "16:9"), (2.0, "2:1"), (21.0 / 9, "21:9")]
        if let (_, name) = named.first(where: { abs(ratio / $0.0 - 1) <= 0.01 }) { return name }
        return String(format: "%.2f:1", Double(ratio))
    }

    @ViewBuilder private func groups(_ section: InspectorSection) -> some View {
        ForEach(adjustmentGroups.filter { $0.section == section }) { group in
            disclosure(group.title, symbol: group.symbol, enable: group.enable) {
                ForEach(group.controls) { control($0) }
                    .disabled(group.enable.map { !model.value($0).bool } ?? false)
            }
        }
    }

    private func disclosure<Content: View>(_ title: String, symbol: String, enable: String? = nil, @ViewBuilder content: @escaping () -> Content) -> some View {
        let isExpanded = Binding(
            get: { expanded.split(separator: ",").contains(Substring(title)) },
            set: { open in
                var titles = expanded.split(separator: ",").map(String.init).filter { $0 != title }
                if open { titles.append(title) }
                expanded = titles.joined(separator: ",")
            })
        return DisclosureGroup(isExpanded: isExpanded) {
            content()
        } label: {
            HStack {
                Label(title, systemImage: symbol)
                Spacer()
                if let enable {
                    Toggle(title, isOn: model.toggle(enable)).labelsHidden().toggleStyle(.switch).controlSize(.mini)
                }
            }
            // As in Photos, clicking the title toggles the group, not only the chevron.
            .contentShape(Rectangle())
            .onTapGesture { withAnimation { isExpanded.wrappedValue.toggle() } }
        }
    }

    @ViewBuilder private func developmentTime(stock: Stock?, path: String, label: String) -> some View {
        if let stock, stock.times.count > 1 {
            Picker(label, selection: Binding(get: { model.value(path).double }, set: { model.set(path, $0 == 0 ? .null : .number($0)) })) {
                Text("Profile default").tag(0.0)
                ForEach(stock.times, id: \.self) { Text("\($0.formatted()) min").tag($0) }
            }
        }
    }
}
