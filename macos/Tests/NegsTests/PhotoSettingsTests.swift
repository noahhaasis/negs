import XCTest
@testable import NegsApp

final class PhotoSettingsTests: XCTestCase {
    private var directory: URL!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-settings-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }
    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: directory) }

    private func state(contrast: Double = 25) -> EditState {
        var state = EditState()
        state.film = "kodak_portra_400"
        state.params = .object(["post": .object(["contrast": .number(contrast), "tone_anchors": .array([.object(["input": .number(49), "target": .number(50)])])])])
        return state
    }
    private func read(_ url: URL) throws -> PhotoSettingsFile {
        try JSONDecoder().decode(PhotoSettingsFile.self, from: Data(contentsOf: url))
    }

    func testItIsNamedAfterThePhotographAndHoldsEverything() throws {
        let source = directory.appendingPathComponent("DSC_0001.NEF")
        let url = try PhotoSettingsFile(image: source.lastPathComponent, state: state()).write(beside: source)
        XCTAssertEqual(url.lastPathComponent, "DSC_0001.json")
        XCTAssertEqual(url.deletingLastPathComponent().path, directory.path)
        let saved = try read(url)
        XCTAssertEqual(saved.image, "DSC_0001.NEF")
        XCTAssertEqual(saved.state, state(), "Film, params, pins, crop and border all round-trip")
        XCTAssertEqual(saved.state.params["post.tone_anchors"].array.count, 1)
    }

    func testIsReadableJSON() throws {
        let source = directory.appendingPathComponent("a.png")
        let text = try String(contentsOf: PhotoSettingsFile(image: "a.png", state: state()).write(beside: source), encoding: .utf8)
        XCTAssertTrue(text.contains("\n"), "Pretty-printed")
        XCTAssertTrue(text.contains("\"negsSettings\""))
    }

    func testSavingAgainReplacesThisPhotographsFile() throws {
        let source = directory.appendingPathComponent("a.png")
        _ = try PhotoSettingsFile(image: "a.png", state: state(contrast: 1)).write(beside: source)
        let url = try PhotoSettingsFile(image: "a.png", state: state(contrast: 2)).write(beside: source)
        XCTAssertEqual(url.lastPathComponent, "a.json")
        XCTAssertEqual(try read(url).state, state(contrast: 2))
    }

    /// A RAW and its JPEG share a base name; neither loses its settings.
    func testAPhotographWithTheSameBaseNameKeepsItsOwnFile() throws {
        let raw = try PhotoSettingsFile(image: "a.nef", state: state(contrast: 1)).write(beside: directory.appendingPathComponent("a.nef"))
        let jpeg = try PhotoSettingsFile(image: "a.jpg", state: state(contrast: 2)).write(beside: directory.appendingPathComponent("a.jpg"))
        XCTAssertEqual(raw.lastPathComponent, "a.json")
        XCTAssertEqual(jpeg.lastPathComponent, "a.jpg.json")
        XCTAssertEqual(try read(raw).state, state(contrast: 1))
        XCTAssertEqual(try read(jpeg).state, state(contrast: 2))
    }

    /// A JSON file that isn't ours is never overwritten.
    func testSomeoneElsesJSONIsLeftAlone() throws {
        let foreign = directory.appendingPathComponent("a.json")
        try Data("{\"mine\": true}".utf8).write(to: foreign)
        let url = try PhotoSettingsFile(image: "a.png", state: state()).write(beside: directory.appendingPathComponent("a.png"))
        XCTAssertEqual(url.lastPathComponent, "a.png.json")
        XCTAssertEqual(try String(contentsOf: foreign, encoding: .utf8), "{\"mine\": true}")
    }

    func testAnUnwritableFolderThrows() {
        let source = directory.appendingPathComponent("missing/a.png")
        XCTAssertThrowsError(try PhotoSettingsFile(image: "a.png", state: state()).write(beside: source))
    }
}
