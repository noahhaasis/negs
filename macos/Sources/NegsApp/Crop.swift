import AppKit
import QuartzCore
import SwiftUI

/// A presentation crop, like the border: changing one never re-renders. The
/// rectangle is a fraction of the unrotated photograph, origin top left, so
/// it turns with the photograph. Straightening turns the photograph by
/// `angle` degrees (clockwise as shown) about its centre, and the rectangle
/// is then in that turned frame. Export and Save Preview crop with
/// `spektrafilm-io::crop` before rotating and adding the border; keep the
/// pixel rounding in sync.
struct CropSettings: Codable, Equatable, Sendable {
    static let full = CGRect(x: 0, y: 0, width: 1, height: 1)
    static let maxAngle = 45.0
    var rect = CropSettings.full
    /// The aspect ratio the rectangle was drawn with, offered again next time.
    var aspect = CropAspect.original
    /// The custom ratio, width by height as shown.
    var custom = CGSize(width: 5, height: 4)
    var angle = 0.0

    var cropped: Bool { rect != Self.full || angle != 0 }

    /// The pixel rectangle in a `size` photograph, rounded like the exporter:
    /// edges round to the nearest pixel, and at least one pixel remains. A
    /// straightened crop keeps its origin and rounds its sides.
    func pixels(in size: CGSize) -> CGRect {
        guard cropped else { return CGRect(origin: .zero, size: size) }
        let (w, h) = (size.width.rounded(), size.height.rounded())
        if angle != 0 {
            return CGRect(x: rect.minX * w, y: rect.minY * h, width: max((rect.width * w).rounded(), 1), height: max((rect.height * h).rounded(), 1))
        }
        func edge(_ fraction: CGFloat, _ size: CGFloat) -> CGFloat { min(max((fraction * size).rounded(), 0), size) }
        let x1 = min(max(edge(rect.maxX, w), edge(rect.minX, w) + 1), w)
        let y1 = min(max(edge(rect.maxY, h), edge(rect.minY, h) + 1), h)
        let x0 = min(edge(rect.minX, w), x1 - 1), y0 = min(edge(rect.minY, h), y1 - 1)
        return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }
    /// The cropped photograph's size, before rotation.
    func size(of size: CGSize) -> CGSize { size == .zero ? .zero : pixels(in: size).size }

    /// The rectangle as the photograph is shown, after `rotation` clockwise
    /// quarter turns, like `ImageBuf::rotated_cw`.
    static func shown(_ rect: CGRect, rotation: Int) -> CGRect {
        var r = rect
        for _ in 0..<((rotation % 4 + 4) % 4) {
            r = CGRect(x: 1 - r.minY - r.height, y: r.minX, width: r.height, height: r.width)
        }
        return r
    }
    /// The inverse of `shown`.
    static func unrotated(_ rect: CGRect, rotation: Int) -> CGRect {
        var r = rect
        for _ in 0..<((rotation % 4 + 4) % 4) {
            r = CGRect(x: r.minY, y: 1 - r.minX - r.width, width: r.height, height: r.width)
        }
        return r
    }
}

enum CropAspect: String, Codable, Sendable, CaseIterable, Identifiable {
    case original, freeform, square
    case sixteenNine = "16:9", threeTwo = "3:2", fourThree = "4:3", fiveFour = "5:4", sevenFive = "7:5"
    case custom

    var id: String { rawValue }
    var title: String {
        switch self {
        case .freeform: "Freeform"
        case .original: "Original"
        case .square: "Square"
        case .custom: "Custom"
        default: rawValue
        }
    }
    /// The film or print format with that shape.
    var format: String? {
        switch self {
        case .square: "6×6"
        case .threeTwo: "35mm"
        case .fourThree: "645"
        case .fiveFour: "4×5"
        case .sevenFive: "5×7"
        default: nil
        }
    }
    /// The long side over the short side, for a photograph shown at `photo`.
    func ratio(photo: CGSize) -> CGFloat? {
        switch self {
        case .freeform, .custom: nil
        case .original: photo.width > 0 && photo.height > 0 ? max(photo.width, photo.height) / min(photo.width, photo.height) : nil
        case .square: 1
        case .sixteenNine: 16.0 / 9
        case .threeTwo: 1.5
        case .fourThree: 4.0 / 3
        case .fiveFour: 1.25
        case .sevenFive: 1.4
        }
    }
    /// Whether the ratio has a landscape and a portrait orientation.
    var turns: Bool { self != .freeform && self != .square }
}

/// Crop rectangle edits, in pixels of the photograph as shown (origin top
/// left). A straightened photograph turns about its centre, so its edges are
/// no longer the rectangle's bounds: `fits`, `constrain`, and `inscribe`
/// keep the crop on the turned photograph.
enum CropGeometry {
    /// An edge or corner: -1 is the left or top edge, 1 the right or bottom,
    /// 0 neither. Both zero moves the rectangle.
    struct Handle: Equatable {
        var x = 0, y = 0
        static let move = Handle()
        var corner: Bool { x != 0 && y != 0 }
    }

    /// The largest `ratio` (width over height) rectangle in `bounds`,
    /// centred on `center` as far as it fits.
    static func fit(ratio: CGFloat, around center: CGPoint, in bounds: CGRect) -> CGRect {
        let width = min(bounds.width, bounds.height * ratio), height = width / ratio
        return CGRect(x: clamp(center.x - width / 2, bounds.minX, bounds.maxX - width),
                      y: clamp(center.y - height / 2, bounds.minY, bounds.maxY - height), width: width, height: height)
    }
    static func fit(ratio: CGFloat, around center: CGPoint, in bounds: CGSize) -> CGRect {
        fit(ratio: ratio, around: center, in: CGRect(origin: .zero, size: bounds))
    }

