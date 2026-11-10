import SwiftUI

/// A thumbnail's marks in the strip and the grid (§4.4, round 7): stars at
/// the bottom left, a pick's small flag at the bottom right (round 15), a reject's
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
                FlagMark()
                    .fill(Theme.paper)
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

/// A pick: a small waving flag on a pole (round 15, B), filled, in a 20 pt
/// box drawn to fill the rect:
/// `M4.35 2.6 C4.9 2.4 5.5 2.6 5.55 3.2 C5.6 7.8 5.35 12.6 5.3 17.4 C5.25 17.9 4.75 17.9 4.7 17.4 C4.55 12.6 4.35 7.8 4.35 3.2 Z`
/// (the pole, a little heavier at the top) and
/// `M5.4 3.5 C7.6 2.4 9.9 4.3 12.2 3.7 C13.6 3.3 14.9 2.8 16.2 2.7 C16.5 2.7 16.6 2.9 16.5 3.2 C15.9 5.6 16 8 16.4 10.3 C16.45 10.6 16.3 10.8 16 10.85 C14.6 11.1 13.3 11.6 11.9 11.6 C9.6 11.8 7.6 10.1 5.4 11.2 Z`.
struct FlagMark: Shape {
    func path(in rect: CGRect) -> Path {
        let sx = rect.width / 20, sy = rect.height / 20
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: rect.minX + x * sx, y: rect.minY + y * sy) }
        var path = Path()
        path.move(to: p(4.35, 2.6))
        path.addCurve(to: p(5.55, 3.2), control1: p(4.9, 2.4), control2: p(5.5, 2.6))
        path.addCurve(to: p(5.3, 17.4), control1: p(5.6, 7.8), control2: p(5.35, 12.6))
        path.addCurve(to: p(4.7, 17.4), control1: p(5.25, 17.9), control2: p(4.75, 17.9))
        path.addCurve(to: p(4.35, 3.2), control1: p(4.55, 12.6), control2: p(4.35, 7.8))
        path.closeSubpath()
        path.move(to: p(5.4, 3.5))
        path.addCurve(to: p(12.2, 3.7), control1: p(7.6, 2.4), control2: p(9.9, 4.3))
        path.addCurve(to: p(16.2, 2.7), control1: p(13.6, 3.3), control2: p(14.9, 2.8))
        path.addCurve(to: p(16.5, 3.2), control1: p(16.5, 2.7), control2: p(16.6, 2.9))
        path.addCurve(to: p(16.4, 10.3), control1: p(15.9, 5.6), control2: p(16, 8))
        path.addCurve(to: p(16, 10.85), control1: p(16.45, 10.6), control2: p(16.3, 10.8))
        path.addCurve(to: p(11.9, 11.6), control1: p(14.6, 11.1), control2: p(13.3, 11.6))
        path.addCurve(to: p(5.4, 11.2), control1: p(9.6, 11.8), control2: p(7.6, 10.1))
        path.closeSubpath()
        return path
    }
}
