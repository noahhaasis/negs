import AppKit
import Combine
import Sparkle
import SwiftUI

/// Construct Sparkle only after startup work has finished. No release URL or
/// signing key is guessed for local builds; packaging supplies both together.
@MainActor final class UpdateController: NSObject, ObservableObject, SPUUpdaterDelegate {
    @Published private(set) var canCheck = false
    @Published private(set) var automaticChecks = false
    @Published private(set) var lastCheck: Date?
    @Published private(set) var configurationError: String?
    private var controller: SPUStandardUpdaterController?
    private var observations: [NSKeyValueObservation] = []
    private var launchTask: Task<Void, Never>?
    private var installTask: Task<Void, Never>?
    private weak var model: EditorModel?
    private(set) var installing = false

    var configured: Bool {
        Self.validConfiguration(Bundle.main.infoDictionary ?? [:])
    }

    nonisolated static func validConfiguration(_ info: [String: Any]) -> Bool {
        guard let feed = info["SUFeedURL"] as? String, let url = URL(string: feed),
              url.scheme == "https", url.host != nil,
              let key = info["SUPublicEDKey"] as? String, Data(base64Encoded: key)?.count == 32 else { return false }
        return true
    }

    func schedule(afterStartupOf model: EditorModel) {
        self.model = model
        guard configured, launchTask == nil else { return }
        launchTask = Task { [weak self, weak model] in
            // Avoid adding work during the first window layout and first render.
            try? await Task.sleep(for: .seconds(3))
            while let model, !Task.isCancelled {
                if (model.ready && (model.source == nil || model.previewCurrent || model.renderFailed)) || model.error != nil {
                    self?.start(); return
                }
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    private func start() {
        guard configured, controller == nil else { return }
        let controller = SPUStandardUpdaterController(startingUpdater: false, updaterDelegate: self, userDriverDelegate: nil)
        self.controller = controller
        let updater = controller.updater
        observations = [
            updater.observe(\.canCheckForUpdates, options: [.initial, .new]) { [weak self] updater, _ in
                MainActor.assumeIsolated { self?.canCheck = updater.canCheckForUpdates }
            },
            updater.observe(\.automaticallyChecksForUpdates, options: [.initial, .new]) { [weak self] updater, _ in
                MainActor.assumeIsolated { self?.automaticChecks = updater.automaticallyChecksForUpdates }
            },
            updater.observe(\.lastUpdateCheckDate, options: [.initial, .new]) { [weak self] updater, _ in
                MainActor.assumeIsolated { self?.lastCheck = updater.lastUpdateCheckDate }
            }
        ]
        do { try updater.start() }
        catch { configurationError = error.localizedDescription }
    }

    func check() {
        start()
        if canCheck { controller?.checkForUpdates(nil) }
    }

    func setAutomaticChecks(_ enabled: Bool) {
        start()
        controller?.updater.automaticallyChecksForUpdates = enabled
    }

    func updater(_ updater: SPUUpdater, shouldPostponeRelaunchForUpdate item: SUAppcastItem,
                 untilInvokingBlock installHandler: @escaping () -> Void) -> Bool {
        postponeRestart(until: installHandler)
    }

    /// Also exercised with a real ExportJob without starting a network update.
    func postponeRestart(until installHandler: @escaping () -> Void) -> Bool {
        installing = true
        guard let model, model.exporting || model.saving else { return false }
        model.status = "Update downloaded. Waiting for the current save or export before restarting…"
        installTask = Task { [weak self, weak model] in
            while let model, model.exporting || model.saving {
                try? await Task.sleep(for: .milliseconds(250))
                guard !Task.isCancelled else { return }
            }
            guard self != nil, !Task.isCancelled else { return }
            installHandler()
        }
        return true
    }

    func updater(_ updater: SPUUpdater, willInstallUpdate item: SUAppcastItem) { installing = true }

    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        installing = false
        installTask?.cancel(); installTask = nil
    }

    func stop() {
        launchTask?.cancel()
        installTask?.cancel()
    }
}

struct UpdateSettings: View {
    @ObservedObject var updates: UpdateController

    var body: some View {
        Form {
            if updates.configured {
                Toggle("Automatically check for updates", isOn: Binding(
                    get: { updates.automaticChecks }, set: { updates.setAutomaticChecks($0) }))
                Text("Updates are installed only when you choose to restart Negs.")
                    .font(.callout).foregroundStyle(.secondary)
                if let lastCheck = updates.lastCheck {
                    Text("Last checked: \(lastCheck.formatted(date: .abbreviated, time: .shortened))")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Button("Check for Updates…", action: updates.check).disabled(!updates.canCheck)
                if let error = updates.configurationError { Text(error).foregroundStyle(.red) }
            } else {
                Text("Updates are not configured for this build.")
                Text("Install a release build to receive updates.").foregroundStyle(.secondary)
            }
        }
        .padding(24).frame(width: 420)
    }
}
