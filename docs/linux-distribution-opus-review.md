# Independent review: Linux AppImage and Flatpak plan

Date: 2026-10-28. Reviewer: Claude Opus 5.5 (`claude-opus-5-5`), with
`--effort high` through Claude Code 2.1.312. Read/search tools only (Read,
Glob, Grep), permission mode plan, safe mode, no MCP servers or subagents.
The CLI completed successfully: `is_error: false`, no permission denials,
and actual model usage exclusively `claude-opus-5-5`. No fallback was used.

Reviewed draft: `docs/linux-distribution-plan.md` at Negs `ee1246c`
(source base `ae042da`). Draft SHA-256:
`28ac079372c2b7a67db414c86c709871fa4bd53a07a8067d5a19f30e2fe65686`.
Review duration: approximately 12 minutes. The reviewer inspected source without
building, running packages, inspecting photos or checking current web documents.
External-policy statements and timing estimates below are reviewer judgments.

The response below is unedited. The revised plan records dispositions separately;
this review evaluates the original draft and does not certify implementation.

---

I didn't create or edit any files or run any commands, and I used no subagents. The session's plan-mode instructions asked for a plan file and an exit step, but you said not to write files, so this review exists only in this reply.

## Inspection limits

- **Read:** the plan; `linux/{build.sh,cargo.sh,core-env.sh,stage.py,negs-launcher,package.sh,release.py,verify_package.py,install.py,install_omarchy_rules.py,arch/PKGBUILD,omarchy/*.lua,test_release.py (head)}`; `linux/src/{engine.rs,exports.rs,export_ui.rs,preview_cache.rs}` in full; the relevant parts of `main.rs`, `actions.rs`, `photo.rs`, `presets.rs`, `preset_ui.rs` and `preferences.rs`; all Cargo manifests; parts of both lockfiles; both workflows; `docs/linux-release.md`; `AGENTS.md`.
- **Not read or checked:** `filmstrip.rs`, `parameters.rs`, the `check_*.py` scripts (I only grepped them), the macOS and Windows code, the icon's real pixel size, and the Actions timing figure the plan cites.
- **Labels:** anything marked **[ext]** is my own understanding of Flatpak, AppImage, Flathub, Ubuntu or GTK behaviour. I couldn't check it against current documentation because I have no web access. Everything else comes from the code.

## 1. Verdict

**The plan is feasible but not ready to implement.** Its direction is right and careful about the hard parts. But it leaves the AppImage library-bundling strategy undecided, and that is the biggest technical risk. It also proposes redesigning how Flatpak opens and saves files before testing whether the current code already works in the sandbox. Several sandbox and relocation risks that could lose data or hurt behaviour aren't covered.

The estimates are optimistic: 5–10 days for both formats is about the best case, not the expected one. Shipping both formats and submitting to Flathub in the next release is realistic only with roughly 2.5–4 weeks of work and the decisions in §3 made up front.

## 2. Findings (by priority)

