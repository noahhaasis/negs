import AppKit
import XCTest
@testable import NegsApp

/// Settings as "B · Index" (design/proposals/r19-settings.html): the title
/// band, three tabs, and a window that fits the open page from its top edge.
@MainActor final class SettingsTests: XCTestCase {
    private func controller() -> (SettingsWindowController, EditorModel) {
        _ = NSApplication.shared
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: "settingsTab")
        addTeardownBlock { defaults.set(saved, forKey: "settingsTab") }
        let model = EditorModel(sidecars: SidecarStore())
        addTeardownBlock { model.shutdown() }
        return (SettingsWindowController(updates: UpdateController(), model: model, removeAppData: {}), model)
    }

    func testTheWindowHasTheTitleBandAndNoSystemTitle() throws {
        let (controller, _) = controller()
        let window = try XCTUnwrap(controller.window)
        XCTAssertEqual(window.title, "Settings", "VoiceOver and the Window menu read the title")
        XCTAssertEqual(window.titleVisibility, .hidden, "the band shows it in Newsreader")
        XCTAssertTrue(window.toolbar?.items.isEmpty ?? true)
        let frame = try XCTUnwrap(window.contentView as? EditorFrameView)
        XCTAssertEqual(frame.band.title, "Settings")
        XCTAssertEqual(frame.content.identifier, InkKeyLoop.inspectorIdentifier, "Tab steps through the page's controls")
        XCTAssertEqual(window.contentRect(forFrameRect: window.frame).width, SettingsWindowController.width)
    }

    func testEachPageFitsTheWindowFromItsTop() throws {
        UserDefaults.standard.set(SettingsTab.speed.rawValue, forKey: "settingsTab")
        let (controller, _) = controller()
        let window = try XCTUnwrap(controller.window)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        window.contentView?.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        let speed = window.frame
        XCTAssertGreaterThan(speed.height, Theme.Size.titleBar + Theme.Size.tabRow + 100, "the page has its rows")

        UserDefaults.standard.set(SettingsTab.photographs.rawValue, forKey: "settingsTab")
        RunLoop.main.run(until: Date().addingTimeInterval(0.6))
        let photographs = window.frame
        XCTAssertGreaterThan(photographs.height, speed.height, "Photographs has more rows than Speed")
        XCTAssertEqual(photographs.maxY, speed.maxY, accuracy: 0.5, "the top edge stays put")
        XCTAssertNil(window.firstResponder as? FocusAnchorView, "nothing has the focus box until Tab")
    }

    func testStockMenusHaveASectionPerGroup() {
        let stock = { (id: String, positive: Bool) in
            Stock(.object(["id": .string(id), "name": .string(id.capitalized), "paper": .bool(false), "positive": .bool(positive),
                           "bw": .bool(false), "target_print": .string("endura"), "development_times": .array([])]))
        }
        let groups = StockGroup.films([stock("gold", false), stock("velvia", true)], papers: [])
        XCTAssertEqual(SettingsView.items(groups), [
            .header("Color Negative"), .option("Gold", tag: "gold"),
            .header("Color Slide"), .option("Velvia", tag: "velvia"),
        ])
    }
}