    static func move(_ rect: CGRect, by offset: CGSize, in bounds: CGRect) -> CGRect {
        CGRect(x: clamp(rect.minX + offset.width, bounds.minX, bounds.maxX - rect.width),
               y: clamp(rect.minY + offset.height, bounds.minY, bounds.maxY - rect.height), width: rect.width, height: rect.height)
    }
    static func move(_ rect: CGRect, by offset: CGSize, in bounds: CGSize) -> CGRect {
        move(rect, by: offset, in: CGRect(origin: .zero, size: bounds))
    }

    /// Drags `handle` of `rect` to `point`. The opposite edges stay put; with
    /// a `ratio`, an edge drag grows the other side about its centre, and a
    /// corner follows whichever side the pointer pulls further.
    static func resize(_ rect: CGRect, handle: Handle, to point: CGPoint, ratio: CGFloat?, in bounds: CGRect, minimum: CGFloat) -> CGRect {
        let p = CGPoint(x: clamp(point.x, bounds.minX, bounds.maxX), y: clamp(point.y, bounds.minY, bounds.maxY))
        let anchor = CGPoint(x: handle.x < 0 ? rect.maxX : rect.minX, y: handle.y < 0 ? rect.maxY : rect.minY)
        let room = CGSize(width: handle.x > 0 ? bounds.maxX - anchor.x : handle.x < 0 ? anchor.x - bounds.minX : bounds.width,
                          height: handle.y > 0 ? bounds.maxY - anchor.y : handle.y < 0 ? anchor.y - bounds.minY : bounds.height)
        let pulled = CGSize(width: max(CGFloat(handle.x) * (p.x - anchor.x), 0), height: max(CGFloat(handle.y) * (p.y - anchor.y), 0))
        var size = rect.size
        if let ratio {
            var width = handle.x != 0 ? pulled.width : 0
            if handle.y != 0 { width = max(width, pulled.height * ratio) }
            let smallest = max(minimum, minimum * ratio)
            width = min(max(width, smallest), min(room.width, room.height * ratio))
            size = CGSize(width: width, height: width / ratio)
        } else {
            if handle.x != 0 { size.width = min(max(pulled.width, min(minimum, room.width)), room.width) }
            if handle.y != 0 { size.height = min(max(pulled.height, min(minimum, room.height)), room.height) }
        }
        func origin(_ side: Int, _ anchor: CGFloat, _ length: CGFloat, _ mid: CGFloat, _ low: CGFloat, _ high: CGFloat) -> CGFloat {
            side > 0 ? anchor : side < 0 ? anchor - length : clamp(mid - length / 2, low, high - length)
        }
        return CGRect(x: origin(handle.x, anchor.x, size.width, rect.midX, bounds.minX, bounds.maxX),
                      y: origin(handle.y, anchor.y, size.height, rect.midY, bounds.minY, bounds.maxY), width: size.width, height: size.height)
    }
    static func resize(_ rect: CGRect, handle: Handle, to point: CGPoint, ratio: CGFloat?, in bounds: CGSize, minimum: CGFloat) -> CGRect {
        resize(rect, handle: handle, to: point, ratio: ratio, in: CGRect(origin: .zero, size: bounds), minimum: minimum)
    }

    /// The handle under `point`, for a crop drawn at `frame` (both in view points).
    static func handle(at point: CGPoint, frame: CGRect) -> Handle? {
        let reach = min(18, frame.width / 3, frame.height / 3), edge: CGFloat = 8
        func side(_ value: CGFloat, _ low: CGFloat, _ high: CGFloat, _ tolerance: CGFloat) -> Int? {
            abs(value - low) <= tolerance ? -1 : abs(value - high) <= tolerance ? 1 : nil
        }
        let inside = frame.insetBy(dx: -edge, dy: -edge).contains(point)
        guard inside else { return nil }
        if let x = side(point.x, frame.minX, frame.maxX, reach), let y = side(point.y, frame.minY, frame.maxY, reach) {
            return Handle(x: x, y: y)
        }
        if let x = side(point.x, frame.minX, frame.maxX, edge) { return Handle(x: x, y: 0) }
        if let y = side(point.y, frame.minY, frame.maxY, edge) { return Handle(x: 0, y: y) }
        return .move
    }

    // MARK: Straightening

