import AppKit
import SwiftUI

/// The colour behind the photograph in the viewer: black by default, like the
/// rest of the app. A custom colour is stored in defaults as an sRGB hex string.
enum ViewerBackground {
    static let customKey = "viewerBackgroundCustom"
    static let colorKey = "viewerBackgroundColor"
    /// A neutral dark grey, as a starting point for a custom colour.
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

    /// The viewer's backdrop: black unless a custom one is on.
    static func canvas(custom: Bool, hex: String) -> Color {
        guard custom, let customColor = color(hex: hex) else { return Color.black }
        return customColor
    }
}
