import SwiftUI

/// Your negative (ticket 33): a scan of a real colour negative, read back
/// to the film's dyes with upstream's convert-film route, then printed.
/// The words and rules the label, the tray and Prep share.
enum NegativeScan {
    /// Upstream's `workflow.route` strings.
    static let route = "input > convert-film > print > scan"
    /// Read as this unless the photograph's film is a colour negative already.
    static let defaultFilm = "kodak_portra_400"
    /// The tray's "Your negative" tile; never a stock the helper knows.
    static let stockID = "negs.your-negative"
    static let name = "Your negative"
    static let tileText = "Read the photograph as a scan of a colour negative"

    /// What reading a negative sets, and their defaults (the engine's),
    /// put back when the photograph is printed as a scene again.
    static let ownPaths = ["workflow.route", "film_render.base", "film_render.convert",
                           "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"]
    static var defaults: [String: JSONValue] = [:]

    /// What finding the base sets: this frame's measurements, so a reset
    /// of any of them on a negative finds them again.
    static let foundPaths = ["film_render.base", "film_render.convert.exposure_compensation_ev",
                             "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"]

    /// `path` is, holds or lies in one of `foundPaths`.
    static func isFound(_ path: String) -> Bool {
        foundPaths.contains { $0 == path || path.hasPrefix($0 + ".") || $0.hasPrefix(path + ".") }
    }

    /// `state` printed as a scene again: what reading a negative set, back
    /// to the defaults.
    static func leaving(_ state: EditState) -> EditState {
        var next = state
        for path in ownPaths {
            next.params[path] = defaults[path] ?? (path == "workflow.route" ? .string("input > film > print > scan") : next.params[path])
        }
        // The crop to the picture stays: it's geometry, with its own undo step.
        next.roll = nil
        return next
    }

    /// How the scan is read from the file: a RAW's white balance and lens
    /// correction, a TIFF's interpretation. A new one reads other light, so
    /// the base is found again; a roll's frames share it, and a preset never
    /// changes it on a negative.
    static let scanReading = ["io.raw_white_balance", "io.lens_correction", "io.input_interpretation"]

    /// What a preset never changes on a negative (decision 6): how it's read
    /// (the film, base, backlight, route and how the file is read) and its
    /// print balance, which belongs to this frame and its roll. The preset
    /// brings the look and the paper; a new paper prints it as Auto does.
    static let presetKeeps = ["workflow", "film_render.base", "film_render.convert",
                              "enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"] + scanReading

    /// `applied` (a preset over `current`) with `current`'s negative kept.
    static func keepingNegative(_ applied: EditState, from current: EditState) -> EditState {
        guard current.params["workflow.route"].string == route else { return applied }
        var next = applied
        next.film = current.film
        for path in presetKeeps { next.params[path] = current.params[path] }
        next.params["io.scan_film"] = .bool(false)
        next.roll = current.roll
        return next
    }

    // MARK: Darkroom roll (§9.2)

    /// `target` joining `roll`, printed like `reference`: the read-as film,
    /// paper, backlight, how the scan is read (white balance, lens
    /// correction, interpretation) and base tuning are the reference
    /// frame's, and the filtration the roll's unless `target` is a frame of
    /// this roll that set its own. Its print exposure, crop and own base are
    /// measured for it afterwards (`EditorModel.useNegativeForRoll`).
    static func joining(_ roll: NegativeRoll, from reference: EditState, into target: EditState) -> EditState {
        var next = target
        next.film = reference.film; next.paper = reference.paper
        for path in ["workflow", "film_render.base", "film_render.convert", "film_render.development_time", "print_render.development_time"] + scanReading {
            next.params[path] = reference.params[path]
        }
        let ownFiltration = target.roll.map { $0.id == roll.id && !$0.filtered(target) } ?? false
        if !ownFiltration {
            next.params["enlarger.m_filter_shift"] = .number(roll.magenta)
            next.params["enlarger.y_filter_shift"] = .number(roll.yellow)
        }
        next.params["io.scan_film"] = .bool(false)
        next.brightnessMatch = nil
        next.roll = roll
        return next
    }

    /// How much more light the camera gave `frame`'s scan than
    /// `reference`'s, in stops, from the shutter, aperture and ISO both
    /// recorded (a lens without contacts records no aperture); nil when
    /// they share none (a scanner's TIFF: the same light each time).
    static func scanStops(of frame: PhotoFacts?, from reference: PhotoFacts?) -> Double? {
        var stops: Double?
        func add(_ value: Double?, _ base: Double?, _ term: (Double, Double) -> Double) {
            if let value, let base { stops = (stops ?? 0) + term(value, base) }
        }
        add(frame?.exposure, reference?.exposure) { log2($0 / $1) }
        add(frame?.aperture, reference?.aperture) { 2 * log2($1 / $0) }
        add(frame?.iso, reference?.iso) { log2($0 / $1) }
        return stops
    }