    /// `point` turned `degrees` clockwise (y down) about `center`.
    static func turn(_ point: CGPoint, by degrees: Double, about center: CGPoint) -> CGPoint {
        let (sin, cos) = (Foundation.sin(degrees * .pi / 180), Foundation.cos(degrees * .pi / 180))
        let (dx, dy) = (point.x - center.x, point.y - center.y)
        return CGPoint(x: center.x + dx * cos - dy * sin, y: center.y + dx * sin + dy * cos)
    }
    /// The corners of a `photo` turned by `angle`, clockwise from top left.
    static func corners(of photo: CGSize, angle: Double) -> [CGPoint] {
        let center = CGPoint(x: photo.width / 2, y: photo.height / 2)
        return [CGPoint.zero, CGPoint(x: photo.width, y: 0), CGPoint(x: photo.width, y: photo.height), CGPoint(x: 0, y: photo.height)]
            .map { turn($0, by: angle, about: center) }
    }
    /// The turned photograph's bounding box.
    static func bounds(of photo: CGSize, angle: Double) -> CGRect {
        let points = corners(of: photo, angle: angle)
        let xs = points.map(\.x), ys = points.map(\.y)
        return CGRect(x: xs.min()!, y: ys.min()!, width: xs.max()! - xs.min()!, height: ys.max()! - ys.min()!)
    }
    /// Whether `rect` lies on the photograph turned by `angle`.
    static func fits(_ rect: CGRect, angle: Double, photo: CGSize) -> Bool {
        let center = CGPoint(x: photo.width / 2, y: photo.height / 2)
        let tolerance = 1e-6 * max(photo.width, photo.height, 1)
        return [CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY)].allSatisfy {
            let p = turn($0, by: -angle, about: center)
            return p.x >= -tolerance && p.y >= -tolerance && p.x <= photo.width + tolerance && p.y <= photo.height + tolerance
        }
    }
    /// The furthest step from `start` towards `target` that stays on the turned photograph.
    static func constrain(from start: CGRect, to target: CGRect, angle: Double, photo: CGSize) -> CGRect {
        if fits(target, angle: angle, photo: photo) { return target }
        guard fits(start, angle: angle, photo: photo) else { return start }
        func lerp(_ t: CGFloat) -> CGRect {
            CGRect(x: start.minX + (target.minX - start.minX) * t, y: start.minY + (target.minY - start.minY) * t,
                   width: start.width + (target.width - start.width) * t, height: start.height + (target.height - start.height) * t)
        }
        var (low, high): (CGFloat, CGFloat) = (0, 1)
        for _ in 0..<24 {
            let mid = (low + high) / 2
            if fits(lerp(mid), angle: angle, photo: photo) { low = mid } else { high = mid }
        }
        return lerp(low)
    }
    /// `rect` shrunk about its centre, never grown, until it lies on the
    /// photograph turned by `angle`, like the iPhone's automatic zoom while
    /// straightening. A centre off the turned photograph moves towards its
    /// centre, as little as keeps nearly the largest size.
    static func inscribe(_ rect: CGRect, angle: Double, photo: CGSize) -> CGRect {
        let center = CGPoint(x: photo.width / 2, y: photo.height / 2)
        let half = [CGPoint(x: -rect.width / 2, y: -rect.height / 2), CGPoint(x: rect.width / 2, y: -rect.height / 2),
                    CGPoint(x: rect.width / 2, y: rect.height / 2), CGPoint(x: -rect.width / 2, y: rect.height / 2)]
            .map { turn($0, by: -angle, about: .zero) }
        /// The largest scale about `middle`, or nil if `middle` is off the photograph.
        func scale(about middle: CGPoint) -> CGFloat? {
            let q = turn(middle, by: -angle, about: center)
            guard q.x >= 0, q.y >= 0, q.x <= photo.width, q.y <= photo.height else { return nil }
            var s: CGFloat = 1
            for v in half {
                if v.x > 0 { s = min(s, (photo.width - q.x) / v.x) } else if v.x < 0 { s = min(s, q.x / -v.x) }
                if v.y > 0 { s = min(s, (photo.height - q.y) / v.y) } else if v.y < 0 { s = min(s, q.y / -v.y) }
            }
            return max(s, 0)
        }
        let candidates = (0...20).map { i -> (CGPoint, CGFloat) in
            let t = CGFloat(i) / 20
            let middle = CGPoint(x: rect.midX + (center.x - rect.midX) * t, y: rect.midY + (center.y - rect.midY) * t)
            return (middle, scale(about: middle) ?? 0)
        }
        let best = candidates.map(\.1).max() ?? 0
        guard best > 0, let (middle, s) = candidates.first(where: { $0.1 >= best * 0.98 }) else { return rect }
        let size = CGSize(width: rect.width * s, height: rect.height * s)
        return CGRect(x: middle.x - size.width / 2, y: middle.y - size.height / 2, width: size.width, height: size.height)
    }

    private static func clamp(_ value: CGFloat, _ low: CGFloat, _ high: CGFloat) -> CGFloat { min(max(value, low), max(low, high)) }
}

/// The crop being drawn. Nothing changes the edit until Done.
@MainActor @Observable final class CropSession {
    /// A fraction of the unrotated photograph, like `CropSettings.rect`.
    var rect: CGRect
    private(set) var aspect: CropAspect
    /// The locked ratio is taller than wide (for presets; custom uses its sides).
    private(set) var portrait: Bool
    private(set) var custom: CGSize
    /// Degrees clockwise, as `CropSettings.angle`.
    private(set) var angle: Double
    /// The developed photograph as shown, in pixels, and its rotation.
    private(set) var photo: CGSize
    private(set) var rotation: Int
    /// A handle is being dragged; the thirds grid shows.
    var adjusting = false
    /// The dial or slider is turning the photograph; a finer grid shows.
    private(set) var straightening = false
    /// A ratio field has focus, so Return and Escape belong to it.
    var typing = false
    /// The edit before cropping began; Cancel restores its rotation.
    @ObservationIgnored let start: EditState
    /// The crop when straightening began. The crop shrinks from it to stay on
    /// the turned photograph, and grows back as the angle returns.
    @ObservationIgnored private var intended: CGRect?
    /// The decoded source's size, to project Image Size's working frame; zero if unknown.
    @ObservationIgnored let source: CGSize

    init(start: EditState, photo: CGSize, rotation: Int, source: CGSize = .zero) {
        self.start = start; self.source = source
        let settings = start.crop
        rect = settings.rect; aspect = settings.aspect; custom = settings.custom; angle = settings.angle
        self.photo = photo; self.rotation = rotation
        let shown = CropSettings.shown(settings.rect, rotation: rotation)
        portrait = shown.height * photo.height > shown.width * photo.width
        if aspect == .custom, (custom.height > custom.width) != portrait { custom = CGSize(width: custom.height, height: custom.width) }
    }

