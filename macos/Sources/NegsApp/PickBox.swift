import CNegsCore
import SwiftUI

/// The ink drawn round a thumbnail (rounds 20 and 21, `r20-box.html`, B): a
/// pick's cream box and, in the grid, selection's yellow box over it, under
/// the marks' dark halo. Both reach past the picture, over the gaps and the
/// pictures beside it, so the grid and the strip draw this in a layer of its
/// own above every thumbnail. Lay it out at the picture's size: it draws
/// `PickBox.reach` outside it.
struct BoxInk: View {
    let name: String
    /// A pick: the cream box.
    let picked: Bool
    /// Selected or current, in the grid: the yellow box, over the cream one.
    var selected = false
    /// The strip's box: a little closer to the picture, with shorter overshoots.
    var inStrip = false

    var body: some View {
        let plain = Theme.palette.plain
        Canvas { context, size in
            let reach = PickBox.reach
            let picture = CGSize(width: size.width - 2 * reach, height: size.height - 2 * reach)
            context.translateBy(x: reach, y: reach)
            if plain {
                // Plain: straight rounded boxes, no halo.
                func ring(_ outset: CGFloat, _ width: CGFloat, _ color: Color) {
                    let rect = CGRect(origin: .zero, size: picture).insetBy(dx: -outset, dy: -outset)
                    context.stroke(Path(roundedRect: rect, cornerRadius: 4), with: .color(color), lineWidth: width)
                }
                if picked { ring(inStrip ? 2 : 3, 2, Theme.paper) }
                if selected { ring(7, 3, Theme.accent) }
                return
            }
            if picked {
                let box = inStrip ? PickBox.strip(in: picture, name: name) : PickBox.grid(in: picture, name: name)
                context.fill(box.path(in: picture), with: .color(Theme.paper))
            }
            if selected {
                context.fill(PickBox.selection(in: picture, name: name).path(in: picture), with: .color(Theme.accent))
            }
        }
        .padding(-PickBox.reach)
        // The halo: drop-shadow 1.5 pt black at 0.95, then 4 pt at 0.45.
        .shadow(color: .black.opacity(plain ? 0 : 0.95), radius: 0.75)
        .shadow(color: .black.opacity(plain ? 0 : 0.45), radius: 2)
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }
}

/// A box drawn in grease pencil round a picture, as an editor boxes a frame
/// on a contact sheet. Each name draws one of three, with its own corners,
/// bow, wobble, weight and tilt, so no two look alike and the same
/// photograph always gets the same one:
///
/// - **Four:** four separate sides, crossing at the corners.
/// - **Loop:** one stroke round, its end running past its start.
/// - **Two:** down and across, then across and up, meeting at the corners.
///
/// Each stroke is a filled outline whose width follows the pencil's
/// pressure. The geometry is the shared core's (core/src/marks, a port of
/// the page's `boxPaths`, `ribbon` and `smoothPts`, seeded with the reject
/// cross's hash and random numbers over the name and "·box").
enum PickBox {
    enum Kind: Equatable { case four, loop, two }

    struct Box {
        var kind: Kind
        /// Each stroke, before the tilt, in the picture's coordinates.
        var strokes: [Path]
        /// The tilt round the picture's centre, in degrees, clockwise.
        var tilt: CGFloat

        /// The strokes, tilted.
        func path(in size: CGSize) -> Path {
            let transform = CGAffineTransform(translationX: size.width / 2, y: size.height / 2)
                .rotated(by: tilt * .pi / 180)
                .translatedBy(x: -size.width / 2, y: -size.height / 2)
            var path = Path()
            for stroke in strokes { path.addPath(stroke, transform: transform) }
            return path
        }
    }

    /// How far outside the picture a box may draw: the yellow box's 8 pt,
    /// a corner's jitter, an overshoot of up to 12 pt, the stroke's width
    /// and wobble, and the tilt on a wide picture.
    static let reach = CGFloat(CoreMarks.constants.reach)

    /// A pick in the grid: 3 pt out.
    static func grid(in size: CGSize, name: String) -> Box {
        box(CoreMarks.withName(name) { negs_marks_pick_box_grid(size.width, size.height, $0, $1) })
    }
    /// A pick in the strip: 2 pt out, the overshoots kept short.
    static func strip(in size: CGSize, name: String) -> Box {
        box(CoreMarks.withName(name) { negs_marks_pick_box_strip(size.width, size.height, $0, $1) })
    }
    /// Selection in the grid: a second box, 8 pt out, in a 4 pt yellow line
    /// (the page's "Yellow line" over its 2.6 pt stroke).
    static func selection(in size: CGSize, name: String) -> Box {
        box(CoreMarks.withName(name) { negs_marks_pick_box_selection(size.width, size.height, $0, $1) })
    }

    static func seed(_ name: String) -> UInt32 { RejectCross.hash(name + "\u{00B7}box") }

    static func kind(for name: String) -> Kind { kind(CoreMarks.withName(name) { negs_marks_box_kind($0, $1) }) }

    /// The box round a `size` picture, `outset` outside it. `tight` keeps
    /// the corners and overshoots small (the strip); `weight` scales the
    /// stroke.
    static func box(in size: CGSize, outset o: CGFloat, name: String, tight: Bool = false, weight: CGFloat = 1) -> Box {
        box(CoreMarks.withName(name) { negs_marks_pick_box(size.width, size.height, o, $0, $1, tight ? 1 : 0, weight) })
    }

    private static func box(_ strokes: negs_strokes_t) -> Box {
        let block = CoreMarks.take(strokes)
        return Box(kind: kind(block.kind), strokes: block.paths, tilt: CGFloat(block.tilt))
    }

    private static func kind(_ k: UInt32) -> Kind { k == NEGS_BOX_FOUR ? .four : k == NEGS_BOX_LOOP ? .loop : .two }
}
