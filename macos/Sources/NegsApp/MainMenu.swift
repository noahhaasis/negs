import AppKit
import SwiftUI

/// The menu bar. The standard items go to the responder chain; the app's own
/// commands go to the model, and are enabled and titled from its state when a
/// menu opens.
@MainActor final class MainMenu: NSObject, NSMenuItemValidation, NSMenuDelegate {
    let menu = NSMenu()
    private let model: EditorModel
    private let updates: UpdateController
    private let removeAppData: () -> Void
    private let recent = NSMenu(title: "Open Recent")
    private var settings: NSWindow?
    private var removingAppData = false

    init(model: EditorModel, updates: UpdateController, removeAppData: @escaping () -> Void) {
        self.model = model
        self.updates = updates
        self.removeAppData = removeAppData
        super.init()
        let name = ProcessInfo.processInfo.processName == "NegsApp" ? "Negs" : ProcessInfo.processInfo.processName

        let app = submenu(name)
        app.addItem(withTitle: "About \(name)", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        command(app, "Check for Updates…", #selector(checkForUpdates), "")
        app.addItem(.separator())
        command(app, "Settings…", #selector(showSettings), ",")
        app.addItem(.separator())
        let services = NSMenu(title: "Services")
        app.addItem(withTitle: "Services", action: nil, keyEquivalent: "").submenu = services
        NSApp.servicesMenu = services
        app.addItem(.separator())
        app.addItem(withTitle: "Hide \(name)", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        app.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h").keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Quit \(name)", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

        let file = submenu("File")
        command(file, "Open Photograph…", #selector(openPhotograph), "o")
        // The action only lets validation disable the submenu while it's empty.
        let recentItem = file.addItem(withTitle: "Open Recent", action: #selector(showRecent), keyEquivalent: "")
        recentItem.target = self
        recentItem.submenu = recent
        recent.delegate = self
        file.addItem(.separator())
        file.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        file.addItem(.separator())
        command(file, "Save Preview…", #selector(savePreview), "s")
        command(file, "Export Photograph…", #selector(exportPhotograph), "e", [.command, .shift])
        command(file, "Export LUT…", #selector(exportLUT), "l", [.command, .shift])
        file.addItem(.separator())
        command(file, "New Preset", #selector(newPreset), "s", [.command, .shift])
        command(file, "Import Presets…", #selector(importPresets), "o", [.command, .shift])

        let edit = submenu("Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z").keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Delete", action: #selector(NSText.delete(_:)), keyEquivalent: "")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")

        let view = submenu("View")
        command(view, "Hide Film Strip", #selector(toggleFilmstrip), "f", [.command, .option])
        command(view, "Show Adjustments", #selector(toggleAdjustments), "i", [.command, .option])
        command(view, "Show Presets", #selector(togglePresets), "p", [.command, .option])
        view.addItem(.separator())
        command(view, "Fit Photograph", #selector(fitPhotograph), "0")
        command(view, "Actual Size", #selector(actualSize), "1")
        command(view, "Rotate Clockwise", #selector(rotate), "r")
        command(view, "Crop", #selector(crop), "k")
        view.addItem(.separator())
        command(view, "Reset All Adjustments", #selector(resetAdjustments), "r", [.command, .shift])
        view.addItem(.separator())

        // Plain keys, as in Lightroom and Capture One. Disabled while text is
        // typed, so a number typed into a field never rates the photograph.
        let photo = submenu("Photo")
        let rating = NSMenu(title: "Rating")
        photo.addItem(withTitle: "Rating", action: nil, keyEquivalent: "").submenu = rating
        for stars in 0...5 {
            let item = rating.addItem(withTitle: stars == 0 ? "None" : String(repeating: "★", count: stars),
                                      action: #selector(rate(_:)), keyEquivalent: "\(stars)")
            item.keyEquivalentModifierMask = []; item.tag = stars; item.target = self
            if stars == 0 { rating.addItem(.separator()) }
        }
        rating.addItem(.separator())
        command(rating, "Raise Rating", #selector(raiseRating), "]", [])
        command(rating, "Lower Rating", #selector(lowerRating), "[", [])
        let flag = NSMenu(title: "Flag")
        photo.addItem(withTitle: "Flag", action: nil, keyEquivalent: "").submenu = flag
        command(flag, "Pick", #selector(pick), "p", [])
        command(flag, "Reject", #selector(reject), "x", [])
        command(flag, "Unflagged", #selector(unflag), "u", [])
        photo.addItem(.separator())
        command(photo, "Advance After Rating", #selector(toggleAdvance), "")
        photo.addItem(.separator())
        command(photo, "Clear Filter", #selector(clearFilter), "l", [.command, .option])

        let window = submenu("Window")
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        window.addItem(.separator())
        window.addItem(withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        NSApp.windowsMenu = window

        NSApp.helpMenu = submenu("Help")
    }

    private func submenu(_ title: String) -> NSMenu {
        let submenu = NSMenu(title: title)
        menu.addItem(withTitle: title, action: nil, keyEquivalent: "").submenu = submenu
        return submenu
    }

    private func command(_ menu: NSMenu, _ title: String, _ action: Selector, _ key: String, _ modifiers: NSEvent.ModifierFlags = .command) {
        let item = menu.addItem(withTitle: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = self
    }

    // MARK: Commands

    @objc private func checkForUpdates() { updates.check() }
    @objc private func showSettings() {
        if settings == nil {
            let window = NSWindow(contentViewController: NSHostingController(rootView: UpdateSettings(updates: updates, model: model, removeAppData: removeAppData)))
            window.title = "Settings"
            window.styleMask = [.titled, .closable]
            window.isReleasedWhenClosed = false
            window.center()
            settings = window
        }
        settings?.makeKeyAndOrderFront(nil)
    }
    func prepareForAppDataRemoval() { removingAppData = true; settings?.orderOut(nil) }
    @objc private func openPhotograph() { model.openPanel() }
    @objc private func showRecent() {}
    @objc private func openRecent(_ item: NSMenuItem) { if let url = item.representedObject as? URL { model.open(url) } }
    @objc private func savePreview() { model.savePanel(export: false) }
    @objc private func exportPhotograph() { model.savePanel(export: true) }
    @objc private func exportLUT() { model.exportLUTPanel() }
    @objc private func toggleFilmstrip() { model.filmstripVisible.toggle() }
    @objc private func toggleAdjustments() { model.togglePane(.adjustments) }
    @objc private func togglePresets() { model.togglePane(.presets) }
    @objc private func newPreset() { model.newPreset() }
    @objc private func importPresets() { model.importPresetsPanel() }
    @objc private func fitPhotograph() { model.zoom(fit: true) }
    @objc private func actualSize() { model.zoom(fit: false) }
    @objc private func rotate() { model.rotate() }
    @objc private func crop() { model.cropping ? model.endCrop(apply: true) : model.beginCrop() }
    @objc private func resetAdjustments() { model.reset() }
    @objc private func rate(_ item: NSMenuItem) { model.setRating(item.tag) }
    @objc private func raiseRating() { model.changeRating(by: 1) }
    @objc private func lowerRating() { model.changeRating(by: -1) }
    @objc private func pick() { model.setFlag(.pick) }
    @objc private func reject() { model.setFlag(.reject) }
    @objc private func unflag() { model.setFlag(nil) }
    @objc private func toggleAdvance() { model.advanceAfterRating.toggle() }
    @objc private func clearFilter() { model.library.filter = LibraryFilter() }

    /// Rating and flag keys act on the photograph in the editing window,
    /// and not while a text field there (or any other window) has the keys.
    private var rating: Bool { model.source != nil && Self.takesRatingKeys(NSApp.keyWindow) }
    /// The same check as holding \ for the original.
    static func takesRatingKeys(_ window: NSWindow?) -> Bool {
        guard let window, window is EditorWindow else { return false }
        return !(window.firstResponder is NSText)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if removingAppData { return false }
        let photo = model.source != nil
        switch item.action {
        case #selector(checkForUpdates): return updates.configured && updates.canCheck
        case #selector(showRecent): return !model.recentFiles.isEmpty
        case #selector(savePreview): return model.canSave
        case #selector(exportPhotograph): return photo && model.ready && !model.exporting && !model.saving && !model.cropping
        case #selector(exportLUT): return model.canExportLUT
        case #selector(toggleFilmstrip):
            item.title = model.filmstripVisible ? "Hide Film Strip" : "Show Film Strip"
            return true
        case #selector(toggleAdjustments):
            item.state = model.inspectorVisible && model.inspectorPane == .adjustments ? .on : .off; return true
        case #selector(togglePresets):
            item.state = model.inspectorVisible && model.inspectorPane == .presets ? .on : .off; return true
        case #selector(newPreset), #selector(importPresets): return model.canChangePresets
        case #selector(fitPhotograph), #selector(actualSize): return photo && !model.cropping
        case #selector(rotate): return photo
        case #selector(crop):
            item.title = model.cropping ? "Done Cropping" : "Crop"
            return model.cropping || model.image != nil
        case #selector(resetAdjustments): return model.hasCatalog && !model.cropping
        case #selector(rate(_:)):
            item.state = model.currentSummary?.rating == item.tag ? .on : .off; return rating
        case #selector(raiseRating), #selector(lowerRating): return rating
        case #selector(pick): item.state = model.currentSummary?.flag == .pick ? .on : .off; return rating
        case #selector(reject): item.state = model.currentSummary?.flag == .reject ? .on : .off; return rating
        case #selector(unflag): item.state = model.currentSummary.map { $0.flag == nil } == true ? .on : .off; return rating
        case #selector(toggleAdvance): item.state = model.advanceAfterRating ? .on : .off; return true
        case #selector(clearFilter): return model.library.filter.active
        default: return true
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        for url in model.recentFiles {
            let item = menu.addItem(withTitle: url.lastPathComponent, action: #selector(openRecent), keyEquivalent: "")
            item.target = self; item.representedObject = url; item.toolTip = url.path
        }
    }
}
