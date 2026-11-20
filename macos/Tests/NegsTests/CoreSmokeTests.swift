import CNegsCore
import XCTest

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
}