    /// The base tuning's exposure for a frame without clear film of its own:
    /// the roll's, less the extra light its scan got. Exact for a change of
    /// exposure alone (the converter's gain is 2^ev on the scanned RGB).
    static func exposureEV(roll ev: Double, scanStops: Double?) -> Double { ev - (scanStops ?? 0) }

    // MARK: The picture

    /// The crop to the picture the rebate finder found (`negative_base`'s
    /// `picture`, in the decoded photograph's frame like the crop): a direct
    /// copy of its rectangle and straightening. Nil for none.
    static func crop(picture: JSONValue) -> CropSettings? {
        guard case .object = picture else { return nil }
        let (x0, y0, x1, y1) = (picture["x0"].double, picture["y0"].double, picture["x1"].double, picture["y1"].double)
        guard [x0, y0, x1, y1].allSatisfy(\.isFinite), 0 <= x0, x0 < x1, x1 <= 1, 0 <= y0, y0 < y1, y1 <= 1 else { return nil }
        var crop = CropSettings(rect: CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0))
        crop.aspect = .freeform
        crop.angle = min(max(picture["angle_deg"].double, -CropSettings.maxAngle), CropSettings.maxAngle)
        return crop
    }

    /// The picture as `negative_auto` and `negative_meter` bounds: left, top, right, bottom.
    static func bounds(picture: JSONValue) -> JSONValue? {
        guard crop(picture: picture) != nil else { return nil }
        return .array(["x0", "y0", "x1", "y1"].map { picture[$0] })
    }

    /// What reading this photograph's film edge found, for its source
    /// generation and the way the scan was read (`key`: film, backlight,
    /// white balance and the rest; another reading can find otherwise).
    struct Survey: Equatable {
        var generation: Int
        var key: String?
        /// `negative_base`'s picture, or null.
        var picture: JSONValue
        var looksNegative: Bool
        /// The base a detection finds on this reading, and how it found it.
        var base: JSONValue
        var found: Bool
        var edge: String?

        /// `result` is a `negative_base` answer without a point.
        init(generation: Int, key: String?, result: JSONValue) {
            self.generation = generation; self.key = key
            picture = result["picture"]; looksNegative = result["looks_negative"].bool
            base = result["base"]; found = result["found"].bool
            edge = result["edge"].string.isEmpty ? nil : result["edge"].string
        }
    }

    /// `a` and `b` are the same film base (`film_render.base`), allowing for
    /// the last bit of a number's round trip.
    static func sameBase(_ a: JSONValue, _ b: JSONValue) -> Bool {
        guard case .object(let x) = a, case .object(let y) = b else { return a == b }
        return Set(x.keys).union(y.keys).allSatisfy { key in
            let (p, q) = (x[key] ?? .null, y[key] ?? .null)
            if case .number(let p) = p, case .number(let q) = q { return abs(p - q) <= 1e-9 * max(1, abs(q)) }
            return p == q
        }
    }

    /// Use for whole roll's word for the photographs it left alone.
    static func skipped(_ count: Int) -> String {
        count == 1 ? "1 photograph skipped: it doesn't look like a negative."
            : "\(count) photographs skipped: they don't look like negatives."
    }

    /// Prep's word when the scan doesn't look like a colour negative. A
    /// scan white-balanced on the film base reads so too: no blame.
    static func notNegative(raw: Bool) -> String {
        raw ? "This doesn't look like a colour negative. If it is, set White balance to Daylight."
            : "This doesn't look like a colour negative."
    }

    /// A film a negative can be read as: a colour negative.
    static func readable(_ stock: Stock) -> Bool { !stock.paper && !stock.positive && !stock.bw }

    /// The tray's pseudo stock for "Your negative".
    static let stock = Stock(.object(["id": .string(stockID), "name": .string(name)]))

    /// The light behind the negative while it was photographed or scanned.
    static let scanLights: [(id: String, title: String)] = [
        ("D55", "Daylight, 5500 K"), ("D50", "Light box, 5000 K"), ("D65", "Cool LED, 6500 K"),
    ]

    enum Pick: Equatable {
        /// A click on clear film (the rebate) reads the base there.
        case base
        /// A click on something grey balances the print on it.
        case neutral

        var prompt: String {
            switch self {
            case .base: "Click the clear film between frames or along the edge. Esc cancels."
            case .neutral: "Click something that should be grey. Esc cancels."
            }
        }
    }

    /// `film_render.convert.flat_field`: the light frame each scan is
    /// divided by (§9.4), a path or empty for off. It sits with the backlight
    /// in the negative's convert settings, so it goes where they go (the roll).
    static let flatFieldPath = "film_render.convert.flat_field"

    /// Prep's Flat field row: the roll's light frame, and whether the
    /// helper used it on this frame.
    struct FlatField: Equatable {
        /// The light frame's path; empty is off.
        var path: String
        /// The helper's answer for this frame and this light frame: nil
        /// until it has said.
        var used: Bool?
        var reason: String?

        /// `answer` is a render's or a base detection's `flat_field`
        /// (`{path, used, reason}`); one for another light frame is ignored.
        init(path: String, answer: JSONValue = .null) {
            self.path = path
            if !path.isEmpty, answer["path"].string == path {
                used = answer["used"].bool
                reason = answer["reason"].string.isEmpty ? nil : answer["reason"].string
            }
        }

        var isOn: Bool { !path.isEmpty }
        /// The row's value: the light frame's name, or Off.
        var value: String { isOn ? URL(fileURLWithPath: path).lastPathComponent : "Off" }
        /// Under the row: why this frame doesn't use it.
        var hint: String? {
            guard isOn, used == false else { return nil }
            return reason ?? "The light frame isn't used on this frame."
        }
        static let help = "A frame of the bare light, shot with the roll's camera, lens and aperture. Each scan is divided by it before the base is read, so the light's falloff doesn't print."
    }

    /// Where the film base shown came from: what a base detection found,
    /// or, for a base no detection made while the photograph is open, the
    /// roll's or one set by hand (`from`).
    struct BaseStatus: Equatable {
        var found: Bool
        var picked: Bool
        var edge: String?
        var from: Origin? = nil

        enum Origin: Equatable {
            /// The roll's base, on a frame without clear film of its own.
            case roll
            /// Picked (when the photograph was open before) or set in Lab.
            case hand
        }

        /// The base is a guess from the picture: no clear film was found.
        var guessed: Bool { !found && !picked && from == nil }

        /// `edge` is the photograph's own; the sentence names it as `orientation` shows it.
        func sentence(as orientation: Orientation = Orientation()) -> String {
            switch from {
            case .roll?: return "Film base from the roll: this frame shows no clear film of its own."
            case .hand?: return "Film base as picked or set in Lab."
            case nil: break
            }
            if picked { return "Film base read where you clicked." }
            if found { return edge.map { "Film base found along the \(orientation.shownEdge($0)) edge." } ?? "Film base found all round the picture." }
            return "No clear film found: the base is a guess from the thinnest part of the picture."
        }
    }
}

