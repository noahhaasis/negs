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

/// Prepare the photograph, develop the film, then finish the scan.
enum InspectorTab: String, CaseIterable {
    case prep = "Prep", lab = "Lab", output = "Output"
}

let prepToneControls: [Adjustment] = [
    .slider("Highlights", "prep.highlights", -180...100, 1),
    .slider("Shadows", "prep.shadows", -100...100, 1),
    .slider("Contrast", "prep.contrast", -100...100, 1),
    .slider("Lift", "prep.lift", -100...100, 1),
    .slider("Gain", "prep.gain", -100...100, 1)
]
let prepColorControls: [Adjustment] = [
    .slider("Color Boost", "prep.color_boost", -100...100, 1),
    .slider("Saturation", "prep.saturation", -100...100, 1)
]
let prepDetailControls: [Adjustment] = [
    .slider("Midtone Detail", "prep.midtone_detail", -100...100, 1)
]

struct AdjustmentGroup: Identifiable {
    let title: String
    let symbol: String
    let section: InspectorSection
    /// Switches the whole effect on and off from the group's header.
    var enable: String?
    /// How many of the leading controls the Lab shows without Show all
    /// controls; a group with none shows only then (or once it's changed).
    var basic = 0
    let controls: [Adjustment]
    var id: String { title }
}

let adjustmentGroups: [AdjustmentGroup] = [
    .init(title: "Lens Diffusion", symbol: "camera.filters", section: .camera, enable: "camera.diffusion_filter.active", basic: 2, controls: [
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
    .init(title: "Film Curves", symbol: "chart.xyaxis.line", section: .film, enable: "film_render.density_curves_morph.active", basic: 1, controls: [
        .slider("Gamma", "film_render.density_curves_morph.gamma_factor", 0.5...2, 0.01),
        .slider("Fast gamma", "film_render.density_curves_morph.gamma_factor_fast", 0.5...2, 0.01),
        .slider("Slow gamma", "film_render.density_curves_morph.gamma_factor_slow", 0.5...2, 0.01),
        .slider("Red gamma", "film_render.density_curves_morph.gamma_factor_red", 0.5...2, 0.01),
        .slider("Green gamma", "film_render.density_curves_morph.gamma_factor_green", 0.5...2, 0.01),
        .slider("Blue gamma", "film_render.density_curves_morph.gamma_factor_blue", 0.5...2, 0.01),
        .slider("Developer exhaustion", "film_render.density_curves_morph.developer_exhaustion", 0...1, 0.01)
    ]),
    .init(title: "Grain", symbol: "circle.dotted", section: .film, enable: "film_render.grain.active", basic: 3, controls: [
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
    .init(title: "Halation", symbol: "sun.haze", section: .film, enable: "film_render.halation.active", basic: 1, controls: [
        .slider("Amount", "film_render.halation.halation_amount", 0...3, 0.01),
        .slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1...5, 0.01),
        .slider("Scatter amount", "film_render.halation.scatter_amount", 0...3, 0.01),
        .slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1...5, 0.01),
        .slider("Bounces", "film_render.halation.halation_n_bounces", 1...5, 1),
        .slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0...1, 0.01)
    ]),
    .init(title: "DIR Couplers", symbol: "circle.hexagongrid", section: .film, enable: "film_render.dir_couplers.active", basic: 1, controls: [
        .slider("Amount", "film_render.dir_couplers.amount", 0...2, 0.01),
        .slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0...100, 0.1),
        .slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0...400, 1),
        .slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0...1, 0.01)
    ]),
    .init(title: "Film Base", symbol: "square.stack.3d.down.right", section: .film, enable: "film_render.base.active", basic: 1, controls: [
        .slider("Density", "film_render.base.scale", 0...2, 0.01),
        .slider("Tilt", "film_render.base.tilt", -1...1, 0.01),
        .slider("Cyan", "film_render.base.cyan", 0...2, 0.01),
        .slider("Magenta", "film_render.base.magenta", 0...2, 0.01),
        .slider("Yellow", "film_render.base.yellow", 0...2, 0.01)
    ]),
    .init(title: "Enlarger", symbol: "light.overhead.left", section: .print, basic: 4, controls: [
        .slider("Print exposure", "enlarger.print_exposure", 0.1...5, 0.01),
        .slider("Magenta shift", "enlarger.m_filter_shift", -50...50, 0.1),
        .slider("Yellow shift", "enlarger.y_filter_shift", -50...50, 0.1),
        .balancePrint("Balance Print", "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base."),
        .toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
        .slider("Preflash exposure", "enlarger.preflash_exposure", 0...0.5, 0.01),
        .slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50...50, 0.1),
        .slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50...50, 0.1)
    ]),
    .init(title: "Print Curves", symbol: "chart.xyaxis.line", section: .print, enable: "print_render.density_curves_morph.active", basic: 1, controls: [
        .slider("Gamma", "print_render.density_curves_morph.gamma_factor", 0.5...2, 0.01),
        .slider("Fast gamma", "print_render.density_curves_morph.gamma_factor_fast", 0.5...2, 0.01),
        .slider("Slow gamma", "print_render.density_curves_morph.gamma_factor_slow", 0.5...2, 0.01),
        .slider("Red gamma", "print_render.density_curves_morph.gamma_factor_red", 0.5...2, 0.01),
        .slider("Green gamma", "print_render.density_curves_morph.gamma_factor_green", 0.5...2, 0.01),
        .slider("Blue gamma", "print_render.density_curves_morph.gamma_factor_blue", 0.5...2, 0.01),
        .slider("Developer exhaustion", "print_render.density_curves_morph.developer_exhaustion", 0...1, 0.01)
    ]),
    .init(title: "Print Base", symbol: "square.stack.3d.down.right", section: .print, enable: "print_render.base.active", basic: 1, controls: [
        .slider("Density", "print_render.base.scale", 0...2, 0.01),
        .slider("Cyan", "print_render.base.cyan", 0...2, 0.01),
        .slider("Magenta", "print_render.base.magenta", 0...2, 0.01),
        .slider("Yellow", "print_render.base.yellow", 0...2, 0.01)
    ]),
    .init(title: "Glare", symbol: "light.max", section: .print, enable: "print_render.glare.active", basic: 1, controls: [
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

// MARK: Rows for the adjustments

struct AdjustmentControl: View {
    let model: EditorModel
    let adjustment: Adjustment

    var body: some View {
        switch adjustment {
        case .toggle(let title, let path):
            TickRow(title: title, isOn: model.toggle(path), changed: model.isModified(path), help: ControlHelp.text(path))
                .resettable(model, path)
        case .choice(let title, let path, let options):
            ChoiceRow(title: title, items: options.map { .option(optionTitle($0), tag: $0) }, selection: model.value(path).string,
                      changed: model.isModified(path), help: ControlHelp.text(path)) { model.set(path, .string($0)) }
                .resettable(model, path)
        case .slider(let title, let path, let range, let step):
            SliderRow(model: model, title: title, path: path, range: range, step: step)
        case .stockSlider(let title, let path, let index, let range, let step, let bwTitle):
            if !model.filmIsBW || bwTitle != nil {
                LabeledSlider(title: model.filmIsBW ? bwTitle ?? title : title,
                              value: model.stockNumber(path, index, range: range, step: step), range: range, step: step,
                              defaultValue: model.stockDefault(path, index), modified: model.isStockModified(path, index),
                              reset: { model.resetStock(path, index) },
                              editing: { $0 ? model.beginInteraction() : model.endInteraction() },
                              help: ControlHelp.text(path))
            }
        case .balancePrint(let title, let help):
            HStack {
                Spacer()
                Button { model.balancePrint() } label: {
                    if model.balancingPrint {
                        VStack(spacing: 2) {
                            Text("Balancing").italic()
                            RunningLine().frame(width: 44)
                        }
                    } else {
                        Text(title)
                    }
                }
                .buttonStyle(OutlinedButtonStyle())
                .disabled(!model.canBalancePrint)
                .inkFocus { if model.canBalancePrint { model.balancePrint() } }
                .help(help)
            }
            .padding(.horizontal, Theme.Size.gutter)
            .padding(.vertical, 4)
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
                      editing: { $0 ? model.beginInteraction() : model.endInteraction() },
                      help: ControlHelp.text(path))
    }
}

/// Rows a choice reveals: indented under it, on a 1 pt `track` rule (§4.9).
struct RevealedRows<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) { content }
            .padding(.leading, 12)
            .overlay(alignment: .leading) {
                Rectangle().fill(Theme.track).frame(width: 1).padding(.leading, Theme.Size.gutter).padding(.vertical, 4)
            }
    }
}