    var settings: CropSettings {
        guard angle == 0 else { return CropSettings(rect: rect, aspect: aspect, custom: custom, angle: angle) }
        let unit = rect.intersection(CropSettings.full)
        // Within a thousandth of the whole photograph is no crop.
        let whole = abs(unit.minX) < 1e-3 && abs(unit.minY) < 1e-3 && abs(unit.maxX - 1) < 1e-3 && abs(unit.maxY - 1) < 1e-3
        return CropSettings(rect: whole ? CropSettings.full : unit, aspect: aspect, custom: custom)
    }
    var modified: Bool { rect != CropSettings.full || aspect != .original || angle != 0 }

    /// With an Image Size target, the frame this draft would develop: the
    /// smaller the crop, the larger the frame. Nil without a target.
    var projectedWorking: CGSize? {
        guard source != .zero, let target = ImageSize.target(of: start.params) else { return nil }
        return ImageSize.working(source: source, target: target, crop: settings)
    }
    /// The draft needs more working pixels than the helper develops; Done keeps the draft.
    var sizeError: String? {
        guard let working = projectedWorking, ImageSize.exceedsLimit(working: working, source: source) else { return nil }
        return ImageSize.limitMessage(working: working)
    }
    var orientationPortrait: Bool { aspect == .custom ? custom.height > custom.width : portrait }

    /// Width over height of the locked ratio, as shown; nil for freeform.
    var ratio: CGFloat? {
        if aspect == .custom { return custom.width > 0 && custom.height > 0 ? custom.width / custom.height : nil }
        guard let ratio = aspect.ratio(photo: photo) else { return nil }
        return portrait ? 1 / ratio : ratio
    }

    /// The rectangle in pixels of the photograph as shown.
    var shown: CGRect {
        get {
            let r = CropSettings.shown(rect, rotation: rotation)
            return CGRect(x: r.minX * photo.width, y: r.minY * photo.height, width: r.width * photo.width, height: r.height * photo.height)
        }
        set {
            guard photo.width > 0, photo.height > 0 else { return }
            let r = CGRect(x: newValue.minX / photo.width, y: newValue.minY / photo.height, width: newValue.width / photo.width, height: newValue.height / photo.height)
            rect = CropSettings.unrotated(r, rotation: rotation)
        }
    }
    /// Where the rectangle can go: the turned photograph's bounding box.
    var bounds: CGRect { CropGeometry.bounds(of: photo, angle: angle) }

    /// A newer render: a quarter turn swaps the locked ratio's orientation,
    /// as the rectangle turns with the photograph.
    func update(photo: CGSize, rotation: Int) {
        if (rotation - self.rotation) % 2 != 0 {
            portrait.toggle(); custom = CGSize(width: custom.height, height: custom.width)
        }
        self.photo = photo; self.rotation = rotation
    }

    func choose(_ aspect: CropAspect) {
        self.aspect = aspect
        if aspect == .original { portrait = photo.height > photo.width }
        refit()
    }
    func orient(portrait: Bool) {
        if aspect == .custom {
            if (custom.height > custom.width) != portrait { custom = CGSize(width: custom.height, height: custom.width) }
        } else { self.portrait = portrait }
        refit()
    }
    func setCustom(width: CGFloat? = nil, height: CGFloat? = nil) {
        if let width { custom.width = min(max(width, 0.1), 1000) }
        if let height { custom.height = min(max(height, 0.1), 1000) }
        refit()
    }
    func reset() {
        rect = CropSettings.full; aspect = .original; angle = 0
        portrait = photo.height > photo.width
    }

    func beginStraightening() {
        if intended == nil { intended = shown }
        straightening = true
    }
    /// Turns the photograph, shrinking the crop as needed to stay on it.
    func straighten(to degrees: Double) {
        let base = intended ?? shown
        angle = min(max((degrees * 10).rounded() / 10, -CropSettings.maxAngle), CropSettings.maxAngle)
        shown = CropGeometry.inscribe(base, angle: angle, photo: photo)
    }
    func endStraightening() {
        intended = nil
        withAnimation(.easeOut(duration: 0.2)) { straightening = false }
    }

    /// The largest rectangle with the locked ratio, around the current centre.
    private func refit() {
        guard let ratio else { return }
        let current = shown
        let fitted = CropGeometry.fit(ratio: ratio, around: CGPoint(x: current.midX, y: current.midY), in: photo)
        shown = CropGeometry.inscribe(fitted, angle: angle, photo: photo)
    }
}

/// The whole photograph with the crop drawn over it (§4.10). It sits where
/// the print does, between the caption, which stays, and the label's band,
/// which the straightening dial takes. Borders aren't shown, and the canvas
/// doesn't zoom while cropping.
struct CropEditor: View {
    let model: EditorModel
    let session: CropSession
    let image: AnyObject

    @State private var drag: (handle: CropGeometry.Handle, start: CGRect)?

    /// The photograph's place: fitted between the caption's and the label's
    /// bands and the canvas's side margins, as the print is.
    static func place(photo: CGSize, in size: CGSize) -> CGRect {
        let inset = ImageScrollView.fitInset, top = CanvasArea.captionBand, bottom = CanvasArea.labelBand
        let room = CGSize(width: max(size.width - 2 * inset, 1), height: max(size.height - top - bottom, 1))
        let scale = photo.width > 0 && photo.height > 0 ? min(room.width / photo.width, room.height / photo.height) : 1
        let fitted = CGSize(width: photo.width * scale, height: photo.height * scale)
        return CGRect(x: ((size.width - fitted.width) / 2).rounded(), y: (top + (room.height - fitted.height) / 2).rounded(),
                      width: fitted.width, height: fitted.height)
    }

