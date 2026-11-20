# Building

Claude sessions build `--release` without LTO, with 16 codegen units and
incremental compilation (`.claude/settings.json`). Renders are byte-identical
to the shipping profile and within 1% as fast, and rebuilds are ~10× faster.
CI and `web/build.sh` keep `Cargo.toml`'s shipping profile. To compare speed
against shipped builds or older numbers, unset the `CARGO_PROFILE_RELEASE_*`
variables for both sides.

- Try the macOS app: `scripts/package_macos_app.sh --dev` gives
  `dist/negs-macos-dev/Negs.app` (Apple silicon, ~15 s after an engine edit,
  ~55 s after a Swift edit). For UI work that doesn't need release speed, use
  `--dev --debug` (~25 s after a Swift edit). Run the script without `--dev`
  (universal, DMG, source archive) only when packaging is the task. It
  replaces `dist/negs-macos`, which may be the app Noah is running.
- `--dev` puts `spektrafilm-native`, `spektrafilm` and `spektrafilm-f64` in
  `target/release`, where the Swift tests look for them. Run Swift tests with
  `swift test --package-path macos --scratch-path target/swift-native`.
- Rust tests: run the crate or test you touched
  (`cargo test --release -p spektrafilm-core --test prep`) before the whole
  workspace (`cargo test --release --workspace --exclude spektrafilm-wasm`).
- New worktrees get a copy-on-write clone of the main checkout's
  `target/release` and `target/debug` from a post-checkout hook
  (`scripts/seed_worktree_build.sh --install`), so only the workspace crates
  build. Don't point a worktree's `--target-dir` at the main checkout: shared
  build directories serialise sessions and rebuild each other's crates.

# Frame pipeline model

`macos/Specs/FramePipeline.tla` models how the canvas gets its frame
(`EditorModel` `render()`, the brightness match and held frame, releases,
crops, early frames). When changing those, update the matching action and run
`scripts/check_frame_model.sh` (~4 min) and
`FramePipelineTests` (random sequences: `NEGS_PIPELINE_SEEDS=25`). See
`macos/Specs/README.md`.

`LibraryStateTests` does the same for sidecars, undo and thumbnails: random
edits, ratings, Paste, Reset, Duplicate, Delete, Undo/Redo, other apps'
writes and relaunches, checking that what the app shows is what is on disk,
that an action undone at once leaves the files as they were, and that each
thumbnail shows the edits it's filed under. Run it with
`NEGS_LIBRARY_SEEDS=25` when changing sidecar writes, library undo or
thumbnail filing.

# Windows PC

All Windows work happens in `C:\Users\Noah\Projects\negs`, which stays on
main. `C:\Users\Noah\.negs\build-main.cmd` pulls GitHub's main and builds it:
the app is `C:\Users\Noah\Projects\negs\windows\target\release\Negs.exe`. A
branch gets a worktree under `negs\worktrees\<name>` with
`C:\Users\Noah\.negs\wt.cmd <name> <mac-branch> [build]`; remove it when done.
Don't clone the repo into `.negs` or make other `Projects\negs-*` folders.