/// The film frame as a stills format, which writes the frame's long edge in
/// mm; Custom shows the slider. A value no format has reads as Custom, so
/// presets and older edits need no setting of their own.
struct FilmFormatPicker: View {
    let model: EditorModel
    static let path = "camera.film_format_mm"
    static let range: ClosedRange<Double> = 4...120
    /// Long edges of the image area. 135 stays at the engine's 35 (not the
    /// gate's 36) so existing edits render as before; 6×4.5 and 6×6 share 56.
    static let formats: [(name: String, mm: Double)] = [
        ("110", 17), ("Half frame", 24), ("35 mm (135)", 35),
        ("6×4.5 / 6×6", 56), ("6×7", 70), ("6×9", 84), ("4×5", 120),
    ]
    static func format(mm: Double) -> String? { formats.first { abs($0.mm - mm) < 0.05 }?.name }
    private static let custom = "Custom"

    var body: some View {
        let mm = model.value(Self.path).double
        let custom = model.customFilmFormat || Self.format(mm: mm) == nil
        let items: [ChoiceItem] = Self.formats.map { .option($0.name, tag: $0.name) } + [.divider, .option("Custom…", tag: Self.custom)]
        ChoiceRow(title: "Film format", items: items, selection: custom ? Self.custom : Self.format(mm: mm) ?? Self.custom,
                  changed: model.isModified(Self.path), help: ControlHelp.text(Self.path)) { name in
            if let format = Self.formats.first(where: { $0.name == name }) {
                model.customFilmFormat = false
                model.set(Self.path, .number(format.mm))
            } else {
                model.customFilmFormat = true
            }
        }
        if custom {
            RevealedRows { SliderRow(model: model, title: "Long edge (mm)", path: Self.path, range: Self.range, step: 0.1) }
        }
    }
}

/// The short running line that says work is under way: a 1 pt `track` line
/// with a yellow segment crossing it every 1.1 s (§4.14).
struct RunningLine: View {
    @State private var phase = false
    /// With Reduce Motion the segment stands still, a third of the way along.
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            ZStack(alignment: .leading) {
                Rectangle().fill(Theme.track).frame(height: 1)
                Rectangle().fill(Theme.accent).frame(width: width * 0.3, height: 1)
                    .offset(x: reduceMotion ? width * 0.3 : phase ? width : -width * 0.3)
            }
            .clipped()
        }
        .frame(height: 1)
        .onAppear { if !reduceMotion { withAnimation(.linear(duration: 1.1).repeatForever(autoreverses: false)) { phase = true } } }
        .accessibilityHidden(true)
    }
}

/// Image Size, in Output: the photograph's resolution before the film
/// develops it. Reads only its own parameters, the crop and the source's
/// size, so other edits don't redraw it.
struct ImageSizeSection: View {
    let model: EditorModel
    @State private var fieldError: String?

    var body: some View {
        let params: JSONValue = .object(["io": .object(["photo_long_side_px": model.value(ImageSize.targetPath)])])
        let mode = ImageSize.mode(of: params)
        let source = model.sourceSize
        let working = source == .zero ? CGSize.zero : ImageSize.working(source: source, params: params, crop: model.crop)
        let photograph = model.crop.size(of: working)
        InkSection(title: "Image Size", edited: model.isModified(ImageSize.targetPath)) {
            ChoiceRow(title: "Size", items: [.option("Original", tag: ImageSize.Mode.original.rawValue), .option("Longest side", tag: ImageSize.Mode.longestSide.rawValue)],
                      selection: mode.rawValue, changed: mode != .original,
                      help: "Original develops the photograph’s own pixels. Longest side enlarges or reduces it first, so the cropped photograph has that many pixels on its longest side.") {
                fieldError = nil
                model.chooseImageSize(ImageSize.Mode(rawValue: $0) ?? .original)
            }
            if mode == .longestSide {
                RevealedRows {
                    NumberRow(title: "Longest side", unit: "px", error: fieldError != nil,
                              help: "The cropped photograph’s longest side, before any border. Exports can still be reduced in the save panel.") {
                        PixelField(title: "Longest side", value: ImageSize.target(of: params) ?? 0, error: fieldError != nil) { pixels in
                            fieldError = model.setLongestSide(pixels)
                            return fieldError == nil
                        }
                    }
                    if let fieldError { HintText(fieldError, warning: true) }
                }
            }
            // Verbatim, so pixel counts read like the status bar's, without grouping.
            ReadoutRow(title: "Photograph", value: photograph == .zero ? "—" : "\(Int(photograph.width)) × \(Int(photograph.height)) · \(ImageSize.megapixels(photograph)) MP",
                       help: "The exported photograph, cropped, before rotation, the border, and any export size.")
            if ImageSize.exceedsLimit(working: working, source: source) {
                HintText(ImageSize.limitMessage(working: working), warning: true)
            } else if model.previewScale < 1, model.image != nil {
                HintText("The preview shows \(EditorModel.percent(model.previewScale)) of these pixels. Exports develop all of them; inspect grain in an exported file.")
            }
            HintText("Resizes before film simulation, including halation and grain.")
        }
    }
}