    var body: some View {
        GeometryReader { geometry in
            let frame = Self.place(photo: session.photo, in: geometry.size)
            let scale = session.photo.width > 0 ? frame.width / session.photo.width : 1
            let shown = session.shown
            let crop = CGRect(x: frame.minX + shown.minX * scale, y: frame.minY + shown.minY * scale,
                              width: shown.width * scale, height: shown.height * scale)
            ZStack(alignment: .topLeading) {
                CropPhoto(image: image, frame: frame, rotation: session.rotation, angle: session.angle)
                CropOverlay(photo: CropGeometry.corners(of: frame.size, angle: session.angle).map { CGPoint(x: $0.x + frame.minX, y: $0.y + frame.minY) },
                            crop: crop, fine: session.straightening)
            }
            .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
            .contentShape(Rectangle())
            .gesture(dragGesture(crop: crop, scale: scale))
            .onContinuousHover { phase in
                guard drag == nil else { return }
                switch phase {
                case .active(let point): Self.cursor(for: CropGeometry.handle(at: point, frame: crop)).set()
                case .ended: NSCursor.arrow.set()
                }
            }
            .overlay(alignment: .topLeading) { ZStack(alignment: .topLeading) {
                let left = max(frame.minX, ImageScrollView.fitInset)
                PhotoCaption(facts: model.photoFacts, fileName: model.sourceItem.map { model.library.name(of: $0) } ?? "",
                             portrait: frame.height > frame.width)
                    .frame(width: max(min(frame.maxX, geometry.size.width - ImageScrollView.fitInset) - left, 1), height: 40, alignment: .bottom)
                    .offset(x: left, y: frame.minY - 10 - 40)
                    .allowsHitTesting(false)
                StraightenDial(angle: session.angle, began: session.beginStraightening, changed: session.straighten,
                               ended: session.endStraightening, turn: model.turn(clockwise:))
                    .offset(x: (frame.midX - StraightenDial.width / 2).rounded(), y: frame.maxY + 10)
            } }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Crop")
    }

    private func dragGesture(crop: CGRect, scale: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if drag == nil {
                    guard let handle = CropGeometry.handle(at: value.startLocation, frame: crop) else { return }
                    drag = (handle, session.shown)
                    session.adjusting = true
                    if handle == .move { NSCursor.closedHand.set() }
                }
                guard let (handle, start) = drag else { return }
                let offset = CGSize(width: value.translation.width / scale, height: value.translation.height / scale)
                let (photo, angle, bounds) = (session.photo, session.angle, session.bounds)
                if handle == .move {
                    // One axis at a time, so the crop slides along the turned photograph's edges.
                    let across = CropGeometry.constrain(from: start, to: CropGeometry.move(start, by: CGSize(width: offset.width, height: 0), in: bounds), angle: angle, photo: photo)
                    session.shown = CropGeometry.constrain(from: across, to: CropGeometry.move(across, by: CGSize(width: 0, height: offset.height), in: bounds), angle: angle, photo: photo)
                } else {
                    // Follow the edge being dragged, not the pointer's distance from it.
                    let edge = CGPoint(x: handle.x < 0 ? start.minX : start.maxX, y: handle.y < 0 ? start.minY : start.maxY)
                    let resized = CropGeometry.resize(start, handle: handle, to: CGPoint(x: edge.x + offset.width, y: edge.y + offset.height),
                                                      ratio: session.ratio, in: bounds, minimum: 40 / scale)
                    session.shown = CropGeometry.constrain(from: start, to: resized, angle: angle, photo: photo)
                }
            }
            .onEnded { _ in
                if drag?.handle == .move { NSCursor.openHand.set() }
                drag = nil
                withAnimation(.easeOut(duration: 0.2)) { session.adjusting = false }
            }
    }

    static func cursor(for handle: CropGeometry.Handle?) -> NSCursor {
        guard let handle else { return .arrow }
        if handle == .move { return .openHand }
        if #available(macOS 15, *) {
            let position: NSCursor.FrameResizePosition = switch (handle.x, handle.y) {
            case (-1, -1): .topLeft
            case (1, -1): .topRight
            case (-1, 1): .bottomLeft
            case (1, 1): .bottomRight
            case (-1, _): .left
            case (1, _): .right
            case (_, -1): .top
            default: .bottom
            }
            return .frameResize(position: position, directions: .all)
        }
        return handle.corner ? .crosshair : handle.x != 0 ? .resizeLeftRight : .resizeUpDown
    }
}

/// Outside the crop the photograph darkens with `rgba(8,7,6,.6)`; the crop is
/// a 1 pt `paper` line with the rule of thirds in faint `paper` and 3 pt
/// `paper` corners just outside it. Straightening, a finer grid shows.
private struct CropOverlay: View {
    /// The turned photograph's corners.
    let photo: [CGPoint]
    let crop: CGRect
    let fine: Bool
    @Environment(\.sheetInk) private var ink

    static let shade = Color(red: 8 / 255, green: 7 / 255, blue: 6 / 255).opacity(0.6)

