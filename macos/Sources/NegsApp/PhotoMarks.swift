import SwiftUI

/// A thumbnail's marks in the strip and the grid (§4.4, round 7): stars at
/// the bottom left, a pick's small tick at the bottom right, a reject's
/// hand-drawn X from corner to corner (`CrossMark`), and the yellow dot of an edited
/// photograph at the top right. Each is its own shape, never only a colour,
/// in cream with a dark halo so it reads on a bright sky. Unrated,
/// unflagged, unedited photographs show none. Lay it over the picture.
struct PhotoMarks: View {
    let summary: LibrarySummary
    /// The file's name: it seeds the reject's cross, so each one is drawn
    /// a little differently, and the same way every time.
    let name: String

    var body: some View {
        ZStack {
            if summary.flag == .reject {
                CrossMark(name: name)
            }
            if summary.flag == .pick {
                TickMark()
                    .stroke(Theme.paper, style: StrokeStyle(lineWidth: 2.2 * 16 / 20, lineCap: .round, lineJoin: .round))
                    .frame(width: 16, height: 16)
                    .padding([.trailing, .bottom], 8)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
            }
            if summary.rating > 0 {
                HStack(spacing: 1.5) {
                    ForEach(0..<min(summary.rating, 5), id: \.self) { _ in
                        StarShape()
                            .fill(Theme.paper)
                            .overlay(StarShape().stroke(Theme.paper, style: StrokeStyle(lineWidth: 1.4 * 11 / 20, lineJoin: .round)))
                            .padding(11 * 2.6 / 20)
                            .frame(width: 11, height: 11)
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
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }

    /// The picture's opacity under the marks: rejects are greyed, more so
    /// in the strip (0.22, or 0.5 for the current frame) than the grid (0.4).
    static func rejectOpacity(inStrip: Bool, current: Bool) -> Double {
        inStrip ? (current ? 0.5 : 0.22) : 0.4
    }

    static func help(_ name: String, summary: LibrarySummary, readOnly: Bool) -> String {
        var lines = [name]
        if let problem = summary.problem { lines.append(problem) }
        if readOnly { lines.append("Ratings and edits can’t be saved in this folder: it’s read-only.") }
        return lines.joined(separator: "\n")
    }

    static func accessibilityLabel(_ name: String, summary: LibrarySummary) -> String {
        var parts = [name]
        if summary.rating > 0 { parts.append(summary.rating == 1 ? "1 star" : "\(summary.rating) stars") }
        if let flag = summary.flag { parts.append(flag == .pick ? "Pick" : "Rejected") }
        if summary.edited { parts.append("Edited") }
        return parts.joined(separator: ", ")
    }
}

/// A pick: `M3.5 10.5 C5.5 12 7 14 8 16.5 C10.5 10 13.5 6 17 3` in a 20 pt
/// box (`--m-tick`), drawn to fill the rect.
struct TickMark: Shape {
    func path(in rect: CGRect) -> Path {
        let sx = rect.width / 20, sy = rect.height / 20
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: rect.minX + x * sx, y: rect.minY + y * sy) }
        var path = Path()
        path.move(to: p(3.5, 10.5))
        path.addCurve(to: p(8, 16.5), control1: p(5.5, 12), control2: p(7, 14))
        path.addCurve(to: p(17, 3), control1: p(10.5, 10), control2: p(13.5, 6))
        return path
    }
}