/// A number field's row: the label, then the field on a rule and its unit
/// in italic `dim` (§4.9).
struct NumberRow<Field: View>: View {
    let title: String
    let unit: String
    var error = false
    var help: String? = nil
    @ViewBuilder let field: Field

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            RowLabel(title: title)
            Spacer(minLength: 8)
            field.frame(minWidth: 78, maxWidth: 96).frame(height: 20)
            Text(unit).font(Theme.serif(13, italic: true)).foregroundStyle(Theme.dim)
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row)
        .tooltip(help)
    }
}

/// A whole number of pixels, committed on Return or when focus leaves. An
/// invalid value stays in the field until corrected; Escape restores the
/// current one. AppKit, like `ValueField`, so the inspector column can
/// focus it. The serif on a rule: `track`, `paper` while typing, `warn`
/// when the number can't be used.
struct PixelField: NSViewRepresentable {
    let title: String
    let value: Int
    var error = false
    /// Applies a typed value; false keeps the typing for correction.
    let commit: (Int) -> Bool

    func makeNSView(context: Context) -> RuledField {
        let field = RuledField()
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.font = Theme.serifFont(14.5) as NSFont
        field.textColor = Theme.NS.paper
        field.alignment = .right
        field.setAccessibilityLabel(title)
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: RuledField, context: Context) {
        context.coordinator.parent = self
        field.error = error
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

/// A borderless text field on a 1 pt rule: `track`, `paper` while it has the
/// keys, `warn` while its text can't be used; caret and selection never blue.
final class RuledField: NSTextField {
    var error = false { didSet { if error != oldValue { needsDisplay = true } } }
    private var editing = false { didSet { needsDisplay = true } }

    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became {
            editing = true
            if let editor = currentEditor() as? NSTextView { InkTextField.style(editor) }
        }
        return became
    }
    override func textDidEndEditing(_ notification: Notification) {
        super.textDidEndEditing(notification)
        editing = false
    }
    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        (error ? Theme.NS.warn : editing ? Theme.NS.paper : Theme.NS.track).setFill()
        NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1).fill()
    }
}

/// A colour: a 14 pt swatch in a `track` ring, then its name; a click opens
/// the macOS Colours panel (§4.9).
struct ColourWellRow: View {
    let title: String
    let color: Binding<CGColor>
    var help: String? = nil
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 8) {
            RowLabel(title: title, bright: hovered)
            Spacer(minLength: 8)
            Circle().fill(Color(cgColor: color.wrappedValue)).frame(width: 14, height: 14)
                .overlay(Circle().strokeBorder(Theme.track, lineWidth: 1))
            Text(Self.name(color.wrappedValue)).font(Theme.serif(14.5)).foregroundStyle(Theme.paper)
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row)
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture(perform: open)
        .inkFocus(activate: open)
        .tooltip(help)
        .accessibilityElement()
        .accessibilityLabel(title)
        .accessibilityValue(Self.name(color.wrappedValue))
        .accessibilityAddTraits(.isButton)
        .accessibilityAction(.default, open)
    }

    private func open() { ColourPanelLink.shared.open(color.wrappedValue, title: title) { color.wrappedValue = $0 } }

    /// White, Black, or the colour's hex code.
    static func name(_ color: CGColor) -> String {
        let hex = BorderColor(color).hex.uppercased()
        switch hex.trimmingCharacters(in: CharacterSet(charactersIn: "#")) {
        case "FFFFFF": return "White"
        case "000000": return "Black"
        default: return hex.hasPrefix("#") ? hex : "#" + hex
        }
    }
}

/// Points the shared Colours panel at one colour well at a time.
@MainActor final class ColourPanelLink: NSObject {
    static let shared = ColourPanelLink()
    private var set: ((CGColor) -> Void)?

    func open(_ color: CGColor, title: String, set: @escaping (CGColor) -> Void) {
        let panel = NSColorPanel.shared
        self.set = nil
        panel.showsAlpha = false
        panel.color = NSColor(cgColor: color) ?? .white
        panel.setTarget(self)
        panel.setAction(#selector(changed(_:)))
        panel.title = title
        self.set = set
        panel.orderFront(nil)
    }
    @objc private func changed(_ panel: NSColorPanel) { set?(panel.color.cgColor) }
}

extension View {
    /// A native tooltip, or none.
    @ViewBuilder func tooltip(_ text: String?) -> some View {
        if let text { help(text) } else { self }
    }
}

private extension View {
    func resettable(_ model: EditorModel, _ path: String) -> some View {
        contextMenu {
            Button("Reset to Default") { model.resetValue(path) }.disabled(!model.isModified(path))
        }
    }
}

// MARK: What the Lab shows

/// Which controls the Lab keeps back (round 11, B). Each module shows its
/// first `basic` controls; the rest wait behind "N more", as do the expert
/// modules of each section. View › Show All Controls shows everything, and
/// a control that is off its default always shows.
struct InspectorReveal: Equatable {
    /// View › Show All Controls.
    var all: Bool
    /// The modules (by title) and sections ("section:Print") whose "N more" was clicked.
    var revealed: Set<String>

    init(all: Bool, revealed: Set<String> = []) { self.all = all; self.revealed = revealed }
    /// From the comma-separated default.
    init(all: Bool, stored: String) { self.init(all: all, revealed: Set(stored.split(separator: ",").map(String.init))) }

    static func sectionKey(_ section: InspectorSection) -> String { "section:" + section.rawValue }
    /// The default's new value once `key`'s "N more" is clicked.
    func revealing(_ key: String) -> String { revealed.union([key]).sorted().joined(separator: ",") }

