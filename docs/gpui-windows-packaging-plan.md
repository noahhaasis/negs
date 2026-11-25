# Windows cutover: packaging plan (GPUI app)

Prepared in Phase 6 and **not applied**. It waits for Noah's Go. Until then the installer, `package.ps1` and CI still ship `windows/`. Nothing here touches `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, `web/` or the root `Cargo.toml`/`Cargo.lock`.

## Already done on the branch (inert for shipping)

- **`app/build.rs`**: on Windows, with the SDK's `rc.exe`, it embeds:
  - icon resource 1 (8 sizes from `assets/negs-icon.png`), which GPUI's window class loads;
  - `VERSIONINFO` from `app/Cargo.toml`'s version;
  - `app/negs.manifest`, a copy of `windows/negs.manifest`: common controls 6, PerMonitorV2, `longPathAware`, Windows 10 `supportedOS`.

  GPUI's own `windows-manifest` feature is off because its manifest has no long paths. Without `rc.exe` the linker embeds the manifest, and there is no icon (a cargo warning says so).
- **Installer contract** (`windows/installer.iss` unchanged):
  - The app holds `Local\Negs.AppLifetime` until exit (`AppMutex`) and `Local\Negs.SingleInstance`.
  - It sets AppUserModelID `dev.negs.app`, as the Start menu shortcut does.
  - A second launch hands its photograph to the running window (`WM_COPYDATA`, `COPYDATA_OPEN` as `windows/`), so Open With / file associations behave as now.
- **Updates**: `app/src/updates.rs` is `windows/src/updates.rs` (WinSparkle, EdDSA). It reads `NEGS_APPCAST_URL`/`NEGS_ED_PUBLIC_KEY` at build time (`build.rs` reruns on them) and `WinSparkle.dll` next to `Negs.exe`. `package.ps1`'s `-WinSparkleDll -AppcastUrl -EdPublicKey` work unchanged.

## To change at cutover (after Go)

1. **`app/Cargo.toml`**: add the shipping profile `windows/` has (thin LTO, 1 codegen unit, opt 3). `app/` is its own workspace, so the root profile doesn't reach it. Today a CI build of `app/` would be unoptimised for size: 30.8 MB vs 23.9 MB with the profile.
   ```toml
   [profile.release]
   lto = "thin"
   codegen-units = 1
   opt-level = 3
   ```
   Set `app/Cargo.toml`'s `version` with each release, as `windows/Cargo.toml` and `installer.iss` are now.
2. **`windows/package.ps1`**:
   - `-Prebuilt` checks `app/target/release/Negs.exe` instead of `windows/target/release/Negs.exe`.
   - The non-prebuilt build runs `cargo build --release --locked --manifest-path app/Cargo.toml`.
   - Stage `app/target/release/Negs.exe`.
   - Add a third-party notice for GPUI (Apache-2.0) and its crates. GPUI brings many more crates than `windows/` did; generate one `THIRD-PARTY.txt` with `cargo about` in CI, or at least ship GPUI's `LICENSE-APACHE`.
3. **`windows/verify_package.py`**: compare `Negs.exe` with `app/target/release/Negs.exe`, and read the version from `app/Cargo.toml`.
4. **`.github/workflows/release.yml`**:
   - `changes.windows` filter: add `"app/**"`, `"crates/negs-app/**"`, `"crates/negs-library/**"`, `"crates/negs-marks/**"`.
   - `windows-test`: `cargo test --locked --manifest-path app/Cargo.toml`. Keep the `negs-marks`, `library-map`, `tray-rules` and `sheet-rules` tests while those crates live under `windows/` (`app/` uses `negs-marks`; the rules crates are `windows/`'s and go with it).
   - `windows-app`: `workspaces: app -> app/target`; build `--manifest-path app/Cargo.toml`; upload `app/target/release/Negs.exe` as `build-windows-app`.
   - `windows` (package) job: download that artifact to `app/target/release`.
   - Runners need `rc.exe` and `fxc.exe` (GPUI compiles its HLSL at build time). `windows-2025` has the Windows SDK.
5. **`installer.iss`**: no change (same exe name, mutexes, AUMID, associations).
6. **Retire `windows/src/ui/`** in a separate commit after the first GPUI release installs and updates cleanly. Delete `windows/`'s app crate and its CI steps; keep `windows/negs.manifest` only if `app/` still points at it (it has its own copy). `windows/measure_*.ps1`, `test_uninstall.ps1` and `check_original.py` move to `app/tools/` if still wanted.

## Order

1. Go from Noah.
2. One PR: items 1–4 above.
3. CI builds and packages it. Install over 0.20.x on the PC (scratch user profile first), check:
   - WinSparkle with a test appcast;
   - uninstall (`test_uninstall.ps1`);
   - Open With from Explorer while Negs runs;
   - pinned taskbar icon.
4. Release.
5. Item 6.
