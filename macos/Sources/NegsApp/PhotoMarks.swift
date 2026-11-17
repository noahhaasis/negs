import SwiftUI

/// A thumbnail's marks in the strip and the grid (§4.4, round 7): stars drawn
/// by hand at the bottom left (`HandStar`, round 22), a reject's hand-drawn X from corner to corner
/// (`CrossMark`), and the yellow dot of an edited photograph at the top
/// right. Each is its own shape, never only a colour, in cream with a dark
/// halo so it reads on a bright sky. A RAW with the camera's JPEG beside it
/// gets a small badge at the bottom right, "+JPG" (round 31, C): the one
/// mark that isn't ink. Unrated, unflagged, unedited, unpaired
/// photographs show none. Lay it over the picture. A pick's box (round 20)
/// reaches past the picture, so the grid and the strip draw it in a layer
/// above the thumbnails (`BoxInk`).
struct PhotoMarks: View {
    let summary: LibrarySummary
    /// The file's name: it seeds the reject's cross and the stars, so
    /// each is drawn a little differently, and the same way every time.
    let name: String
    /// The RAW's camera JPEG (or HEIF), shown as the badge.
    var cameraJPEG: URL? = nil
    /// The strip's frames are smaller, and can be narrow portraits with five
    /// stars: there the badge is shortened to "+J".
    var inStrip = false

    var body: some View {
        ZStack {
            if summary.flag == .reject {
                CrossMark(name: name)
            }
            if summary.rating > 0 {
                HStack(spacing: HandStar.gap) {
                    ForEach(0..<min(summary.rating, 5), id: \.self) { index in
                        HandStarShape(name: name, index: index)
                            .fill(Theme.paper)
                            .frame(width: HandStar.size, height: HandStar.size)
                    }
                }
                .padding([.leading, .bottom], 10)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // The halo: drop-shadow 1.5 pt black at 0.95, then 4 pt at 0.45.
        .shadow(color: .black.opacity(0.95), radius: 0.75)
        .shadow(color: .black.opacity(0.45), radius: 2)
        .overlay(alignment: .topTrailing) {
            // The same dot that marks edits in the panel; no halo.
            if summary.edited {
                Circle().fill(Theme.accent).frame(width: 7, height: 7).padding([.top, .trailing], 11)
            }
        }
        .overlay(alignment: .bottomTrailing) {
            if let cameraJPEG {
                PairBadge(word: PairBadge.word(for: cameraJPEG, short: inStrip))
                    .padding([.bottom, .trailing], inStrip ? 5 : 7)
            }
        }
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }

    /// The picture's opacity under the marks: rejects are greyed, more so
    /// in the strip (0.22, or 0.5 for the current frame) than the grid (0.4).
    static func rejectOpacity(inStrip: Bool, current: Bool) -> Double {
        inStrip ? (current ? 0.5 : 0.22) : 0.4
    }

    /// `cameraJPEG`: the RAW's camera JPEG, which goes with it.
    static func help(_ name: String, summary: LibrarySummary, readOnly: Bool, cameraJPEG: URL? = nil) -> String {
        var lines = [name]
        if let cameraJPEG { lines.append("With the camera’s JPEG, \(cameraJPEG.lastPathComponent)") }
        if let problem = summary.problem { lines.append(problem) }
        if readOnly { lines.append("Ratings and edits can’t be saved in this folder: it’s read-only.") }
        return lines.joined(separator: "\n")
    }

    static func accessibilityLabel(_ name: String, summary: LibrarySummary, cameraJPEG: URL? = nil) -> String {
        var parts = [name]
        if cameraJPEG != nil { parts.append("with camera JPEG") }
        if summary.rating > 0 { parts.append(summary.rating == 1 ? "1 star" : "\(summary.rating) stars") }
        if let flag = summary.flag { parts.append(flag == .pick ? "Pick" : "Rejected") }
        if summary.edited { parts.append("Edited") }
        return parts.joined(separator: ", ")
    }
}

/// "+JPG" on a dark chip: the RAW has the camera's picture beside it
/// (round 31, C · the boring one, after Lightroom's badge).
struct PairBadge: View {
    let word: String

    var body: some View {
        Text(word)
            .font(.system(size: 8.5, weight: .semibold))
            .tracking(0.5)
            .foregroundStyle(Theme.onPhoto)
            .padding(.horizontal, 4)
            .padding(.top, 3)
            .padding(.bottom, 2.5)
            .background(.black.opacity(0.58), in: RoundedRectangle(cornerRadius: 3))
            .fixedSize()
    }

    /// "+JPG", or "+HEIF" for a HEIF/HEIC/HIF; "+J" / "+H" in the strip.
    static func word(for companion: URL, short: Bool) -> String {
        let heif = ["heic", "heif", "hif"].contains(companion.pathExtension.lowercased())
        return short ? (heif ? "+H" : "+J") : (heif ? "+HEIF" : "+JPG")
    }
}