    func shows(section: InspectorSection) -> Bool { all || revealed.contains(Self.sectionKey(section)) }
    /// A module shows when it has basics, its section's extras are revealed, or something in it changed.
    func shows(_ group: AdjustmentGroup, edits: Int) -> Bool { group.basic > 0 || shows(section: group.section) || edits > 0 }
    /// A module's controls: its basics, then the rest once revealed, or any changed.
    func controls(_ group: AdjustmentGroup, isModified: (Adjustment) -> Bool) -> [Adjustment] {
        let every = all || revealed.contains(group.title)
        return group.controls.enumerated().filter { every || $0.offset < group.basic || isModified($0.element) }.map(\.element)
    }
    /// "2 more: Scanner, Color Management", or nil when nothing is kept back.
    static func moreText(_ hidden: [String]) -> String? {
        hidden.isEmpty ? nil : "\(hidden.count) more: \(hidden.joined(separator: ", "))"
    }
}

extension AdjustmentInspector {
    /// View › Show All Controls. Returns the new state.
    @discardableResult static func toggleAllControls(in defaults: UserDefaults = .standard) -> Bool {
        let on = !defaults.bool(forKey: allControlsKey)
        defaults.set(on, forKey: allControlsKey)
        return on
    }
}

// MARK: Counting edits

/// What each tab holds, for the dots and counts on the tabs, sections and
/// modules (§4.8): every control that differs from its default.
@MainActor enum InspectorEdits {
    static func isModified(_ adjustment: Adjustment, _ model: EditorModel) -> Bool {
        switch adjustment {
        case .slider(_, let path, _, _), .toggle(_, let path), .choice(_, let path, _): model.isModified(path)
        case .stockSlider(_, let path, let index, _, _, _): model.isStockModified(path, index)
        case .balancePrint: false
        }
    }
    static func count(_ group: AdjustmentGroup, _ model: EditorModel) -> Int {
        (group.enable.map(model.isModified) == true ? 1 : 0) + group.controls.filter { isModified($0, model) }.count
    }
    static func count(_ section: InspectorSection, _ model: EditorModel) -> Int {
        var count = adjustmentGroups.filter { $0.section == section }.map { Self.count($0, model) }.reduce(0, +)
        switch section {
        case .camera: count += ["camera.exposure_compensation_ev", FilmFormatPicker.path, "camera.lens_blur_um"].filter(model.isModified).count
        case .film: count += model.isModified(filmDevelopmentPath) ? 1 : 0
        case .print: count += (scanFilmModified(model) ? 1 : 0) + (model.isModified(printDevelopmentPath) ? 1 : 0)
        case .scan: count += outputCount(model)
        }
        return count
    }
    static let filmDevelopmentPath = "film_render.development_time", printDevelopmentPath = "print_render.development_time"
    /// "Profile default", then each time the stock was measured at.
    static func developmentItems(_ stock: Stock) -> [ChoiceItem] {
        [.option("Profile default", tag: "")] + stock.times.map { .option("\($0.formatted()) min", tag: "\($0)") }
    }
    /// The menu tag for a development time: empty for the profile's default (null or 0).
    static func developmentTag(_ value: JSONValue) -> String {
        let minutes = value.double
        return value == .null || minutes == 0 ? "" : "\(minutes)"
    }
    static func scanFilmModified(_ model: EditorModel) -> Bool {
        model.value("io.scan_film").bool != (model.films.first { $0.id == model.film }?.positive ?? false)
    }
    static func outputCount(_ model: EditorModel) -> Int {
        (model.previewSpace != SaveSettings.colorSpaces[0] ? 1 : 0) + (model.isModified("io.output_cctf_encoding") ? 1 : 0)
    }

    static let whiteBalancePaths = ["io.raw_white_balance.mode", "io.raw_white_balance.temperature", "io.raw_white_balance.tint"]
    static let inputPaths = ["io.input_interpretation.mode", "io.input_interpretation.color_space", "io.input_interpretation.linear",
                             "io.input_color_space", "io.input_cctf_decoding"]
    static func brightnessCount(_ model: EditorModel) -> Int {
        (model.matchesBrightness != model.autoPlaceNewImages ? 1 : 0) + (model.exposureOffsetModified ? 1 : 0)
            + ["camera.auto_exposure", "camera.auto_exposure_method"].filter(model.isModified).count
    }
    static func prepCount(_ model: EditorModel) -> Int {
        var count = brightnessCount(model)
        count += (prepToneControls + prepColorControls + prepDetailControls).filter { isModified($0, model) }.count
        if model.isRaw {
            count += whiteBalancePaths.filter(model.isModified).count
            if model.lensCorrectionSupported, model.isModified(EditorModel.lensCorrectionPath) { count += 1 }
        } else {
            count += inputPaths.filter(model.isModified).count
        }
        return count
    }
    static func labCount(_ model: EditorModel) -> Int {
        InspectorSection.allCases.map { count($0, model) }.reduce(0, +)
    }
    static func outputTabCount(_ model: EditorModel) -> Int {
        (model.isModified(ImageSize.targetPath) ? 1 : 0) + (model.border != BorderSettings() ? 1 : 0)
    }
    static func count(_ tab: InspectorTab, _ model: EditorModel) -> Int {
        switch tab {
        case .prep: prepCount(model)
        case .lab: labCount(model)
        case .output: outputTabCount(model)
        }
    }
}

// MARK: The panel

/// The top of the inspector column: Adjustments · Presets as words, then
/// the edit count and Reset, or New and Import… beside the presets (§4.8).
struct PaneHeader: View {
    let model: EditorModel

    var body: some View {
        let presets = model.inspectorPane == .presets
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            paneWord("Adjustments", pane: .adjustments, help: "Show the adjustments (⌥⌘I)")
            paneWord("Presets", pane: .presets, help: "Show the presets (⌥⌘P)")
            Spacer(minLength: 0)
            if presets {
                WordButton(title: "New", help: "Save the film, paper and Lab settings as a preset (⇧⌘S)", action: model.newPreset)
                    .disabled(!model.canChangePresets)
                WordButton(title: "Import…", help: "Import presets (⇧⌘O)", action: model.importPresetsPanel)
                    .disabled(!model.canChangePresets)
            } else if model.source != nil {
                let edits = InspectorTab.allCases.map { InspectorEdits.count($0, model) }.reduce(0, +)
                if edits > 0 {
                    (Text("\(edits)").font(Theme.serif(14, weight: 700, italic: true)).foregroundColor(Theme.paper)
                        + Text(edits == 1 ? " edit" : " edits").font(Theme.serif(14, italic: true)).foregroundColor(Theme.dim))
                        .accessibilityLabel("\(edits) \(edits == 1 ? "edit" : "edits")")
                }
                WordButton(title: "Reset",
                           help: "Restore defaults on all tabs while keeping the film, paper, rotation, crop, border, and Image Size. You can undo this.",
                           action: model.reset)
                    .disabled(!model.hasCatalog || model.cropping)
            }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .padding(.top, 13)
        .frame(height: Theme.Size.paneHeader, alignment: .top)
    }

