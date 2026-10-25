import Foundation

/// Fixed app-owned locations only. Never derive a deletion target from a
/// session, recent photograph, export destination, or engine override.
struct AppDataLocations: Sendable {
    var directories: [URL]
    var temporary: URL
    var preferencesDomain: String

    static var current: AppDataLocations {
        let fm = FileManager.default
        let library = fm.urls(for: .libraryDirectory, in: .userDomainMask)[0]
        let domain = Bundle.main.bundleIdentifier ?? ProcessInfo.processInfo.processName
        return AppDataLocations(directories: [
            fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("dev.negs.app"),
            fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("dev.negs.app"),
            // Sparkle stores downloads and updater state under the bundle ID.
            fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent(domain),
            library.appendingPathComponent("HTTPStorages/\(domain)"),
            library.appendingPathComponent("HTTPStorages/\(domain).binarycookies"),
            library.appendingPathComponent("Saved Application State/\(domain).savedState"),
            library.appendingPathComponent("Logs/\(domain)")
        ], temporary: ScratchSpace.defaultRoot, preferencesDomain: domain)
    }
}

enum AppDataCleaner {
    /// Call after stopping the engine, updater and window autosave, and after
    /// draining cache/session I/O. Returns paths that couldn't be removed so
    /// the user can retry; a partial cleanup is never reported as complete.
    static func removeFiles(at locations: AppDataLocations) -> [String] {
        var failures: [String] = []
        let fm = FileManager.default
        for directory in Set(locations.directories) {
            do { try fm.removeItem(at: directory) }
            catch let error as CocoaError where error.code == .fileNoSuchFile { continue }
            catch { failures.append("\(directory.path): \(error.localizedDescription)") }
        }
        do { try ScratchSpace.cleanForRemoval(root: locations.temporary) }
        catch { failures.append("\(locations.temporary.path): \(error.localizedDescription)") }
        return failures.sorted()
    }

    @discardableResult static func removePreferences(domain: String, defaults: UserDefaults = .standard) -> Bool {
        defaults.removePersistentDomain(forName: domain)
        // Commit the removal before the process exits. Clearing through the
        // preferences service avoids a cached plist being written back later.
        return defaults.synchronize()
    }
}
