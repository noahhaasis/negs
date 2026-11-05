import SwiftUI

/// The folder as a roll of 35mm film. The strip artwork runs
/// across, so the roll turns it a quarter clockwise to run down the window.
enum FilmRollLayout {
    static let stripWidth: CGFloat = 150
    /// The column is the film and nothing else.
    static var columnWidth: CGFloat { stripWidth }
    /// The artwork's film base, for frames before it has loaded.
    static let filmBase = Color(red: 15.0 / 255, green: 13.0 / 255, blue: 12.0 / 255)
    /// Until the artwork loads: its proportions and gate, from `frames.json`.
    static let defaultTexture = CGSize(width: 3800, height: 3500)
    static let defaultWindow = CGRect(x: 0.026316, y: 0.157143, width: 0.947368, height: 0.685714)

    /// How long one frame is on the roll. Whole points, so no light shows between frames.
    static func frameHeight(stripWidth: CGFloat, texture: CGSize) -> CGFloat {
        (stripWidth * texture.width / texture.height).rounded()
    }

    /// Where the photograph shows, in a frame of `frame` on the roll, from the
    /// artwork's `window` (fractions of it, from its top left).
    static func gate(window: CGRect, frame: CGSize) -> CGRect {
        CGRect(x: (1 - window.maxY) * frame.width, y: window.minX * frame.height,
               width: window.height * frame.width, height: window.width * frame.height)
    }

    /// A landscape photograph lies on its side on a roll running down, as on
    /// real film; a portrait one stands up.
    static func liesOnItsSide(_ image: CGSize) -> Bool { image.width > image.height }

    /// How many blank frames keep the film going to the bottom of a column
    /// `viewport` tall, after `photos` frames.
    static func blankFrames(photos: Int, viewport: CGFloat, frameHeight: CGFloat) -> Int {
        guard frameHeight > 0 else { return 0 }
        return max(0, Int((viewport / frameHeight).rounded(.up)) - photos)
    }

    /// Still films print a frame number, and the artwork's is always the same
    /// one. Cine and print films have other text in those places.
    static func printsFrameNumbers(film: String) -> Bool {
        !(film.hasPrefix("kodak_vision3") || ["kodak_doublex", "kodak_2302", "kodak_2383", "kodak_2393", "kodak_verita_200d"].contains(film))
    }
}

/// One frame of the roll: the photograph, with the strip's artwork over it.
struct FilmFrame: View {
    let file: PhotoFile
    /// Its place on the roll, from 1.
    let number: Int
    let selected: Bool
    let strip: FilmStrip?
    let film: String
    let open: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    private var texture: CGSize { strip?.pixelSize ?? FilmRollLayout.defaultTexture }

    var body: some View {
        let width = FilmRollLayout.stripWidth
        let height = FilmRollLayout.frameHeight(stripWidth: width, texture: texture)
        let gate = FilmRollLayout.gate(window: strip?.window ?? FilmRollLayout.defaultWindow, frame: CGSize(width: width, height: height))
        Button(action: open) {
            ZStack(alignment: .topLeading) {
                photograph(in: gate)
                FilmArtwork(number: number, strip: strip, film: film)
                if selected {
                    // A grease pencil's mark, as on a contact sheet.
                    RoundedRectangle(cornerRadius: 4)
                        .strokeBorder(Color(red: 0.88, green: 0.14, blue: 0.1), lineWidth: 2.5)
                        .frame(width: gate.width + 6, height: gate.height + 6)
                        .offset(x: gate.minX - 3, y: gate.minY - 3)
                }
            }
            .frame(width: width, height: height, alignment: .topLeading)
            .clipped()
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(file.name)
        .accessibilityLabel(file.name)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
        .onAppear { visible = true }
        .onDisappear { visible = false; image = nil }
        .task(id: visible ? file : nil) {
            guard visible else { return }
            let request = Task.detached(priority: .utility) { await PhotoThumbnails.shared.image(for: file) }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            image = result
        }
    }

    @ViewBuilder private func photograph(in gate: CGRect) -> some View {
        ZStack {
            Color(white: 0.16)
            if let image {
                let sideways = FilmRollLayout.liesOnItsSide(CGSize(width: image.width, height: image.height))
                Image(decorative: image, scale: 1)
                    .resizable()
                    .scaledToFill()
                    .frame(width: sideways ? gate.height : gate.width, height: sideways ? gate.width : gate.height)
                    .clipped()
                    .rotationEffect(.degrees(sideways ? 90 : 0))
            }
        }
        .frame(width: gate.width, height: gate.height)
        .clipped()
        .offset(x: gate.minX, y: gate.minY)
    }
}

/// A frame past the last photograph: the roll carries on, with nothing on it.
struct BlankFilmFrame: View {
    let number: Int
    let strip: FilmStrip?
    let film: String