    private func paneWord(_ title: String, pane: InspectorPane, help: String) -> some View {
        let chosen = model.inspectorPane == pane
        return Button { model.inspectorPane = pane } label: {
            Text(title).font(Theme.ui(12, weight: .medium)).foregroundStyle(chosen ? Theme.paper : Theme.dim)
        }
        .buttonStyle(InkPressStyle())
        .inkFocus { model.inspectorPane = pane }
        .help(help)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

/// Prep, Lab and Output in type; the chosen one underlined in pencil, and a
/// tab holding edits marked with the dot and its count (§4.8).
struct InspectorTabs: View {
    let model: EditorModel
    @Binding var tab: InspectorTab

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 24) {
            ForEach(InspectorTab.allCases, id: \.self) { item in
                TabWord(title: item.rawValue, chosen: tab == item, count: InspectorEdits.count(item, model)) { tab = item }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.Size.gutter)
        .padding(.top, 8)
        .frame(height: Theme.Size.tabRow, alignment: .top)
        .overlay(alignment: .top) { InkRule() }
        .overlay(alignment: .bottom) { InkRule() }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Inspector tabs")
    }
}

/// A tab in type: Newsreader 21, the chosen one 700 in `paper` and
/// underlined in pencil, with the dot and count of its edits (§4.8).
struct TabWord: View {
    let title: String
    let chosen: Bool
    var count = 0
    let choose: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: choose) {
            HStack(alignment: .top, spacing: 0) {
                Text(title).font(Theme.serif(21, weight: chosen ? 700 : 500))
                    .foregroundStyle(chosen ? Theme.paper : hovered ? Theme.label : Theme.dim)
                    .padding(.bottom, 5)
                    .pencilUnderline(chosen, height: 6, offset: 0)
                if count > 0 {
                    EditDot().padding(.leading, 5).padding(.trailing, 3).padding(.top, 6)
                    Text("\(count)").font(Theme.ui(10.5, weight: .semibold)).foregroundStyle(Theme.label).padding(.top, 1)
                }
            }
        }
        .buttonStyle(InkPressStyle())
        .onHover { hovered = $0 }
        .inkFocus(activate: choose)
        .accessibilityLabel(count > 0 ? "\(title), \(count) \(count == 1 ? "edit" : "edits")" : title)
        .accessibilityAddTraits(chosen ? [.isSelected, .isButton] : .isButton)
    }
}

struct AdjustmentInspector: View {
    let model: EditorModel
    @AppStorage("expandedAdjustments") private var expanded = "Enlarger"
    @AppStorage("inspectorTab") private var tab = InspectorTab.prep
    /// View › Show All Controls: every control, as the Lab's Advanced
    /// controls switch did. Off, each module shows the few most photographs
    /// need, plus any setting that is off its default.
    @AppStorage(Self.allControlsKey) private var allControls = false
    /// The modules and sections whose "N more" were opened.
    @AppStorage(Self.moreKey) private var moreShown = ""
    static let allControlsKey = "labAllControls", moreKey = "moreAdjustments"

    var body: some View {
        // The histogram, film and paper, and the tabs stay put above the
        // tab's scrolling sections, so every tab opens with the stocks in view.
        VStack(spacing: 0) {
            ScopePanelView(histogram: model.histogram, waveform: model.waveform)
                .padding(.horizontal, Theme.Size.gutter)
                .padding(.bottom, 10)
            InspectorTabs(model: model, tab: $tab)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch tab {
                    case .prep: prep
                    case .lab: lab
                    case .output: output
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.automatic)
            if tab == .output { ExportFoot(model: model) }
        }
        .disabled(!model.hasCatalog)
    }

    // MARK: Prep

    @ViewBuilder private var prep: some View {
        if model.isRaw {
            InkSection(title: nil, edited: false) {
                control(.choice("White balance", "io.raw_white_balance.mode", ["as_shot", "daylight", "tungsten", "custom"]))
                if model.value("io.raw_white_balance.mode").string == "custom" {
                    RevealedRows {
                        control(.slider("Temperature (K)", "io.raw_white_balance.temperature", 1667...25000, 1))
                        control(.slider("Tint", "io.raw_white_balance.tint", 0.01...4, 0.01))
                    }
                }
            }
        }
        InkSection(title: "Brightness", edited: InspectorEdits.brightnessCount(model) > 0) {
            TickRow(title: model.isRaw ? "Match camera brightness" : "Match original brightness",
                    isOn: Binding(get: { model.matchesBrightness }, set: model.setMatchesBrightness),
                    changed: model.matchesBrightness != model.autoPlaceNewImages,
                    help: model.isRaw
                        ? "Develops the RAW about as bright as the camera’s own preview, and keeps it there when you change film, paper or a preset. Exposure adds to it."
                        : "Develops the photograph about as bright as the file itself, and keeps it there when you change film, paper or a preset. Exposure adds to it.")
            LabeledSlider(title: "Exposure (EV)", value: model.exposureOffset, range: model.exposureOffsetRange, step: 0.05,
                          defaultValue: 0, modified: model.exposureOffsetModified, reset: model.resetExposureOffset,
                          editing: { $0 ? model.beginInteraction() : model.endInteraction() },
                          help: model.matchesBrightness
                              ? "Brighter or darker than the match, in stops. It stays when the match changes with the film, paper or a preset."
                              : "Brightens or darkens the scene before the film, in stops. Unlike Negative exposure, the print doesn’t make up for it.")
            if let note = model.brightnessMatchNote { HintText(note) }
            // While matched the meter only sets where the highlight limit
            // starts, so it shows when matching is off, or when changed.
            if !model.brightnessMatchActive || model.isModified("camera.auto_exposure") || model.isModified("camera.auto_exposure_method") {
                TickRow(title: "Auto exposure", isOn: model.toggle("camera.auto_exposure"), changed: model.isModified("camera.auto_exposure"),
                        help: "Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it.")
                if model.value("camera.auto_exposure").bool {
                    RevealedRows {
                        control(.choice("Metering", "camera.auto_exposure_method", ["average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"]))
                    }
                }
            }
        }
        controlSection("Tone", prepToneControls)
        controlSection("Color", prepColorControls)
        controlSection("Detail", prepDetailControls)
        // How the file is read; set once per photograph.
        if model.isRaw {
            if model.lensCorrectionSupported {
                InkSection(title: "Lens", edited: model.isModified(EditorModel.lensCorrectionPath)) {
                    TickRow(title: "Lens corrections", isOn: model.toggle(EditorModel.lensCorrectionPath),
                            changed: model.isModified(EditorModel.lensCorrectionPath),
                            help: "Corrects distortion and vignetting with the data the camera recorded for its lens.")
                }
            }
        } else {
            InkSection(title: nil) { inputColour }
        }
    }

    private func controlSection(_ title: String, _ controls: [Adjustment]) -> some View {
        InkSection(title: title, edited: controls.contains { InspectorEdits.isModified($0, model) }) {
            ForEach(controls) { control($0) }
        }
    }

