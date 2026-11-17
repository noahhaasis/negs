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