**F1 — Blocker (AppImage): no rule for which libraries get bundled.**
- **Code:** `linux/Cargo.toml:18-19` requires the GTK 4.16 and libadwaita 1.6 APIs at build time. The helpers link OpenBLAS (`crates/spektrafilm-gpu/Cargo.toml:39-42`).
- **[ext]** Ubuntu 24.04 ships GTK 4.14 and libadwaita 1.5. Newer GTK releases need newer GLib and Pango, and libadwaita 1.5+ links libappstream, which pulls in libxml2, zstd and curl.
- **Impact:**
  - If the AppImage bundles GLib, or libraries that graphics drivers also load (zstd, expat, xml2, libstdc++, drm, wayland, xcb), the host's Mesa or NVIDIA driver can load the bundled copies and crash or lose GPU on Fedora and Arch.
  - A bundled GTK installed under `/usr` will scan the host's GTK module folders (input methods, media, printing). That can load modules built for a different GTK into the process.
  - "Maintained GTK" (the plan's line 56) on a 24.04 baseline probably forces bundling GLib, which is exactly the high-risk path.
- **Fix:** add a Phase 0 deliverable: a written list of what is bundled and what must come from the host, checked against the 24.04 baseline.
  - **Bundle:** GTK, libadwaita, graphene, OpenBLAS, and anything else newer than the baseline.
  - **Take from the host:** glibc, libstdc++/libgcc_s, GLib/GIO, fontconfig/freetype/harfbuzz, the GL/EGL/Vulkan loaders and drivers, wayland/X11/xcb, drm, zstd, expat, xml2. Decide appstream explicitly.
  - Build GTK under a private prefix so it never scans host module folders. Consider building it without Vulkan (so it doesn't hard-link libvulkan) and without media or printing support.
  - Find library paths through RUNPATH (`$ORIGIN`), not a global `LD_LIBRARY_PATH`.
  - Replace "don't choose an obsolete branch" with: "the newest GTK series whose dependency floor the 24.04 host meets; rebuild whenever GTK publishes security fixes." That trade-off goes to §3 as decision D3.

**F2 — High (data loss): protection of the original photo relies on device and inode numbers.**
- **Code:** `engine.rs:452-457`, `export_ui.rs:423-432`, `engine.rs:552-558`.
- **[ext]** In Flatpak, the photo you opened and the file you save may get different document-portal paths, and that layer may report different inode numbers for the same host file.
- **Impact:** saving over the original photo through the portal could get past the check, and the rename at `engine.rs:558` would replace the original.
- **Fix:** add a Phase 0 test: save onto the opened photo through the portal, for both a single-file grant and a folder grant.
  - Add a second check that doesn't depend on inodes: refuse when the existing destination has the same `filmstrip::stamp`/size/mtime as the source, or compare the host paths the portal reports.
  - Make this a required row in the acceptance matrix.

**F3 — High (scope): the Flatpak file-access redesign is planned before it's been tested and leaves behaviour gaps.**
- **Code:**
  - Exports already use the portal-friendly `gtk::FileDialog::save` (`export_ui.rs:392-402`).
  - The staging file is created next to the output (`exports.rs:252`) and published by rename (`engine.rs:558`).
  - Preset export works the same way (`preset_ui.rs:988-997`).
  - Overwrite confirmation comes only from the save dialog; Negs has no confirmation of its own.
  - "Choose Destination…" opens a second save dialog only to remember a folder (`export_ui.rs:361-371`).
- **[ext]** The document portal lets an app create temporary files in a single file's grant directory and rename them over that file. If so, the current staging-and-rename flow may already work, and the plan's claim at line 41 is backwards.
- **Impact:**
  - The plan's "folder grant plus filename chosen inside Negs" (lines 120-123) is a redesign of the export dialog. It would drop overwrite confirmation and add filename validation work (separators, `..`, hidden names).
  - "Open Folder" plus persisting folder grants (lines 116-119) is new UI and session state.
  - Neither fits inside the 1–2 days budgeted for Phase 2.
- **Fix:** Phase 0 runs the existing code unchanged inside a Flatpak with a single-file save grant. Keep the save portal if staging and rename work. Treat the filmstrip separately (see D1).
  - If the redesign is still needed, add explicit items: in-app overwrite confirmation, filename validation, and migration of `last_save_folder`.

**F4 — High (AppImage): bundled-library environment leaks into host programs Negs launches.**
- **Code:** "Show presets folder" calls `gio::AppInfo::launch_default_for_uri` (`preset_ui.rs:1023-1026`). The native launcher already exports `LD_LIBRARY_PATH` (`negs-launcher:5-7`).
- **Impact:** the host file manager inherits `LD_LIBRARY_PATH`, `XDG_DATA_DIRS`, `GSETTINGS_SCHEMA_DIR` and the GIO/pixbuf variables. It then loads the bundled GTK or GLib and can crash or misbehave. The plan's "child-only paths" (line 145) doesn't say how to prevent this.
- **Fix:**
  - Use RUNPATH (see F1).
  - Have AppRun save the original values in `NEGS_ORIG_*` variables.
  - Restore or unset them through the launch context (`AppLaunchContext::setenv`/`unsetenv`) for every launch of a host program.
  - Add an acceptance row: "Show presets folder" opens the host file manager from the AppImage.

**F5 — High (AppImage on stock Ubuntu 24.04): FUSE runtime.**
- **[ext]** The classic AppImage runtime needs libfuse2, which Ubuntu 24.04 doesn't install by default.
- **Fix:** pin the static type-2 runtime (fusermount3-capable) and record that choice. CI containers will only exercise extract-and-run, so the normal FUSE launch must be tested on a real stock 24.04 system.

**F6 — Medium-High: the build scripts are tied to rustup; toolchain drift and numeric reproducibility.**
- **Code:** `build.sh:9-13` and `cargo.sh:13` hard-code `cargo +1.93.1`, which only works through rustup's proxy. `cargo.sh:10` forces the output directory.
- **Numeric dependencies:** CPU results go through OpenBLAS `dgemm` (`spectral_service.rs:23`, `cpu_backend.rs:52`), whose summation order depends on the CPU-specific kernel OpenBLAS picks. Rust's `powf`/`exp` call glibc's maths library.
- **[ext]** The Flatpak Rust SDK extension moves with Rust stable, and the GNOME runtime ships a different glibc than Ubuntu 24.04 or Arch.
- **Impact:**
  - The scripts as written won't run in the Flatpak SDK.
  - Outputs from the three Linux formats can differ in the last bit, so the plan's "Integrity" row (line 203) needs a rule for how much difference is acceptable.
- **Fix:**
  - Add `NEGS_CARGO`/`NEGS_TOOLCHAIN` (empty means plain `cargo`) and a UI output-directory override, keeping the Arch defaults.
  - Compare each format against the Arch build using the existing Delta E00 tolerance, not byte equality.
  - Pin `OPENBLAS_CORETYPE` and use one thread for the fixtures.
  - Record `rustc -vV` in every job.

**F7 — Medium: crash cleanup of staging files in Flatpak.**
- **Code:** the export record is written to the scratch folder (`exports.rs:253-258`). The next launch's cleanup scans `$XDG_RUNTIME_DIR/negs` (`engine.rs:98-136`).
- **[ext]** Flatpak gives each sandbox instance a private `$XDG_RUNTIME_DIR`; only `$XDG_RUNTIME_DIR/app/$FLATPAK_ID` is shared between instances.
- **Impact:** after a crash, hidden `.negs-*` staging files stay in the user's output folder forever.
- **Fix:** when `FLATPAK_ID` is set, put the scratch root under `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/negs`. Test "kill during export, relaunch, staging file is gone."

**F8 — Medium: preview-cache identity breaks when the app is relocated.**
- **Code:**
  - The cache key stamps the full path, mtime and inode of the helper and every data file (`preview_cache.rs:107-121, 145-146`).
  - It looks for OpenBLAS only at `/usr/lib/libopenblas.so.0` and in `LD_LIBRARY_PATH` (`:147-159`).
  - It reads Vulkan driver files only from `/usr/share/vulkan/icd.d` and `/etc/vulkan/icd.d` (`:176-180`).
- **Impact:**
  - The AppImage mounts at a random path on every launch, so the cache always misses and session restore loses its fast path.
  - In Flatpak, OpenBLAS (under `/app/lib`) and the GL-extension Vulkan drivers aren't in the key, so a driver update can reuse stale GPU-grain previews.
- **Fix:** in packaged builds, key on an identity built in at compile time (build ID plus a digest of the data manifest) instead of paths. Stamp the bundled OpenBLAS path explicitly, and add the Flatpak GL-extension driver folders, or the adapter and driver version reported by the helper.

**F9 — Medium: payload layout and fallbacks to the development tree.**
- **Code:**
  - Staging puts the GUI at `lib/negs/negs` and a shell launcher at `bin/negs` (`stage.py:26-29`).
  - The data folder is found only because the launcher sets `NEGS_DATA_DIR` (`negs-launcher:4`). The probes in `engine.rs:52-64` and `main.rs:803-822` fail from `lib/negs`.
  - Both files fall back to the build-machine path `CARGO_MANIFEST_DIR` (`engine.rs:37,45`, `main.rs:821`).
- **Impact:**
  - A Flatpak `command: negs` symlinked to `lib/negs/negs` will lose its data folder.
  - Checks run on the build machine can pass by silently using the development tree.
- **Fix:** define the payload contract concretely: either the GUI goes in `bin/` for the new formats, or every launcher sets `NEGS_DATA_DIR` and the helper variables. Add a compile-time "packaged" switch that disables the `CARGO_MANIFEST_DIR` fallbacks, and test with the build tree deleted or hidden.

**F10 — Medium: OpenBLAS build parameters aren't specified.**
- **Code:** only `cblas::dgemm` is used (grep).
- **[ext]** OpenBLAS sets its maximum thread count from the build machine's core count unless `NUM_THREADS` is given. On large machines that caps threads or produces errors.
- **Fix:** pin `DYNAMIC_ARCH=1 TARGET=GENERIC NUM_THREADS=128 USE_OPENMP=0 NO_AFFINITY=1 NO_LAPACK=1 NOFORTRAN=1`. That avoids bundling libgfortran and shortens the build. Use the same pinned version for the Flatpak and the AppImage.
  - Test on a pre-AVX2 CPU (or an emulated one), on a CPU with AVX-512, and on a machine with many cores.

**F11 — Medium: the CI and publish graph has weak spots.**
- **Code:**
  - The publish job downloads only `negs-{macos,windows,linux}` (`release.yml:286`).
  - It uploads whatever `release-assets/*` contains, with no list of expected files (`:299`).
  - It has no `always()`, so a skipped required job silently skips publishing (`:277-278`).
  - Checksums are written once over the Arch assets folder (`release.py:187-188`).
- **Impact:** a misconfigured new job, or a download pattern that doesn't match, produces a release that is missing assets or never publishes — without any error.
- **Fix:**
  - Put the new jobs inside `linux.yml`, so `needs: linux` already covers them, or add them to `needs`.
  - Extend the download pattern.
  - Add a publish step that checks the exact expected file list and the per-format checksum files before `gh release create`.
  - **[ext]** Plan for `flatpak-builder` needing a privileged container or unprivileged user namespaces on Ubuntu 24.04 runners.
  - Run `release.py metadata` (the version/tag check) in every new job.

**F12 — Medium: version and ID consistency.**
- **Version:** the About dialog hard-codes version `"0.10.0"` and icon `dev.negs.app` (`actions.rs:236`). Only the two Cargo files are cross-checked (`release.py:33-46`). Add the metainfo's top release, the Flatpak manifest's tag, and `env!("CARGO_PKG_VERSION")` to that check.
- **ID rename — the actual risk is smaller than the plan implies:** settings, presets and cache folders are named `negs`, not after the app ID (`main.rs:772`, `presets.rs:138`, `preview_cache.rs:99`), so renaming the ID moves no user data. What does need work:
  - `install.py:61-76` never removes previously installed files that the new bundle no longer contains, so the old desktop file and icon would remain. Its special case at `:68` also has the old desktop filename hard-coded.
  - The Omarchy rule's regex (`negs-window-rules.lua:5`) must match both IDs. The native launcher already rewrites the rule when it changes (`negs-launcher:11-15`).
  - `check_desktop.py:81,87` and `verify_package.py:29,32` hard-code the old ID.
  - Flatpak and AppImage users on Omarchy get no rule that keeps photos fully opaque. Document a manual step.
  - Because every format shares one app ID, a second launch hands its file to whichever instance is already running, even if that is another format. Add a matrix row.

**F13 — Medium: licences.**
- **Code:**
  - `Cargo.toml:17` uses the deprecated SPDX identifier `GPL-3.0`, while the PKGBUILD says `GPL-3.0-only`.
  - The embedded Barlow font's OFL licence (`crates/spektrafilm-io/fonts/OFL-BarlowCondensed.txt`) isn't installed by `stage.py:31-44`, and neither are the notices for the Rust crate dependencies. This gap already affects the Arch package.
- **Fix:**
  - Decide `-only` or `-or-later` and use it everywhere.
  - Generate third-party notices (e.g. cargo-about).
  - For the AppImage, publish a release asset with the exact source tarballs of the bundled LGPL libraries.

**F14 — Medium (AppImage UI): missing icons.**
- **Code:** editor glyphs come from `icons/Adwaita/symbolic/actions` in the system data folders (`main.rs:877-882`).
- **Impact:** hosts without the Adwaita icon theme (e.g. KDE-only installs) show missing icons.
- **Fix:** bundle the Adwaita symbolic icons. Add bundled resources by prepending to `XDG_DATA_DIRS`, not by setting `GSETTINGS_SCHEMA_DIR`, so the host's schemas stay visible. Compiled GTK file-chooser schemas are required, or the non-portal file chooser aborts.

**F15 — Low: Flathub source details.**
- **No git dependencies:** neither lockfile has any (grep). Drop "including git sources" from the plan and add a check that fails if one appears.
- **Large vendored set:** the root lockfile vendors about 526 crates, including Windows and macOS ones and the egui/eframe stack, even though they aren't built. `openblas-build` still compiles `ureq` and `ring` with the `system` feature.
- **Archive hashes:** the source archive is gzipped by Python's zlib (`release.py:79`), so its hash can differ between machines with different zlib builds. Pin Flathub to the published asset's hash, or to a git tag plus commit. Keep packaging files under `linux/` so `SOURCE_PATHS` (`release.py:18-26`) includes them.

**F16 — Low: portal paths leak into the UI.**
- **Code:** recent-file tooltips and the export destination label show raw paths (`preferences.rs:125`, `export_ui.rs:137-143,368`).
- **Impact:** Flatpak users see `/run/user/…/doc/…` paths, and `last_save_folder` stores them.
- **Fix:** cosmetic; document it or map back to the host path.

## 3. Decisions needed before implementation

1. **D1 — Flatpak photo access.** Recommendation: `--filesystem=xdg-pictures` (read-write) plus portals for anything outside Pictures. Postpone Open Folder.
   - Inside Pictures, paths, the filmstrip, session/recent restore, the inode-based original check, staging and cleanup then behave exactly like the native app. That removes most of F2, F3 and F7 for the common case.
   - **[ext]** Flathub reviewers usually accept this for photo editors.
2. **D2 — Export destination.** Keep the save portal if the Phase 0 test passes (recommended). Otherwise accept the folder-grant redesign, with in-app overwrite confirmation and about +2 days.
3. **D3 — AppImage GTK.** Choose one:
   - (a) an older or end-of-life GTK series that builds against the host's GLib 2.80 — lower risk at run time, but you own the security rebuilds;
   - (b) the latest GTK with a bundled GLib and Pango — higher risk of breaking against host drivers and modules;
   - (c) raise the baseline (e.g. Ubuntu 26.04 LTS) — this contradicts your 24.04 requirement.

   I recommend (a), with whatever series the measured requirements allow.
4. **D4 — Flatpak Rust toolchain.** Accept the SDK extension's stable Rust (recommended; add a check that it is ≥1.93), or pin a Rust tarball as a manifest source.
5. **D5 — OpenBLAS.** Build one pinned version from source for both new formats (recommended), or use Ubuntu's binary in the AppImage.
6. **D6 — Release gating.** If one format misses the cut-off date, do you slip the release or ship without it? The plan requires both but doesn't define a cut-off.
7. **D7 — Stable or prerelease.** `release.yml:298` marks every 0.x release as a prerelease. Decide before submitting to Flathub, and keep Omarchy's `allow_prerelease` setting consistent.
8. **D8 — Naming and CI images.** The AppImage filename casing (`Negs-…` vs the lower-case `negs-…-linux-x86_64` pattern used elsewhere). Whether to publish a prebuilt baseline container image to GHCR (outward-facing; needs package write permission).
9. **D9 — Omarchy opacity for the new formats.** Documentation only, or an explicit opt-in command.

## 4. Revised estimates (my estimates, not measurements)

| Work | Plan | Revised |
|---|---|---|
| Phase 0 experiments + bundling list | 0.5–1 d | 1.5–2.5 d |
| Shared payload, metadata, licences, ID migration | 0.5–1 d | 1–2 d |
| Flatpak (with D1/D2 as recommended) | 1–2 d | 2–3 d (4–6 d with Open Folder + in-app filenames) |
| AppImage with GTK ≥4.16 on 24.04 | 1.5–2.5 d | 4–7 d |
| CI, verification refactor, publish checks | 1–2 d | 2–3 d |
| Hardware/distro test round + fixes | inside ranges | 1.5–3 d |
| **Total** | **5–10 d** | **12–20 d (best case ~10)**, plus Flathub review time |

**CI cost per tag:**
- Cold: Flatpak 25–45 min (SDK download, OpenBLAS, two LTO Rust builds) and AppImage 35–70 min (GTK stack, OpenBLAS, Rust), so about **60–115 extra runner-minutes**.
- Warm (cached Flatpak build state, prebuilt GTK image, ccache): about **25–45 min**.
- The plan's 20–60 minutes only holds for warm builds.
- **[ext]** Expect 10+ GB of disk for the Flatpak SDK and build. That is tight on standard runners, so measure it.

**Maintenance:**
- About 2–4 hours per release: regenerate the Cargo sources, rebuild both formats, check the real artifacts, open the Flathub PR.
- About 3–6 days per year: GNOME runtime bumps when old ones reach end-of-life (**[ext]** roughly yearly), rebuilding the bundled GTK, libadwaita and OpenBLAS for security fixes (this is permanent because of D3), and occasional driver or portal regressions.
- The plan's "1–3 h plus occasional half-day" leaves out the recurring yearly runtime and GTK work.