    /// How long the film or paper develops, for stocks measured at several
    /// times. The stocks themselves are chosen from the label under the photograph.
    @ViewBuilder private func developmentTime(stock: Stock?, path: String, label: String, help: String) -> some View {
        if let stock, stock.times.count > 1 {
            let current = model.value(path)
            ChoiceRow(title: label, items: InspectorEdits.developmentItems(stock), selection: InspectorEdits.developmentTag(current),
                      changed: model.isModified(path), help: help) { tag in
                model.set(path, Double(tag).map { .number($0) } ?? .null)
            }
        }
    }

    // MARK: Lab

    @ViewBuilder private var lab: some View {
        let scanning = model.value("io.scan_film").bool
        InkSection(title: "Camera", edited: InspectorEdits.count(.camera, model) > 0) {
            control(.slider("Negative exposure (EV)", "camera.exposure_compensation_ev", -5...5, 0.1))
            FilmFormatPicker(model: model)
            let lensBlur = shows("camera.lens_blur_um", section: .camera)
            if lensBlur { control(.slider("Lens blur (µm)", "camera.lens_blur_um", 0...100, 0.1)) }
            modules(.camera, ruledFirst: true)
            moreLine(.camera, loose: lensBlur ? [] : ["Lens blur"])
        }
        let filmStock = model.films.first { $0.id == model.film }
        InkSection(title: "Film", edited: InspectorEdits.count(.film, model) > 0) {
            developmentTime(stock: filmStock, path: InspectorEdits.filmDevelopmentPath, label: "Development",
                            help: "How long the film develops, from the times the stock was measured at. Longer gives more density and contrast.")
            modules(.film, ruledFirst: (filmStock?.times.count ?? 0) > 1)
            moreLine(.film)
        }
        let scanSwitch = scanning || shows(section: .print)
        let paperStock = scanning ? nil : model.papers.first { $0.id == model.paper }
        let printTime = (paperStock?.times.count ?? 0) > 1
        InkSection(title: "Print", edited: InspectorEdits.count(.print, model) > 0, skipped: scanning ? "skipped" : nil) {
            developmentTime(stock: paperStock, path: InspectorEdits.printDevelopmentPath, label: "Print development",
                            help: "How long the paper develops, from the times it was measured at. Longer gives deeper blacks and more contrast.")
            // Here rather than beside the stocks: few photographs want it.
            if scanSwitch {
                TickRow(title: "Scan film directly", isOn: model.toggle("io.scan_film"), changed: InspectorEdits.scanFilmModified(model),
                        help: "Skip printing and scan the developed film, as for slides.")
            }
            if scanning { HintText("Printing and the paper are skipped while scanning film directly.") }
            modules(.print, ruledFirst: scanSwitch || printTime, skipped: scanning)
            if !scanning { moreLine(.print, loose: scanSwitch ? [] : ["Scan film directly"]) }
        }
        let outputChanged = InspectorEdits.outputCount(model) > 0
        let outputShown = outputChanged || shows(section: .scan)
        InkSection(title: "Scan & Color", edited: InspectorEdits.count(.scan, model) > 0) {
            modules(.scan, ruledFirst: false)
            if outputShown {
                module("Output", help: ControlHelp.group("Output"), summary: model.previewSpace, edited: outputChanged,
                       ruled: adjustmentGroups.contains { $0.section == .scan && shows($0) }) {
                    ChoiceRow(title: "Colour space", items: SaveSettings.colorSpaces.map { .option($0, tag: $0) }, selection: model.previewSpace,
                              changed: model.previewSpace != SaveSettings.colorSpaces[0],
                              help: "The preview develops in this colour space, so it shows the colours an export in it can hold. Save and Export use it next, and switch it when you pick another.",
                              choose: model.choosePreviewSpace)
                    TickRow(title: "Encode with the colour space’s curve", isOn: model.toggle("io.output_cctf_encoding"),
                            changed: model.isModified("io.output_cctf_encoding"), help: "Off writes linear values, for further processing.")
                }
            }
            moreLine(.scan, loose: outputShown ? [] : ["Output"])
        }
    }

    // MARK: "N more"

    private var reveal: InspectorReveal { InspectorReveal(all: allControls, stored: moreShown) }
    private func revealMore(_ key: String) { moreShown = reveal.revealing(key) }
    /// Whether a section's extra controls (the expert modules, loose rows) show.
    private func shows(section: InspectorSection) -> Bool { reveal.shows(section: section) }
    /// Whether a section's loose control shows: it's revealed, or changed.
    private func shows(_ path: String, section: InspectorSection) -> Bool { shows(section: section) || model.isModified(path) }
    private func shows(_ group: AdjustmentGroup) -> Bool { reveal.shows(group, edits: InspectorEdits.count(group, model)) }
    private func shownControls(_ group: AdjustmentGroup) -> [Adjustment] {
        reveal.controls(group) { InspectorEdits.isModified($0, model) }
    }
    private func visibleCount(_ group: AdjustmentGroup) -> Int {
        group.controls.filter { if case .stockSlider(_, _, _, _, _, let bw) = $0 { return !model.filmIsBW || bw != nil }; return true }.count
    }

    /// "2 more: Scanner, Color Management…" at a section's end, for the
    /// modules and rows it keeps back (round 11, B).
    @ViewBuilder private func moreLine(_ section: InspectorSection, loose: [String] = []) -> some View {
        let hidden = adjustmentGroups.filter { $0.section == section && !shows($0) }.map(\.title) + loose
        if !shows(section: section), let text = InspectorReveal.moreText(hidden) {
            MoreLine(text: text) { revealMore(InspectorReveal.sectionKey(section)) }
                .padding(.top, 6)
        }
    }

    // MARK: Modules

    @ViewBuilder private func modules(_ section: InspectorSection, ruledFirst: Bool, skipped: Bool = false) -> some View {
        let groups = adjustmentGroups.filter { $0.section == section && shows($0) }
        ForEach(Array(groups.enumerated()), id: \.element.id) { offset, group in
            let shown = shownControls(group)
            let enabled = group.enable.map { model.toggle($0) }
            module(group.title, help: ControlHelp.group(group.title), enabled: enabled, summary: summary(group),
                   edited: InspectorEdits.count(group, model) > 0, skipped: skipped, ruled: offset > 0 || ruledFirst,
                   more: visibleCount(group) - shown.count, showMore: { revealMore(group.title) }) {
                if group.title == "Print Curves" { ToneGraphView(model: model).padding(.horizontal, Theme.Size.gutter).padding(.vertical, 6) }
                ForEach(shown) { control($0) }
            }
        }
    }

