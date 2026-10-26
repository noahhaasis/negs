import AppKit
import SwiftUI

/// What the inspector column shows beside a photograph.
enum InspectorPane: String {
    case adjustments, presets
}

/// The Presets panel: the presets in the app's folder. Clicking one applies
/// it; double-click or Return applies it again after other edits.
struct PresetInspector: View {
    let model: EditorModel
    @State private var selection: URL?
    @State private var dropTargeted = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if model.presets.isEmpty { empty } else { list }
        }
        .dropDestination(for: URL.self) { urls, _ in
            let files = urls.filter(\.isFileURL)
            model.importPresets(files)
            return !files.isEmpty
        } isTargeted: { dropTargeted = $0 && model.canChangePresets }
        .overlay {
            if dropTargeted { RoundedRectangle(cornerRadius: 10).strokeBorder(.tint, lineWidth: 3).padding(6).allowsHitTesting(false) }
        }
        .task { await model.refreshPresets() }
        .disabled(!model.canChangePresets)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("Presets").font(.headline)
            Spacer()
            Button(action: model.newPreset) { Image(systemName: "plus") }
                .buttonStyle(.borderless)
                .help("Save the film, paper and Lab settings as a preset (⇧⌘S)")
                .accessibilityLabel("New Preset")
            Menu {
                Button("Import Presets…", action: model.importPresetsPanel)
                Button("Show Presets Folder", action: model.showPresetsFolder)
            } label: { Image(systemName: "ellipsis.circle") }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help("Import presets or show their folder")
                .accessibilityLabel("More")
        }
        .padding(.horizontal, 20).padding(.vertical, 12)
    }

    private var empty: some View {
        VStack(spacing: 10) {
            Spacer()
            Image(systemName: "square.stack").font(.system(size: 36, weight: .light)).foregroundStyle(.secondary)
            Text("No Presets").font(.title3.weight(.semibold))
            Text("Save the film, paper and Lab settings with +, or import presets made in Negs or spektrafilm. You can also drop them here.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button("Import Presets…", action: model.importPresetsPanel).padding(.top, 4)
            Spacer()
        }
        .padding(24)
    }

    private var list: some View {
        List(selection: $selection) {
            ForEach(model.presets) { preset in
                PresetRow(model: model, preset: preset, status: model.presetStatus[preset])
                    .tag(preset.url)
            }
        }
        .listStyle(.inset)
        .contextMenu(forSelectionType: URL.self) { urls in
            if let preset = preset(urls.first) {
                Button("Apply") { model.applyPreset(preset) }.disabled(!valid(preset))
                Button("Rename") { model.renamingPreset = preset.url }
                Button("Export…") { model.exportPresetPanel(preset) }
                Divider()
                Button("Delete…") { model.deletePreset(preset) }
            }
        } primaryAction: { urls in
            if let preset = preset(urls.first), valid(preset) { model.applyPreset(preset) }
        }
        .onChange(of: selection) { _, url in
            if let preset = preset(url), valid(preset), model.renamingPreset == nil { model.applyPreset(preset) }
        }
        .onDeleteCommand { if let preset = preset(selection) { model.deletePreset(preset) } }
    }

    private func preset(_ url: URL?) -> PresetFile? { model.presets.first { $0.url == url } }
    private func valid(_ preset: PresetFile) -> Bool {
        if case .valid = model.presetStatus[preset] { return true }
        return false
    }
}

private struct PresetRow: View {
    let model: EditorModel
    let preset: PresetFile
    let status: PresetStatus?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if model.renamingPreset == preset.url {
                NameField(name: preset.name) { model.renamePreset(preset, to: $0) } cancel: { model.renamingPreset = nil }
            } else {
                Text(preset.name).lineLimit(1).foregroundStyle(invalid == nil ? .primary : .secondary)
            }
            Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(invalid == nil ? 1 : 3)
        }
        .padding(.vertical, 2)
        .help(invalid ?? "")
    }

    private var invalid: String? {
        if case .invalid(let reason) = status { return reason }
        return nil
    }

    private var detail: String {
        switch status {
        case .valid(let film, let paper, let scan):
            let filmName = model.films.first { $0.id == film }?.name ?? film
            return scan ? "\(filmName) · film scan" : "\(filmName) → \(model.papers.first { $0.id == paper }?.name ?? paper)"
        case .invalid(let reason): return reason
        case nil: return " "
        }
    }
}

/// The name being edited: Return keeps it if it's a valid preset name,
/// Escape discards it. AppKit, like the inspector's value fields.
private struct NameField: NSViewRepresentable {
    let name: String
    let commit: (String) -> Void
    let cancel: () -> Void

    func makeNSView(context: Context) -> NSTextField {
        let field = FocusingTextField(string: name)
        field.controlSize = .small
        field.bezelStyle = .roundedBezel
        field.setAccessibilityLabel("Preset name")
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) { context.coordinator.parent = self }
    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: NameField
        private var finished = false
        init(_ parent: NameField) { self.parent = parent }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
            finished = true
            parent.cancel()
            return true
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard !finished, let field = notification.object as? NSTextField else { return }
            finished = true
            let name = field.stringValue.trimmingCharacters(in: .whitespaces)
            if let problem = PresetName.problem(name) {
                NSSound.beep()
                parent.cancel()
                if name != parent.name, !name.isEmpty {
                    let alert = NSAlert()
                    alert.messageText = "“\(name)” can’t be used as a preset name."
                    alert.informativeText = problem
                    alert.runModal()
                }
            } else {
                parent.commit(name)
            }
        }
    }
}