    var body: some View {
        let width = FilmRollLayout.stripWidth
        let height = FilmRollLayout.frameHeight(stripWidth: width, texture: strip?.pixelSize ?? FilmRollLayout.defaultTexture)
        let gate = FilmRollLayout.gate(window: strip?.window ?? FilmRollLayout.defaultWindow, frame: CGSize(width: width, height: height))
        ZStack(alignment: .topLeading) {
            Color(white: 0.16)
                .frame(width: gate.width, height: gate.height)
                .offset(x: gate.minX, y: gate.minY)
            FilmArtwork(number: number, strip: strip, film: film)
        }
        .frame(width: width, height: height, alignment: .topLeading)
        .clipped()
        .accessibilityHidden(true)
    }
}

/// The strip's artwork for one frame of the roll.
private struct FilmArtwork: View {
    let number: Int
    let strip: FilmStrip?
    let film: String

    /// Laid out across, as the artwork is, then turned with it.
    var body: some View {
        let width = FilmRollLayout.stripWidth
        let height = FilmRollLayout.frameHeight(stripWidth: width, texture: strip?.pixelSize ?? FilmRollLayout.defaultTexture)
        if let strip {
            ZStack(alignment: .topLeading) {
                Image(decorative: strip.image, scale: 1)
                    .resizable()
                    .frame(width: height, height: width)
                if FilmRollLayout.printsFrameNumbers(film: film) {
                    numbers(length: height, width: width)
                }
            }
            .frame(width: height, height: width)
            .rotationEffect(.degrees(90))
            .frame(width: width, height: height)
            .allowsHitTesting(false)
        } else {
            // The film, with its window cut by the gate, until the artwork loads.
            Rectangle()
                .fill(FilmRollLayout.filmBase)
                .reverseMask {
                    let gate = FilmRollLayout.gate(window: FilmRollLayout.defaultWindow, frame: CGSize(width: width, height: height))
                    Rectangle().frame(width: gate.width, height: gate.height).position(x: gate.midX, y: gate.midY)
                }
                .frame(width: width, height: height)
                .allowsHitTesting(false)
        }
    }

    /// This frame's own number over the artwork's: in millimetres along and
    /// across the 38 × 35 mm strip, where `spektrafilm-io::film35` prints it.
    @ViewBuilder private func numbers(length: CGFloat, width: CGFloat) -> some View {
        let scale = length / 38
        let label = { (text: String, x: CGFloat, y: CGFloat) in
            Text(text)
                .font(.system(size: 1.2 * scale, weight: .semibold).width(.condensed))
                .foregroundStyle(Color(red: 0.97, green: 0.63, blue: 0.2))
                .fixedSize()
                .frame(width: 2.2 * scale, height: 1.4 * scale, alignment: .leading)
                .background(FilmRollLayout.filmBase)
                .offset(x: x * scale, y: y * scale - 0.7 * scale)
        }
        label("\(number)", 2.3, 1.1)
        label("\(number)", 2.3, 33.95)
        label("\(number)A", 21.1, 33.95)
    }
}

private extension View {
    /// Cuts `mask` out of this view.
    func reverseMask<Mask: View>(@ViewBuilder _ mask: () -> Mask) -> some View {
        self.mask {
            Rectangle().overlay { mask().blendMode(.destinationOut) }.compositingGroup()
        }
    }
}