    var body: some View {
        Canvas { context, _ in
            var outside = Path()
            outside.addLines(photo); outside.closeSubpath()
            outside.addRect(crop)
            context.fill(outside, with: .color(Self.shade), style: FillStyle(eoFill: true))
            let lines = fine ? 9 : 3
            var grid = Path()
            for i in 1..<lines {
                let x = (crop.minX + crop.width * CGFloat(i) / CGFloat(lines)).rounded() + 0.5
                let y = (crop.minY + crop.height * CGFloat(i) / CGFloat(lines)).rounded() + 0.5
                grid.move(to: CGPoint(x: x, y: crop.minY)); grid.addLine(to: CGPoint(x: x, y: crop.maxY))
                grid.move(to: CGPoint(x: crop.minX, y: y)); grid.addLine(to: CGPoint(x: crop.maxX, y: y))
            }
            context.stroke(grid, with: .color(Theme.paper.opacity(0.3)), lineWidth: 1)
            context.stroke(Path(crop.insetBy(dx: -0.5, dy: -0.5)), with: .color(Theme.paper.opacity(0.9)), lineWidth: 1)
            // The corners, 20 pt along each edge, just outside the line.
            let length = min(20, crop.width / 2, crop.height / 2), width: CGFloat = 3
            let outer = crop.insetBy(dx: -width / 2, dy: -width / 2)
            var corners = Path()
            for (x, y, dx, dy) in [(outer.minX, outer.minY, 1.0, 1.0), (outer.maxX, outer.minY, -1, 1), (outer.minX, outer.maxY, 1, -1), (outer.maxX, outer.maxY, -1, -1)] {
                corners.move(to: CGPoint(x: x + dx * length, y: y))
                corners.addLine(to: CGPoint(x: x, y: y))
                corners.addLine(to: CGPoint(x: x, y: y + dy * length))
            }
            // Uncropped, the corners lie on the sheet: on a light one, edged in its ink.
            if !ink.light {
                context.stroke(corners, with: .color(ink.paper), style: StrokeStyle(lineWidth: width + 2, lineCap: .square))
            }
            context.stroke(corners, with: .color(Theme.paper), style: StrokeStyle(lineWidth: width, lineCap: .square))
        }
        .allowsHitTesting(false)
    }
}

/// The straightening dial, 362 wide, where the label was (§4.10): the angle
/// in Newsreader 15 over 61 ticks, a degree apart, every fifth longer, and
/// zero a 2 × 17 yellow line. Drag it sideways; it catches at zero, and a
/// double-click straightens again. A quarter-turn arrow at each end turns
/// the photograph: rotate exists only here.
struct StraightenDial: View {
    let angle: Double
    let began: () -> Void
    let changed: (Double) -> Void
    let ended: () -> Void
    /// A quarter turn, clockwise or not.
    let turn: (Bool) -> Void

    @State private var start: Double?
    @Environment(\.sheetInk) private var ink
    static let width: CGFloat = 362
    /// Points per degree: 61 ticks across the dial.
    static let spacing = (width - 2) / 60

    /// "0,0°", in the reader's own decimal mark.
    static func degrees(_ angle: Double) -> String { "\((angle == 0 ? 0 : angle).formatted(.number.precision(.fractionLength(1))))°" }

    /// The ticks to draw for `angle`: each degree's x across the dial, and
    /// whether it's a fifth's or zero's.
    static func ticks(angle: Double) -> [(degree: Int, x: CGFloat)] {
        let mid = width / 2
        return (-Int(CropSettings.maxAngle) - 30...Int(CropSettings.maxAngle) + 30).compactMap { degree in
            let x = mid + (CGFloat(degree) - CGFloat(angle)) * spacing
            return x >= 0 && x <= width ? (degree: degree, x: x) : nil
        }
    }

    var body: some View {
        VStack(spacing: 4) {
            Text(Self.degrees(angle))
                .font(Theme.serif(15)).monospacedDigit()
                .foregroundStyle(ink.paper)
                .accessibilityHidden(true)
            Canvas { context, size in
                for (degree, x) in Self.ticks(angle: angle) {
                    let zero = degree == 0, major = degree % 5 == 0
                    let rect = zero ? CGRect(x: x - 1, y: 0, width: 2, height: 17)
                        : CGRect(x: (x - 0.5).rounded(), y: 0, width: 1, height: major ? 11 : 6)
                    let shape = zero ? Path(roundedRect: rect, cornerRadius: 1) : Path(rect)
                    context.fill(shape, with: .color(zero ? ink.accent : major ? ink.dim : ink.track))
                }
            }
            .frame(width: Self.width, height: 18)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { value in
                        if start == nil { start = angle; began() }
                        var next = (start ?? 0) - value.translation.width / Self.spacing
                        // Catch at zero, with a tap on a Force Touch trackpad.
                        if abs(next) < 0.4 {
                            if angle != 0 { NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .now) }
                            next = 0
                        }
                        changed(next)
                    }
                    .onEnded { _ in start = nil; ended() }
            )
            .onTapGesture(count: 2) { began(); changed(0); ended() }
            .onHover { inside in (inside ? NSCursor.resizeLeftRight : NSCursor.arrow).set() }
            .help("Drag to straighten the photograph. Double-click to straighten it again.")
            // A slider to VoiceOver, in half degrees.
            .accessibilityRepresentation {
                Slider(value: Binding(get: { angle }, set: { began(); changed($0); ended() }),
                       in: -CropSettings.maxAngle...CropSettings.maxAngle, step: 0.5) { Text("Straighten") }
                    .accessibilityValue("\(Self.degrees(angle).dropLast()) degrees")
            }
            .overlay(alignment: .topLeading) {
                QuarterTurn(clockwise: false, action: { turn(false) }).offset(x: -34, y: -4)
            }
            .overlay(alignment: .topTrailing) {
                QuarterTurn(clockwise: true, action: { turn(true) }).offset(x: 34, y: -4)
            }
        }
        .frame(width: Self.width)
    }
}

/// A quarter-turn arrow at the dial's end: `dim`, `paper` under the pointer.
private struct QuarterTurn: View {
    let clockwise: Bool
    let action: () -> Void
    @State private var hovered = false
    @Environment(\.sheetInk) private var ink

