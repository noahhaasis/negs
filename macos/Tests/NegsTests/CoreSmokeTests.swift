import CNegsCore
import XCTest
@testable import NegsApp

/// The shared app core (core/, Odin) links into the app and its tests: the
/// ABI the header was written for, and a block out of the core and back.
final class CoreSmokeTests: XCTestCase {
    func testLinksTheCoreItWasBuiltFor() {
        XCTAssertEqual(negs_core_abi_version(), NEGS_CORE_ABI_VERSION)
        XCTAssertTrue(String(cString: negs_core_version()).contains("odin"))
    }

    func testBuffersComeBackWhole() {
        let bytes: [UInt8] = [1, 2, 3, 255]
        let buf = bytes.withUnsafeBufferPointer { negs_buf_copy($0.baseAddress, $0.count) }
        defer { negs_buf_free(buf) }
        XCTAssertEqual(Array(UnsafeBufferPointer(start: buf.data, count: buf.len)), bytes)
        XCTAssertEqual(negs_core_layout("negs_buf_t.len", 14), Int64(MemoryLayout<negs_buf_t>.offset(of: \.len)!))
        XCTAssertEqual(negs_core_layout("negs_buf_t", 10), Int64(MemoryLayout<negs_buf_t>.size))
    }

    /// The Swift cases are the core's themes, in its order, under its keys.
    func testThemesAreTheCores() {
        XCTAssertEqual(Int(negs_theme_count()), AppTheme.allCases.count)
        for theme in AppTheme.allCases {
            XCTAssertEqual(String(cString: Palette.core(theme.coreId).key), theme.rawValue)
            let parsed = theme.title.withCString { negs_theme_parse($0, strlen($0)) }
            XCTAssertEqual(parsed, Int32(theme.coreId), theme.title)
        }
        XCTAssertEqual(AppTheme.contactSheet.palette.filmHex.accent, 0xe2492f)
        XCTAssertEqual(AppTheme.darkroom.hint, "Warm black, cream ink and the yellow pencil. The default.")
    }

    /// The layouts Swift sees are the core's.
    func testMarksAndThemeLayouts() {
        func size<T>(_: T.Type, _ name: String) { XCTAssertEqual(negs_core_layout(name, name.utf8.count), Int64(MemoryLayout<T>.size), name) }
        size(negs_seg_t.self, "negs_seg_t")
        size(negs_strokes_t.self, "negs_strokes_t")
        size(negs_radii_t.self, "negs_radii_t")
        size(negs_theme_t.self, "negs_theme_t")
        size(negs_theme_constants_t.self, "negs_theme_constants_t")
        XCTAssertEqual(negs_core_layout("negs_theme_t.key", 16), Int64(MemoryLayout<negs_theme_t>.offset(of: \.key)!))
        XCTAssertEqual(negs_core_layout("negs_strokes_t.tilt", 19), Int64(MemoryLayout<negs_strokes_t>.offset(of: \.tilt)!))
    }
}
