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
        view.addItem(.separator())
        command(view, "Fit Photograph", #selector(fitPhotograph), "0")
        command(view, "Actual Size", #selector(actualSize), "1")
        command(view, "Rotate Clockwise", #selector(rotate), "r")
        command(view, "Crop", #selector(crop), "k")
        view.addItem(.separator())
        command(view, "Reset All Adjustments", #selector(resetAdjustments), "r", [.command, .shift])
        view.addItem(.separator())

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
    @objc private func toggleFilmstrip() { model.filmstripVisible.toggle() }
    @objc private func toggleAdjustments() { model.inspectorVisible.toggle() }
    @objc private func fitPhotograph() { model.zoom(fit: true) }
    @objc private func actualSize() { model.zoom(fit: false) }
    @objc private func rotate() { model.rotate() }
    @objc private func crop() { model.cropping ? model.endCrop(apply: true) : model.beginCrop() }
    @objc private func resetAdjustments() { model.reset() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if removingAppData { return false }
        let photo = model.source != nil
        switch item.action {
        case #selector(checkForUpdates): return updates.configured && updates.canCheck
        case #selector(showRecent): return !model.recentFiles.isEmpty
        case #selector(savePreview): return model.canSave
        case #selector(exportPhotograph): return photo && model.ready && !model.exporting && !model.cropping
        case #selector(toggleFilmstrip):
            item.title = model.filmstripVisible ? "Hide Film Strip" : "Show Film Strip"
            return true
        case #selector(toggleAdjustments): item.state = model.inspectorVisible ? .on : .off; return true
        case #selector(fitPhotograph), #selector(actualSize): return photo && !model.cropping
        case #selector(rotate): return photo
        case #selector(crop):
            item.title = model.cropping ? "Done Cropping" : "Crop"
            return model.cropping || model.image != nil
        case #selector(resetAdjustments): return model.hasCatalog && !model.cropping
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