    var body: some View {
        Button(action: action) {
            Image(systemName: clockwise ? "arrow.clockwise" : "arrow.counterclockwise")
                .font(.system(size: 14, weight: .regular))
                .foregroundStyle(hovered ? ink.paper : ink.dim)
                .frame(width: 24, height: 24)
                .contentShape(Rectangle())
        }
        .buttonStyle(InkPressStyle())
        .onHover { hovered = $0 }
        .help(clockwise ? "Rotate right (⌘R)" : "Rotate left (⌥⌘R)")
        .accessibilityLabel(clockwise ? "Rotate right" : "Rotate left")
    }
}

/// The developed photograph at `frame`, turned like the canvas and then
/// straightened by `angle`.
private struct CropPhoto: NSViewRepresentable {
    let image: AnyObject
    let frame: CGRect
    let rotation: Int
    let angle: Double

    func makeNSView(context: Context) -> CropPhotoView { CropPhotoView() }
    func updateNSView(_ view: CropPhotoView, context: Context) { view.show(image, frame: frame, rotation: rotation, angle: angle) }
}

final class CropPhotoView: NSView {
    private let photo = CALayer()
    private var frameRect = CGRect.zero
    private var rotation = 0
    private var angle = 0.0
    override var wantsUpdateLayer: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        photo.contentsGravity = .resize
        photo.minificationFilter = .trilinear
        photo.magnificationFilter = .linear
        photo.isOpaque = true
        photo.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull(), "transform": NSNull()]
        layer?.addSublayer(photo)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    /// `frame` is in SwiftUI's top-left coordinates.
    func show(_ image: AnyObject, frame: CGRect, rotation: Int, angle: Double) {
        guard image !== photo.contents as AnyObject? || frame != frameRect || rotation != self.rotation || angle != self.angle else { return }
        photo.contents = image; frameRect = frame; self.rotation = rotation; self.angle = angle
        needsLayout = true
    }

    override func layout() {
        super.layout()
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let size = rotation % 2 != 0 ? CGSize(width: frameRect.height, height: frameRect.width) : frameRect.size
        photo.bounds = CGRect(origin: .zero, size: size)
        // Layers are y-up, so clockwise is a negative angle.
        photo.position = CGPoint(x: frameRect.midX, y: bounds.height - frameRect.midY)
        photo.setAffineTransform(CGAffineTransform(rotationAngle: -.pi / 2 * CGFloat(rotation) - angle * .pi / 180))
        CATransaction.commit()
    }
}

/// The crop panel takes the inspector while cropping (§4.10): "Crop" with
/// Reset Crop, the aspect ratios as rows with a dot, Orientation in words,
/// Straighten, the Size and hints, and Cancel and Done at the foot.
struct CropInspector: View {
    let model: EditorModel
    @Bindable var session: CropSession

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 16) {
                Text("Crop").font(Theme.ui(12, weight: .medium)).foregroundStyle(Theme.paper)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
                WordButton(title: "Reset Crop", help: "Show the whole, unturned photograph again.", action: session.reset)
                    .disabled(!session.modified)
            }
            .padding(.horizontal, Theme.Size.gutter)
            .padding(.top, 13)
            .frame(height: Theme.Size.paneHeader, alignment: .top)
            InkRule()
            ScrollView {
                VStack(spacing: 0) {
                    InkSection(title: "Aspect Ratio") {
                        ForEach(CropAspect.allCases) { aspect in
                            AspectRow(aspect: aspect, chosen: session.aspect == aspect) { session.choose(aspect) }
                        }
                        if session.aspect == .custom { CustomRatioRow(session: session) }
                        OrientationRow(portrait: session.orientationPortrait) { session.orient(portrait: $0) }
                            .inkDisabled(!session.aspect.turns)
                    }
                    InkSection(title: "Straighten") {
                        LabeledSlider(title: "Angle", value: Binding(get: { session.angle }, set: session.straighten),
                                      range: -CropSettings.maxAngle...CropSettings.maxAngle, step: 0.1, unit: "°", defaultValue: 0,
                                      modified: session.angle != 0,
                                      reset: { session.beginStraightening(); session.straighten(to: 0); session.endStraightening() },
                                      editing: { $0 ? session.beginStraightening() : session.endStraightening() },
                                      help: "Turns the photograph to level a crooked horizon. The crop shrinks to stay on the photograph.")
                    }
                    InkSection(title: nil) {
                        let size = session.exportedSize(working: model.workingSize)
                        ReadoutRow(title: "Size", value: "\(Int(size.width.rounded())) × \(Int(size.height.rounded())) px")
                        HintText("The exported photograph’s size, before the border.")
                        if let working = session.projectedWorking, session.source.width > 0 {
                            let scale = working.width / session.source.width
                            HintText(scale >= 1 ? "Image Size develops the whole frame \(scale.formatted(FloatingPointFormatStyle<Double>.number.precision(.fractionLength(1))))× the source." : "Image Size develops the whole frame at \(EditorModel.percent(scale)) of the source.")
                        }
                        if let error = session.sizeError { HintText(error, warning: true) }
                        HintText("Drag the corners or edges to crop, or inside to move it. Drag the dial under the photograph to straighten it; the arrows at its ends turn it a quarter. The border is added after cropping.")
                    }
                }
            }
            .scrollIndicators(.never)
            VStack(spacing: 8) {
                Text("return applies · esc cancels")
                    .font(Theme.serif(14, italic: true)).foregroundStyle(Theme.label).lineLimit(1)
                // Done is twice Cancel's width.
                GeometryReader { proxy in
                    let third = max((proxy.size.width - 10) / 3, 1)
                    HStack(spacing: 10) {
                        Button("Cancel") { model.endCrop(apply: false) }
                            .buttonStyle(SquareOutlinedButtonStyle())
                            .frame(width: third)
                            .help("Discard the crop (Esc)")
                        Button("Done") { model.endCrop(apply: true) }
                            .buttonStyle(PencilButtonStyle())
                            .frame(width: third * 2)
                            .help("Apply the crop (Return)")
                    }
                }
                .frame(height: 36)
            }
            .padding(.horizontal, Theme.Size.gutter)
            .padding(.top, 11)
            .padding(.bottom, 16)
            .overlay(alignment: .top) { InkRule() }
        }
        // One named group, which also keeps its rules out of VoiceOver.
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Crop")
    }
}