    private func module<Content: View>(_ title: String, help: String?, enabled: Binding<Bool>? = nil, summary: String?, edited: Bool,
                                       skipped: Bool = false, ruled: Bool = true, more: Int = 0, showMore: @escaping () -> Void = {},
                                       @ViewBuilder content: @escaping () -> Content) -> some View {
        let open = Binding(
            get: { expanded.split(separator: ",").contains(Substring(title)) },
            set: { open in
                var titles = expanded.split(separator: ",").map(String.init).filter { $0 != title }
                if open { titles.append(title) }
                expanded = titles.joined(separator: ",")
            })
        return InkModule(title: title, help: help, enabled: enabled, summary: summary, edited: edited, skipped: skipped, ruled: ruled,
                         open: open, more: more, showMore: showMore, content: content)
    }

    /// A folded module's summary: its first slider's value.
    private func summary(_ group: AdjustmentGroup) -> String? {
        for control in group.controls {
            switch control {
            case .slider(_, let path, _, let step):
                return model.value(path).double.formatted(.number.precision(.fractionLength(0...LabeledSlider.decimals(step))))
            case .stockSlider(_, let path, let index, let range, let step, _):
                return model.stockNumber(path, index, range: range, step: step).wrappedValue
                    .formatted(.number.precision(.fractionLength(0...LabeledSlider.decimals(step))))
            case .choice(_, let path, _):
                return optionTitle(model.value(path).string)
            default: continue
            }
        }
        return nil
    }