/// A darkroom roll (§9.2): frames printed alike, as a printer prints a
/// roll with one filtration and gives each frame its own exposure.
///
/// Where it lives: in each frame's own sidecar (`edits.roll`), beside that
/// frame's full copy of the shared settings. Negs keeps no catalog, so a
/// roll file would be one more thing to lose when a frame is moved, copied
/// or opened on another computer or by the CLI; this way every frame
/// renders on its own, and the roll is just the frames that carry its id.
/// The roll's filtration is kept too, so a frame knows whether it set its
/// own (its filtration differs) and can go back to the roll's.
struct NegativeRoll: Codable, Equatable, Sendable {
    var id: String
    /// The roll's enlarger filtration: magenta and yellow shifts.
    var magenta: Double
    var yellow: Double
    /// The paper the roll is printed on, which its filtration is balanced
    /// for; nil for a roll kept before it was recorded.
    var paper: String?

    init(id: String = SidecarRestore.newVariantID(), magenta: Double, yellow: Double, paper: String? = nil) {
        self.id = id; self.magenta = magenta; self.yellow = yellow; self.paper = paper
    }

    /// `state` prints with the roll's filtration, not one of its own.
    func filtered(_ state: EditState) -> Bool {
        state.params["enlarger.m_filter_shift"] == .number(magenta) && state.params["enlarger.y_filter_shift"] == .number(yellow)
    }

    /// Prep's word for a frame of this roll with its own filtration
    /// (`state`): why, when it's on another paper.
    func ownFiltration(_ state: EditState) -> String {
        if let paper, paper != state.paper { return "On another paper than its roll, so it has its own filtration." }
        return "This frame has its own filtration."
    }
}

/// Prep in negative mode: the film base, the scan light and the balance.
struct NegativePrep: View {
    let model: EditorModel
    @State private var moreShown = false

