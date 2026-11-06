import SwiftUI

/// The film strip as the main view, to flick through a folder with every frame
/// showing its final edit. The frame nearest the centre is the current one, and
/// becomes the open photograph, for the sidebar, once the film stops.
struct BigFilmStrip: View {
    let model: EditorModel
    let directory: URL
    @State private var listing = FolderListing()
    @State private var strip: FilmStrip?
    @State private var scroller = FilmRollScroller()
    @State private var centre = 0
    @State private var settling: Task<Void, Never>?
    @State private var refreshing: Task<Void, Never>?
    @State private var size = CGSize.zero

    private var shown: [PhotoFile] { PhotoFolder.shown(listing.files, rawOnly: model.filmstripRawOnly) }
    private var texture: CGSize { strip?.pixelSize ?? FilmRollLayout.defaultTexture }
    private var height: CGFloat { max(size.height, FilmRollLayout.stripHeightRange.lowerBound) }
    private var length: CGFloat { FilmRollLayout.frameLength(stripHeight: height, texture: texture) }
    /// Blank film before the first frame and after the last, so each can reach the centre.
    private var leadFrames: Int { FilmRollLayout.leadFrames(viewport: size.width, frameLength: length) }

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach(0..<leadFrames, id: \.self) { _ in BlankFilmFrame(height: height, number: 0, strip: strip, film: model.film) }
                ForEach(Array(shown.enumerated()), id: \.element.url) { index, file in
                    FilmFrame(height: height, file: file, number: index + 1, selected: index == centre, strip: strip, film: model.film,
                              developed: model.rollDeveloper?.frames[file.url]) { model.leaveBigStrip(opening: file.url) }
                }
                ForEach(0..<leadFrames, id: \.self) { index in
                    BlankFilmFrame(height: height, number: shown.count + index + 1, strip: strip, film: model.film)
                }
            }
            .background(EnclosingScrollFinder(scroller: scroller))
        }
        .scrollIndicators(.hidden)
        .overlay {
            FilmMouseSurface(scroller: scroller, enabled: true) { point in
                let frame = CGSize(width: length, height: height)
                let gate = FilmRollLayout.gate(window: strip?.window ?? FilmRollLayout.defaultWindow, frame: frame)
                let onFilm = CGPoint(x: point.x - CGFloat(leadFrames) * length, y: point.y)
                guard let index = FilmRollLayout.photo(at: onFilm, frame: frame, gate: gate, photos: shown.count) else { return }
                model.leaveBigStrip(opening: shown[index].url)
            }
        }
        .onGeometryChange(for: CGSize.self) { $0.size } action: { size = $0; centreOnOpenPhotograph() }
        .background(FilmRollLayout.filmBase)
        .overlay {
            Group {
                if listing.loading { ProgressView().controlSize(.small) }
                else if listing.failed { Text("This folder couldn’t be read.") }
                else if shown.isEmpty { Text(model.filmstripRawOnly ? "No RAW files in this folder." : "No supported photographs in this folder.") }
            }
            .foregroundStyle(.white.opacity(0.75))
        }
        .accessibilityLabel("Film Strip")
        .onAppear {
            scroller.onScroll = { filmMoved() }
        }
        .task(id: directory) {
            await listing.watch(directory) {
                model.rollDeveloper?.show(files: shown)
                DispatchQueue.main.async { centreOnOpenPhotograph() }
            }
        }
        .task(id: "\(model.film) \(model.hasCatalog)") {
            if let loaded = await model.loadRollStrip() { strip = loaded }
        }
        .onChange(of: shown) { _, files in model.rollDeveloper?.show(files: files) }
        // The open photograph's edit, once a run of changes has paused.
        .onChange(of: model.state) { _, _ in
            refreshing?.cancel()
            refreshing = Task {
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled, let source = model.source else { return }
                if model.sameLookForRoll { model.rollDeveloper?.invalidateAll() } else { model.rollDeveloper?.invalidate(source) }
            }
        }
        .onChange(of: model.sameLookForRoll) { _, _ in model.rollDeveloper?.invalidateAll() }
    }

    /// Starts with the open photograph in the middle.
    private func centreOnOpenPhotograph() {
        guard size.width > 0, let source = model.source, let index = shown.firstIndex(where: { $0.url == source }) else { return }
        guard scroller.scroll != nil else { return }
        scroller.offset = FilmRollLayout.centringOffset(frame: index, frameLength: length, lead: leadFrames, viewport: size.width)
        filmMoved()
    }

    private func filmMoved() {
        guard !shown.isEmpty else { return }
        let index = FilmRollLayout.centreFrame(offset: scroller.offset, frameLength: length, lead: leadFrames, viewport: size.width, photos: shown.count)
        if index != centre { centre = index }
        model.rollDeveloper?.setCentre(index)
        settling?.cancel()
        settling = Task {
            try? await Task.sleep(for: .milliseconds(FilmRollLayout.settleMilliseconds))
            guard !Task.isCancelled, !scroller.isCoasting, shown.indices.contains(centre) else { return }
            let url = shown[centre].url
            if url != model.source { model.open(url) }
        }
    }
}
