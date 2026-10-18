import SwiftUI

enum Adjustment: Identifiable {
    case slider(String, String, ClosedRange<Double>, Double)
    case toggle(String, String)
    case choice(String, String, [String])

    var id: String {
        switch self {
        case .slider(_, let path, _, _), .toggle(_, let path), .choice(_, let path, _): return path
        }
    }
}

enum InspectorSection: String, CaseIterable {
    case camera = "Camera", film = "Film", print = "Print", scan = "Scan & Color"
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
    .init(title: "Grain", symbol: "circle.dotted", section: .film, enable: "film_render.grain.active", controls: [
        .slider("Particle area (µm²)", "film_render.grain.agx_particle_area_um2", 0.05...1, 0.01),
        .slider("Post blur", "film_render.grain.blur", 0...3, 0.01),
        .toggle("Sublayers", "film_render.grain.sublayers_active"),
        .slider("Sublayer count", "film_render.grain.n_sub_layers", 1...4, 1)
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
    .init(title: "Enlarger", symbol: "light.overhead.left", section: .print, controls: [
        .slider("Print exposure", "enlarger.print_exposure", 0.1...5, 0.01),
        .toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
        .slider("Magenta shift", "enlarger.m_filter_shift", -50...50, 0.1),
        .slider("Yellow shift", "enlarger.y_filter_shift", -50...50, 0.1),
        .slider("Preflash exposure", "enlarger.preflash_exposure", 0...0.5, 0.01),
        .slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50...50, 0.1),
        .slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50...50, 0.1)
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
        .choice("Spectral upsampling", "settings.rgb_to_raw_method", ["hanatos2025", "arctic2026alpha02", "mallett2019"])
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
        }
    }
}

struct SliderRow: View {
    let model: EditorModel
    let title: String
    let path: String
    let range: ClosedRange<Double>
    let step: Double

    @State private var typing = false
    @FocusState private var fieldFocused: Bool
    private var decimals: Int { max(0, Int(-log10(step).rounded(.down))) }
    private var format: FloatingPointFormatStyle<Double> { .number.precision(.fractionLength(0...decimals)) }

    var body: some View {
        let value = model.number(path, range: range, step: step)
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                Text(title).lineLimit(1)
                    .onTapGesture(count: 2) { model.resetValue(path) }
                Spacer(minLength: 8)
                if model.isModified(path) {
                    Button { model.resetValue(path) } label: { Image(systemName: "arrow.counterclockwise") }
                        .buttonStyle(.borderless).foregroundStyle(.secondary).imageScale(.small)
                        .help("Reset to \(model.defaultValue(path).double.formatted(.number.precision(.fractionLength(0...decimals))))")
                        .accessibilityLabel("Reset \(title)")
                }
                // A field only while typing, so the window never focuses one by default.
                if typing {
                    TextField(title, value: value, format: format)
                        .textFieldStyle(.roundedBorder).labelsHidden().controlSize(.small)
                        .multilineTextAlignment(.trailing).monospacedDigit().frame(width: 64)
                        .focused($fieldFocused)
                        .onAppear { fieldFocused = true }
                        .onSubmit { typing = false }
                        .onExitCommand { typing = false }
                        .onChange(of: fieldFocused) { _, focused in if !focused { typing = false } }
                } else {
                    Text(value.wrappedValue, format: format)
                        .monospacedDigit().foregroundStyle(.secondary)
                        .frame(minWidth: 40, alignment: .trailing)
                        .contentShape(Rectangle())
                        .onTapGesture { typing = true }
                        .help("Click to type a value")
                }
            }
            Slider(value: value, in: range) { Text(title) } onEditingChanged: { editing in
                editing ? model.beginInteraction() : model.endInteraction()
            }
            .labelsHidden().controlSize(.small)
        }
        .padding(.vertical, 1)
        .resettable(model, path)
        .accessibilityElement(children: .contain)
    }
}

private extension View {
    func resettable(_ model: EditorModel, _ path: String) -> some View {
        contextMenu {
            Button("Reset to Default") { model.resetValue(path) }.disabled(!model.isModified(path))
        }
    }
}

struct AdjustmentInspector: View {
    let model: EditorModel
    @AppStorage("expandedAdjustments") private var expanded = "Grain,Halation,Enlarger"

    var body: some View {
        let scanning = model.value("io.scan_film").bool
        Form {
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
            Section("Camera") {
                Toggle("Auto exposure", isOn: model.toggle("camera.auto_exposure"))
                if model.value("camera.auto_exposure").bool {
                    control(.choice("Metering", "camera.auto_exposure_method", ["average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"]))
                }
                control(.slider("Exposure (EV)", "camera.exposure_compensation_ev", -5...5, 0.1))
                control(.slider("Film format (mm)", "camera.film_format_mm", 4...120, 0.1))
                control(.slider("Lens blur (µm)", "camera.lens_blur_um", 0...100, 0.1))
                if model.isRaw {
                    disclosure("RAW White Balance", symbol: "thermometer.sun") {
                        control(.choice("White balance", "io.raw_white_balance.mode", ["as_shot", "daylight", "tungsten", "custom"]))
                        if model.value("io.raw_white_balance.mode").string == "custom" {
                            control(.slider("Temperature (K)", "io.raw_white_balance.temperature", 1667...25000, 1))
                            control(.slider("Tint", "io.raw_white_balance.tint", 0.01...4, 0.01))
                        }
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
                    control(.slider("Working resolution", "io.upscale_factor", 0.1...1, 0.05))
                    Toggle("Limit preview to 12 MP", isOn: Binding(get: { model.limitsPreview }, set: model.limitPreview))
                        .help("Develops the preview faster on large photographs. Export keeps the working resolution.")
                    Toggle("Encode output as sRGB", isOn: model.toggle("io.output_cctf_encoding"))
                    Toggle("Use less memory for export", isOn: model.toggle("settings.serial_grain"))
                        .help("Processes grain channels one at a time. Same result, less memory, longer export.")
                }
            }
            Section("Border") {
                Toggle("Add border", isOn: model.border(\.enabled))
                Group {
                    let photo = model.photoSize == .zero ? nil : model.photoSize
                    BorderStylePicker(selection: model.border(\.style), photo: photo)
                    if let photo, !BorderStyle.film35.fits(photo) {
                        Text("35mm needs a 3:2 or 3:4 photograph. This one is \(Self.ratio(photo)); crop it to 3:2 or 4:3 to use it.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Toggle("Include in export", isOn: model.border(\.export))
                        .help("Adds the border to exported photographs and saved previews.")
                }
                .disabled(!model.border.enabled)
            }
            Section {
                Button("Reset All Adjustments", action: model.reset).frame(maxWidth: .infinity)
                    .help("Restore defaults while keeping the film, paper, and rotation. You can undo this.")
            }
        }
        .formStyle(.grouped)
        .disabled(!model.ready)
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