    /// How a non-RAW photograph's values are read: automatically from its
    /// profile and tags, as an assumed colour space, or (Manual) by the input
    /// colour space and curve, as Negs 0.8 read every file.
    @ViewBuilder private var inputColour: some View {
        let colour = model.inputColour
        let edited = InspectorEdits.inputPaths.contains(where: model.isModified)
        module("Input Colour", help: ControlHelp.group("Input Colour"), summary: colour == .null ? nil : colour["summary"].string, edited: edited, ruled: false) {
            let assumed = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020"]
            ChoiceRow(title: "Read as", items: [.option("Automatic", tag: "auto"), .header("Assume")] + assumed.map { .option($0, tag: "assume:" + $0) } + [.divider, .option("Manual", tag: "manual")],
                      selection: model.inputInterpretation.wrappedValue, changed: model.isModified("io.input_interpretation.mode"),
                      help: "Automatic reads the embedded profile or tags; untagged files are read as sRGB. Assume overrides them. Manual uses the colour space and curve below, as Negs 0.8 did.") {
                model.inputInterpretation.wrappedValue = $0
            }
            switch model.value("io.input_interpretation.mode").string {
            case "assume":
                RevealedRows {
                    TickRow(title: "Linear values", isOn: model.toggle("io.input_interpretation.linear"),
                            changed: model.isModified("io.input_interpretation.linear"), help: ControlHelp.text("io.input_interpretation.linear"))
                }
            case "manual":
                RevealedRows {
                    control(.choice("Colour space", "io.input_color_space", ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020", "ACES2065-1", "ACEScg"]))
                    TickRow(title: "Decode the curve", isOn: model.toggle("io.input_cctf_decoding"),
                            changed: model.isModified("io.input_cctf_decoding"), help: ControlHelp.text("io.input_cctf_decoding"))
                }
            default: EmptyView()
            }
            if colour != .null {
                ReadoutRow(title: "Reads", value: colour["summary"].string, italic: true)
                if colour["automatic"].string != colour["summary"].string { ReadoutRow(title: "Automatic", value: colour["automatic"].string, italic: true) }
                if !colour["warning"].string.isEmpty { HintText(colour["warning"].string, warning: true) }
            }
        }
    }

    // MARK: Output

    /// The size and frame of the finished photograph. Tone and colour edits
    /// live in Prep, before the film, where nothing has clipped yet.
    @ViewBuilder private var output: some View {
        ImageSizeSection(model: model)
        InkSection(title: "Border", edited: model.border != BorderSettings()) {
            TickRow(title: "Add border", isOn: model.border(\.enabled), changed: model.border.enabled,
                    help: "Frames the photograph in a mat or a film frame. The preview always shows it; Include in export decides whether files get it.")
            Group {
                let photo = model.photoSize == .zero ? nil : model.photoSize
                BorderStylePicker(selection: model.border(\.style), photo: photo, colors: (model.border.color, model.border.lineColor))
                    .padding(.horizontal, Theme.Size.gutter).padding(.vertical, 6)
                if let photo, !BorderStyle.film35.fits(photo) {
                    HintText("35mm needs a 3:2 or 3:4 photograph. This one is \(Self.ratio(photo)); crop it to 3:2 or 4:3 to use it.")
                }
                borderControls
                TickRow(title: "Extend to 3:4 for Instagram", isOn: model.border(\.instagram), changed: model.border.instagram != BorderSettings().instagram,
                        help: "Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post.")
                TickRow(title: "Include in export", isOn: model.border(\.export), changed: model.border.export != BorderSettings().export,
                        help: "Adds the border to exported photographs and saved previews.")
            }
            .inkDisabled(!model.border.enabled)
        }
    }

    /// The chosen style's width and colours. Solid and Keyline share the
    /// mat; the film frame's width adds black around it.
    @ViewBuilder private var borderControls: some View {
        switch model.border.style {
        case .solid, .keyline:
            let keyline = model.border.style == .keyline
            borderSlider("Width", \.width, 0...15, step: 0.1, help: "The mat’s width, in percent of the photograph’s long edge.")
            if keyline { borderSlider("Line width", \.line, 0.05...2, step: 0.05, help: "The line’s width, in percent of the photograph’s long edge.") }
            ColourWellRow(title: "Color", color: model.borderColor(\.color), help: "The mat’s colour.")
            if keyline { ColourWellRow(title: "Line color", color: model.borderColor(\.lineColor), help: "The line’s colour.") }
        case .film:
            borderSlider("Width", \.filmWidth, 0...15, step: 0.1, help: "Adds black around the film frame, in percent of the photograph’s long edge.")
        case .film35:
            EmptyView()
        }
    }

    /// A border width in percent of the photograph's long edge.
    private func borderSlider(_ title: String, _ setting: WritableKeyPath<BorderSettings, Double>, _ range: ClosedRange<Double>, step: Double,
                              help: String) -> some View {
        let initial = BorderSettings()[keyPath: setting]
        return LabeledSlider(title: title, value: model.borderPercent(setting, range: range, step: step), range: range, step: step, unit: "%",
                             defaultValue: initial * 100, modified: abs(model.border[keyPath: setting] - initial) > 1e-9,
                             reset: { model.border(setting).wrappedValue = initial },
                             editing: { $0 ? model.beginInteraction() : model.endInteraction(action: "Change Border") },
                             help: help)
    }

    private func control(_ adjustment: Adjustment) -> some View { AdjustmentControl(model: model, adjustment: adjustment) }
    /// A photograph's aspect ratio as people name it, such as 16:9.
    static func ratio(_ size: CGSize) -> String {
        let ratio = max(size.width, size.height) / min(size.width, size.height)
        let named = [(1.0, "1:1"), (5.0 / 4, "5:4"), (7.0 / 5, "7:5"), (16.0 / 9, "16:9"), (2.0, "2:1"), (21.0 / 9, "21:9")]
        if let (_, name) = named.first(where: { abs(ratio / $0.0 - 1) <= 0.01 }) { return name }
        return String(format: "%.2f:1", Double(ratio))
    }
}

/// Export, pinned to the foot of the Output tab: what an export makes, over
/// the yellow button (§4.11). A batch's progress is in the status line.
struct ExportFoot: View {
    let model: EditorModel

    var body: some View {
        VStack(spacing: 8) {
            Text(Self.summary(photo: model.photoSize))
                .font(Theme.serif(14, italic: true)).foregroundStyle(Theme.label).lineLimit(1)
            Button(model.exportTitle, action: model.exportPanel)
                .buttonStyle(PencilButtonStyle())
                .disabled(!model.canExport || model.cropping)
                .inkFocus { if model.canExport && !model.cropping { model.exportPanel() } }
                .help(model.exportTitle == "Export Photograph…" ? "Export photograph (⇧⌘E)" : "Export the selected photographs (⇧⌘E)")
        }
        .padding(.horizontal, Theme.Size.gutter)
        .padding(.top, 11)
        .padding(.bottom, 16)
        .overlay(alignment: .top) { InkRule() }
    }

    /// "3984 × 2656 · JPEG 95 · sRGB": the photograph's size, then the
    /// format and colour space the save panel will offer first.
    static func summary(photo: CGSize, format: OutputFormat = .last, quality: Int = SaveSettings.lastQuality, chosenSpace: Int = SaveSettings.chosenSpace) -> String {
        let depth = SaveSettings.bitDepth(for: format)
        let kind = switch format {
        case .jpeg: "JPEG \(quality)"
        case .png: "PNG \(depth)-bit"
        case .tiff: "TIFF \(depth)-bit"
        }
        let space = SaveSettings.colorSpaces[SaveSettings.effectiveSpace(chosenSpace, bitDepth: format == .jpeg ? 8 : depth)]
        let size = photo == .zero ? nil : "\(Int(photo.width)) × \(Int(photo.height))"
        return ([size, kind, space].compactMap { $0 }).joined(separator: " · ")
    }
}

// Crop's Angle slider, until crop gets its dial (step 6).
/// A small slider that double-click or Option-click resets, as in Lightroom
/// and Capture One. AppKit, because NSSlider tracks the mouse itself and
/// SwiftUI's tap gestures on a Slider never fire. A double-click's first
/// click still moves the knob; the second resets.
struct ResettableSlider: NSViewRepresentable {
    let title: String
    let value: Binding<Double>
    let range: ClosedRange<Double>
    /// The default: on macOS 26 the track fills from here to the knob, so a
    /// moved slider shows where it started.
    var neutral: Double? = nil
    let editing: (Bool) -> Void
    let reset: () -> Void
    /// AppKit's tooltip: SwiftUI's `.help` doesn't reach a hosted NSView.
    var toolTip: String? = nil

    func makeNSView(context: Context) -> NSSlider {
        let slider = Control()
        slider.isContinuous = true
        slider.target = context.coordinator
        slider.action = #selector(Coordinator.changed(_:))
        slider.setContentHuggingPriority(.defaultLow, for: .horizontal)
        slider.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return slider
    }

    func updateNSView(_ slider: NSSlider, context: Context) {
        context.coordinator.parent = self
        let slider = slider as! Control
        slider.editing = editing
        slider.reset = reset
        slider.setAccessibilityLabel(title)
        if slider.toolTip != toolTip { slider.toolTip = toolTip }
        // SwiftUI applies its environment's control size after makeNSView.
        if slider.controlSize != .small { slider.controlSize = .small }
        if slider.minValue != range.lowerBound { slider.minValue = range.lowerBound }
        if slider.maxValue != range.upperBound { slider.maxValue = range.upperBound }
        // neutralValue needs the macOS 26 SDK, which release CI's Xcode 16
        // lacks; builds from it fill the track from the minimum, as before.
        #if compiler(>=6.2)
        if #available(macOS 26, *) {
            let neutral = min(max(neutral ?? range.lowerBound, range.lowerBound), range.upperBound)
            if slider.neutralValue != neutral { slider.neutralValue = neutral }
        }
        #endif
        slider.wanted = value.wrappedValue
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject {
        var parent: ResettableSlider
        init(_ parent: ResettableSlider) { self.parent = parent }
        @objc func changed(_ slider: NSSlider) {
            parent.value.wrappedValue = slider.doubleValue
            // Read back the snapped value at once rather than waiting for
            // updateNSView, so the knob's target is current when a drag ends.
            (slider as! Control).wanted = parent.value.wrappedValue
        }
    }

    final class Control: NSSlider {
        var editing: (Bool) -> Void = { _ in }
        var reset: () -> Void = {}
        /// The model's value, snapped to the step, which the knob shows.
        var wanted = 0.0 {
            didSet { if !tracking { show() } }
        }
        private var tracking = false
        private var settling: Timer?

        private func show() {
            if doubleValue != wanted { doubleValue = wanted }
        }

        override func mouseDown(with event: NSEvent) {
            if event.clickCount == 2 || event.modifierFlags.contains(.option) {
                reset()
                return
            }
            settling?.invalidate()
            // NSSlider tracks the drag inside mouseDown, so it returns when the mouse is released.
            tracking = true
            editing(true)
            super.mouseDown(with: event)
            tracking = false
            editing(false)
            show()
            // On macOS 26 the slider writes its last tracked value back about
            // 0.25 s after release, which undoes the step snap and a
            // double-click's reset. Keep showing the model's value until then.
            // A default-mode timer, so it never fires during a drag.
            let until = Date(timeIntervalSinceNow: 0.6)
            settling = Timer.scheduledTimer(withTimeInterval: 0.03, repeats: true) { [weak self] timer in
                guard let self, Date() < until else { return timer.invalidate() }
                self.show()
            }
        }
    }
}

/// Takes focus as it appears, selecting its text, in the inspector column
/// where SwiftUI focus doesn’t reach.
class FocusingTextField: NSTextField {
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.async { [weak self] in
            guard let self, let window = self.window else { return }
            window.makeFirstResponder(self)
        }
    }
}