    var body: some View {
        // One row per part of the film base, so another (a flat field) slots in.
        InkSection(title: "Film base", edited: false) {
            HintText(model.negativeWorking ?? model.negativeBase?.sentence(as: Orientation(turns: model.imageRotation, flip: model.imageFlip)) ?? "Reading the negative as \(model.filmName).")
            if let warning = model.negativeWarning {
                HintText(warning)
            }
            if let base = model.negativeBase, base.guessed {
                HintText(model.state.roll == nil
                         ? "Pick from the rebate if the scan shows clear film; otherwise this guess can tint the print."
                         : "Pick from the rebate if the scan shows clear film; otherwise Use for whole roll on a frame that has it.")
            }
            buttons([
                ("Find again", "Looks for the clear film again, forgetting a picked point.", model.redetectNegativeBase),
                ("Pick from rebate", NegativeScan.Pick.base.prompt, { model.negativePick = .base }),
            ])
            FlatFieldRow(model: model)
            buttons([("Crop to picture", "Crops to the picture inside the clear film, levelled by the rebate.", model.cropToPicture)])
            if moreShown {
                RevealedRows {
                    ChoiceRow(title: "Backlight", items: NegativeScan.scanLights.map { .option($0.title, tag: $0.id) },
                              selection: model.value("film_render.convert.scan_illuminant").string,
                              changed: model.isModified("film_render.convert.scan_illuminant"),
                              help: "The light behind the negative while it was photographed or scanned. Daylight suits most light pads and scanners.") {
                        model.set("film_render.convert.scan_illuminant", .string($0))
                    }
                }
            } else {
                MoreLine(text: "Backlight") { moreShown = true }
                    .padding(.top, 6)
            }
            if model.state.roll != nil {
                HintText("A frame of a roll: the film, backlight, base and filtration are the roll's; the print exposure is this frame's own.")
            }
            buttons([("Use for whole roll", "Prints every photograph in view as a frame of this roll: this film, backlight, base and filtration, each frame with its own exposure. Frames that set their own filtration keep it.",
                      model.useNegativeForRoll)])
        }
        InkSection(title: "Balance", edited: false) {
            HintText("Balance Print ran when the base was found, and the print exposure was metered. Auto prints this frame as a minilab would, and runs again on each new paper; the picker balances on something grey. Both set Lab › Enlarger.")
            buttons([
                ("Auto", "Sets the enlarger's magenta, yellow and print exposure so this frame prints as a minilab's auto printer would.",
                 model.autoPrintNegative),
                ("Pick neutral", NegativeScan.Pick.neutral.prompt, { model.negativePick = .neutral }),
            ])
            if let roll = model.state.roll, !roll.filtered(model.state) {
                HintText(roll.ownFiltration(model.state))
                buttons([("Roll's filtration", "Prints this frame with the roll's magenta and yellow again, and meters its exposure.",
                          model.useRollFiltration)])
            }
        }
        // Said until it's done.
        if model.isRaw, model.value("io.raw_white_balance.mode").string != "daylight" {
            InkSection(title: nil) {
                HintText("Set White balance to Daylight for a negative on a daylight backlight; As shot can tint it.")
            }
        }
    }

    private func buttons(_ items: [(String, String, () -> Void)]) -> some View {
        HStack(spacing: 8) {
            Spacer()
            ForEach(items.indices, id: \.self) { i in
                Button(items[i].0, action: items[i].2)
                    .buttonStyle(OutlinedButtonStyle())
                    .disabled(model.negativeWorking != nil)
                    .help(items[i].1)
            }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .padding(.vertical, 4)
    }
}

/// Prep › Film base › Flat field: the light frame, Choose… and Off, and
/// why this frame doesn't use it.
struct FlatFieldRow: View {
    let model: EditorModel

    var body: some View {
        let flat = model.flatField
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RowLabel(title: "Flat field", bright: flat.isOn)
            Spacer(minLength: 8)
            Text(flat.value).font(Theme.serif(14.5, weight: flat.isOn ? 700 : 400)).foregroundStyle(Theme.paper)
                .lineLimit(1).truncationMode(.middle)
            Button("Choose…", action: model.chooseFlatFieldPanel)
                .buttonStyle(OutlinedButtonStyle())
            if flat.isOn {
                Button("Off", action: model.clearFlatField)
                    .buttonStyle(OutlinedButtonStyle())
            }
        }
        .disabled(model.negativeWorking != nil)
        .padding(.horizontal, Theme.Size.gutter)
        .frame(minHeight: Theme.Size.row)
        .tooltip(NegativeScan.FlatField.help)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Flat field")
        .accessibilityValue(flat.value)
        if let hint = flat.hint {
            HintText(hint, reason: true)
        }
    }
}
