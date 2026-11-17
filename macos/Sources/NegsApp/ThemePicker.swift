import SwiftUI

/// Settings › Look › Theme: the four themes as small windows in a row, the
/// chosen one ringed in the pencil (design/proposals/r28-themes.html, B).
struct ThemePicker: View {
    static let gap: CGFloat = 12

    var body: some View {
        let chosen = ThemeStore.shared.theme
        HStack(alignment: .top, spacing: Self.gap) {
            ForEach(AppTheme.allCases) { theme in
                ThemeSwatch(theme: theme, chosen: theme == chosen) { ThemeStore.shared.choose(theme) }
            }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .padding(.top, 12)
        .padding(.bottom, 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Theme")
    }
}

/// One theme: a miniature of the window in its colours, and its name.
private struct ThemeSwatch: View {
    let theme: AppTheme
    let chosen: Bool
    let choose: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: choose) {
            VStack(alignment: .leading, spacing: 7) {
                ThemeMiniature(palette: theme.palette)
                    .overlay {
                        if chosen {
                            PencilRing().stroke(Theme.accent, style: StrokeStyle(lineWidth: PencilRing.lineWidth, lineCap: .round))
                                .padding(.horizontal, -PencilRing.outset.width)
                                .padding(.vertical, -PencilRing.outset.height)
                                .rotationEffect(.degrees(-1))
                                .allowsHitTesting(false)
                        }
                    }
                // The name may run a little into the gap (bold "Contact sheet"
                // is wider than its column), so it takes no width of its own.
                Color.clear.frame(height: 18).overlay(alignment: .leading) {
                    Text(theme.title)
                        .font(Theme.serif(14, weight: chosen ? 700 : 400))
                        .foregroundStyle(chosen ? Theme.paper : hovered ? Theme.label : Theme.dim)
                        .lineLimit(1)
                        .fixedSize()
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .frame(maxWidth: .infinity, alignment: .leading)
        .onHover { hovered = $0 }
        .inkFocus(activate: choose)
        .help(theme.hint)
        .accessibilityLabel(theme.title)
        .accessibilityHint(theme.hint)
        .accessibilityAddTraits(chosen ? [.isSelected, .isButton] : .isButton)
    }
}

/// The window in a theme's colours, 62 pt tall: the title band, the sheet
/// with a picture and the label's two lines, the panel with its rule and a
/// marked line, and the strip with the current frame marked.
struct ThemeMiniature: View {
    let palette: Palette
    static let height: CGFloat = 62
    private static let picture = LinearGradient(colors: [Color(red: 0.55, green: 0.68, blue: 0.78), Color(red: 0.86, green: 0.74, blue: 0.56)],
                                                startPoint: .top, endPoint: .bottom)

    var body: some View {
        let c = palette.colors, f = palette.filmColors
        VStack(spacing: 0) {
            c.bg.frame(height: 7)
            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    ZStack(alignment: .topLeading) {
                        c.bg
                        Self.picture.frame(width: 40, height: 27).offset(x: 8, y: 3)
                        HStack(spacing: 3) {
                            Capsule().fill(c.paper).frame(width: 20, height: 2.5)
                            Capsule().fill(c.dim).frame(width: 14, height: 2.5)
                        }
                        .offset(x: 8, y: 33)
                    }
                    HStack(spacing: 2) {
                        ForEach(0..<4, id: \.self) { i in
                            VStack(spacing: 1) {
                                Self.picture.frame(width: 15).opacity(i == 1 ? 1 : Theme.stripDim)
                                (i == 1 ? f.accent : .clear).frame(width: 15, height: 1.5)
                            }
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.top, 2)
                    .frame(height: 13)
                    .background(c.film)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Capsule().fill(c.paper).frame(width: 15, height: 3)
                        .overlay(alignment: .bottom) { c.accent.frame(height: 1.2).offset(y: 2.5) }
                    ForEach([0.7, 1, 0.6], id: \.self) { w in Capsule().fill(c.label.opacity(0.7)).frame(width: 20 * w, height: 2) }
                }
                .padding(.horizontal, 5)
                .padding(.top, 6)
                .frame(width: 30, alignment: .topLeading)
                .frame(maxHeight: .infinity, alignment: .top)
                .background(c.panel)
                .overlay(alignment: .leading) { c.line.frame(width: 1) }
                .overlay(alignment: .top) { c.line.frame(height: 1) }
            }
        }
        .frame(minWidth: 0, maxWidth: .infinity)
        .frame(height: Self.height)
        .clipShape(RoundedRectangle(cornerRadius: 4))
        .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Theme.line, lineWidth: 1))
        .accessibilityHidden(true)
    }
}
