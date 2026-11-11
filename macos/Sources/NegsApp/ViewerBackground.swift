import AppKit
import SwiftUI

/// The colour behind the photograph in the viewer, and the sRGB hex strings
/// a custom one was stored as.
enum ViewerBackground {
    static let customKey = "viewerBackgroundCustom"
    static let colorKey = "viewerBackgroundColor"
    /// A neutral dark grey, near the default in dark appearance.
    static let defaultHex = "#242424"

    static func components(hex: String) -> (red: Double, green: Double, blue: Double)? {
        var text = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("#") { text.removeFirst() }
        guard text.count == 6, let value = UInt32(text, radix: 16) else { return nil }
        return (Double((value >> 16) & 0xFF) / 255,
                Double((value >> 8) & 0xFF) / 255,
                Double(value & 0xFF) / 255)
    }

    static func hex(red: Double, green: Double, blue: Double) -> String {
        func byte(_ value: Double) -> Int { Int((min(max(value, 0), 1) * 255).rounded()) }
        return String(format: "#%02X%02X%02X", byte(red), byte(green), byte(blue))
    }

    static func color(hex: String) -> Color? {
        guard let c = components(hex: hex) else { return nil }
        return Color(.sRGB, red: c.red, green: c.green, blue: c.blue, opacity: 1)
    }

    static func hex(_ color: Color) -> String {
        guard let srgb = NSColor(color).usingColorSpace(.sRGB) else { return defaultHex }
        return hex(red: srgb.redComponent, green: srgb.greenComponent, blue: srgb.blueComponent)
    }

    /// The viewer's backdrop: the sheet the photograph lies on (`bg`).
    ///
    /// Settings › Custom viewer background is switched off for now: the
    /// caption and label are inked for the dark sheet and can't be read on a
    /// light colour (FEEDBACK.md, 2026-11-10). A stored choice is kept under
    /// `customKey` and `colorKey` for when it comes back.
    static var canvas: Color { Theme.color(sheet) }
    /// The backdrop as 0xRRGGBB, for the light table's contrast check
    /// (`LightTable`). A custom colour must come through here too.
    static var sheet: UInt32 { Theme.Hex.bg }
}