/// An aspect ratio as a row: a dot, filled in `paper` when it's chosen, the
/// ratio, and its film or print format in italic on the right.
private struct AspectRow: View {
    let aspect: CropAspect
    let chosen: Bool
    let choose: () -> Void
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 10) {
            Circle()
                .strokeBorder(chosen ? Theme.paper : hovered ? Theme.dim : Theme.track, lineWidth: 1.2)
                .background(Circle().fill(chosen ? Theme.paper : .clear))
                .frame(width: 9, height: 9)
            RowLabel(title: aspect.title, bright: chosen || hovered)
            Spacer(minLength: 8)
            if let format = aspect.format {
                Text(format).font(Theme.serif(13.5, italic: true)).foregroundStyle(Theme.dim)
            }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row - 3)
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture(perform: choose)
        .inkFocus(activate: choose)
        .accessibilityElement()
        .accessibilityLabel([aspect.title, aspect.format].compactMap { $0 }.joined(separator: ", "))
        .accessibilityAddTraits(chosen ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction(.default, choose)
    }
}

/// Landscape and Portrait as two words, the chosen one in `paper` with the
/// pencil under it.
private struct OrientationRow: View {
    let portrait: Bool
    let choose: (Bool) -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RowLabel(title: "Orientation")
            Spacer(minLength: 8)
            HStack(alignment: .firstTextBaseline, spacing: 14) {
                word("Landscape", portrait: false)
                word("Portrait", portrait: true)
            }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row + 4)
    }

    private func word(_ title: String, portrait: Bool) -> some View {
        let chosen = self.portrait == portrait
        return Button { choose(portrait) } label: {
            Text(title).font(Theme.serif(14.5)).foregroundStyle(chosen ? Theme.paper : Theme.dim)
                .pencilUnderline(chosen, offset: 3)
        }
        .buttonStyle(InkPressStyle())
        .inkFocus { choose(portrait) }
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

/// Custom's ratio, "3 × 2": click a side to type it.
private struct CustomRatioRow: View {
    let session: CropSession

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RowLabel(title: "Ratio")
            Spacer(minLength: 8)
            RatioSide(title: "Width", value: session.custom.width, typing: { session.typing = $0 }) { session.setCustom(width: $0) }
            Text("×").font(Theme.serif(14.5)).foregroundStyle(Theme.dim)
            RatioSide(title: "Height", value: session.custom.height, typing: { session.typing = $0 }) { session.setCustom(height: $0) }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row)
    }
}

/// One side of the custom ratio, in the serif; a click turns it into a field.
private struct RatioSide: View {
    let title: String
    let value: CGFloat
    let typing: (Bool) -> Void
    let set: (CGFloat) -> Void
    @State private var editing = false
    @State private var pointed = false

    var body: some View {
        if editing {
            ValueField(title: "\(title) ratio", value: Binding(get: { Double(value) }, set: { set(CGFloat($0)) }), range: 0.1...1000, decimals: 2) { _ in
                editing = false; typing(false)
            }
            .frame(width: 44, height: 20)
            .overlay(alignment: .bottom) { Rectangle().fill(Theme.paper).frame(height: 1) }
        } else {
            Text(Double(value).formatted(.number.precision(.fractionLength(0...2))))
                .font(Theme.serif(14.5)).foregroundStyle(Theme.paper).monospacedDigit()
                .overlay(alignment: .bottom) { Rectangle().fill(pointed ? Theme.dim : Theme.track).frame(height: 1).offset(y: 2) }
                .contentShape(Rectangle())
                .onHover { pointed = $0 }
                .onTapGesture { editing = true; typing(true) }
                .inkFocus { editing = true; typing(true) }
                .help("Click to type the \(title.lowercased())")
                .accessibilityLabel("\(title) ratio")
                .accessibilityValue(Double(value).formatted())
                .accessibilityAddTraits(.isButton)
        }
    }
}

/// Crop's Cancel: outlined like the other buttons, but square-cornered and
/// 36 tall beside Done.
struct SquareOutlinedButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { Outlined(configuration: configuration) }

    private struct Outlined: View {
        let configuration: Configuration
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false

        var body: some View {
            configuration.label
                .font(Theme.serif(16, weight: 500))
                .foregroundStyle(Theme.paper)
                .frame(maxWidth: .infinity, minHeight: 36, maxHeight: 36)
                .padding(.bottom, 1)
                .background(RoundedRectangle(cornerRadius: 3).fill(Theme.paper.opacity(configuration.isPressed ? 0.09 : hovered ? 0.04 : 0)))
                .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(hovered || configuration.isPressed ? Theme.dim : Theme.track, lineWidth: 1))
                .contentShape(Rectangle())
                .opacity(enabled ? 1 : Theme.disabledOpacity)
                .onHover { hovered = $0 && enabled }
        }
    }
}
