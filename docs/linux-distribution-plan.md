# Plan: AppImage and Flatpak for the next Negs release

Prepared 2026-10-28 against Negs `ae042da`. Status: draft awaiting independent
review; packaging is not implemented. This replaces the earlier Flatpak-only
requirement following the request to plan both formats. This planning task does
not publish a release, install over the user's app or submit to Flathub.

## Outcome and scope

The next release must include tested x86_64 AppImage and Flatpak downloads on
GitHub Releases, alongside existing Arch/AUR/Omarchy outputs. Flatpak is the main
store/update channel; AppImage is the portable download. Submit the same released
source to Flathub, whose external review schedule must not block GitHub release.

Both formats must run without installing host GTK, libadwaita or OpenBLAS.
Graphics drivers and a supported kernel remain host requirements. Target Ubuntu
24.04 LTS as the initial AppImage compatibility baseline, the current supported
Fedora release, and current Arch/Omarchy. Establish Flatpak compatibility by
installed-app tests, not host glibc comparison. Do not promise every distribution.

Out of scope: ARM, automatic AppImage updates/desktop integration, a self-hosted
Flatpak update repository, engine changes and macOS/Windows packaging changes.

## Workflow and existing implementation

Read `AGENTS.md`: commit and push completed Negs changes directly to `main`;
external Flathub work uses PRs. Preserve unrelated work, including the existing
`linux/run.sh` modification and untracked `docs/highlights-shadows-plan.md`.
Stage explicit files and recheck the remote before pushing.

| Existing file | Packaging implication |
| --- | --- |
| `linux/Cargo.toml` | Separate GTK workspace; GTK >=4.16, libadwaita >=1.6, Rust >=1.93 |
| `linux/build.sh`, `linux/cargo.sh` | Rustup `+1.93.1` and output paths need narrow SDK overrides |
| `linux/stage.py` | Common executables/data, but native installer integration is mixed in |
| `linux/negs-launcher` | May install Omarchy config; unsuitable for new-format launchers |
| `linux/src/engine.rs` | Helper/data resolution; XDG runtime scratch paths |
| `linux/src/main.rs`, `presets.rs`, `preview_cache.rs` | App ID, XDG persistence and relocated library/driver cache identity |
| `actions.rs`, `export_ui.rs`, `preset_ui.rs` | GTK file dialogs must work through portals |
| `linux/src/photo.rs::siblings` | Parent-directory scan needs folder access |
| `linux/src/exports.rs` | Creates sibling staging file before publishing; a single-file grant may be insufficient |
| `linux/verify_package.py` | Reuse installed CPU engine checks, removing its Arch-specific extraction assumption |
| `linux/release.py`, release workflows | Immutable archives/version checks, parallel artifacts and publication dependencies |

## Phase 0: prove feasibility and freeze inputs (0.5–1 day)

Do this before wiring a complete release pipeline. Save evidence under
`target/linux-validation/distribution/`; publish only non-private summaries.

1. Resolve and pin a GNOME runtime/SDK hosted on Flathub, a compatible Rust
   extension, baseline container digest and AppImage tools/runtime revisions with
   checksums. Use the latest GNOME runtime at submission time; verify its actual
   available ref rather than assuming the desktop GNOME version matches.
2. In Ubuntu 24.04, prove a maintained GTK/libadwaita stack satisfying Negs can be
   built and bundled. Build additional GLib/Pango/etc. where needed. Record the
   dependency closure, license files and actual glibc/libstdc++ symbol floors.
   Do not choose an obsolete GTK branch solely to avoid dependency work. Never
   reuse the current Arch binaries with their newer glibc requirement.
3. Prove OpenBLAS and all native/Rust flags target broadly compatible x86_64,
   without `-march=native`. Preserve rendering and threading behavior.
4. Exercise real Flatpak helpers with selected-file access, selected-folder
   enumeration, persisted grants after restart, and sibling staging plus rename
   into a user-selected output directory.
5. Inventory available GPU hardware and portal backends. Containers establish
   CPU/ABI behavior; they do not establish real GPU/Wayland compatibility.

Exit: recorded input/version ledger and working feasibility experiments. If
AppImage dependency closure or portal behavior is substantially harder than
expected, revise the estimate and design before declaring implementation ready.
Do not silently raise the baseline or drop a required format.

## Phase 1: common payload and metadata (0.5–1 day)

Choose `io.github.noahhaasis.negs` as the permanent Linux application ID. It
matches the GitHub repository and permits owner verification. Align GTK ID,
desktop/icon filenames and metainfo ID. The current `dev.negs.app` is unsuitable
for new Flathub submissions.

- Add canonical desktop/metainfo files under `linux/` with description, screenshot
  URLs, project/source/issue links, releases, content rating and accurate licenses.
  Audit application/data/border licenses; preserve redistribution notices.
- Inventory old-ID references in staging, installation, verification, UI checks
  and Omarchy matching rules. Plan native upgrade cleanup only for obsolete
  desktop/icon files owned by the old installer. Do not edit live configuration
  during validation or delete arbitrary user files.
- Separate common staging from native installer integration. Share executable,
  data and license inventories; keep format launchers separate. Payload:
  `negs`, `spektrafilm-native`, `spektrafilm`, `spektrafilm-f64` and complete
  data. Explicitly set packaged data/helper locations in launchers.
- Preserve separate helper/Fast f32 and Reference f64 builds to prevent Cargo
  feature unification. Keep both lockfiles authoritative. Add optional toolchain
  and output-path overrides while retaining native build defaults.
- Native/AppImage keep existing XDG storage. Flatpak uses its app-specific XDG
  directories. Document separate state and preset import/export; do not request
  home access to migrate automatically. Check writable session/cache/scratch paths.
- Audit cache identity for relocated OpenBLAS and Flatpak driver paths so relevant
  library/driver changes cannot reuse invalid cached previews.

Exit: validated desktop/metainfo files and one payload contract; existing Arch
staging/install checks still pass.

## Phase 2: Flatpak packaging and file access (1–2 days)

Proposed files: `linux/flatpak/io.github.noahhaasis.negs.yml`, `flathub.json`,
generated Cargo source manifests, build entry point and minimal launcher.
Submission places packaging files at the Flathub repository root.

- Use the GNOME runtime/SDK and pinned Rust SDK extension. Generate dependency
  sources for both Cargo lockfiles, including git sources; build `--locked
  --offline` with network disabled. No rustup downloads during build. Build
  OpenBLAS from pinned source if the runtime does not supply it. Install to `/app`.
- Initially allow Wayland, fallback X11 and rendering devices; validate actual
  finish arguments with current tooling and add IPC only if needed. No blanket
  home/host, session-bus, network or device-all permissions. Restrict Flathub
  builds to x86_64 in `flathub.json`.
- Keep single-photo Open through the portal. Add explicit Open Folder to grant
  filmstrip access; opening one photo must not imply access to its siblings.
  Persist usable folder identity. If grants are revoked or a drive disappears,
  ask for reselection while preserving edits. Test Open With/file forwarding.
- Obtain writable access to a user-selected output directory and keep filename
  selection in Negs, preserving sibling-stage/rename exports. Preserve extension
  handling, overwrite confirmation and original-file protection. If a backend
  cannot support this, revise the design before release; do not silently replace
  atomic publication with truncating writes. Cover Preview/Fast/Reference/LUT.
- Preset import/export uses portals; audit atomic preset writes for the same
  file-versus-directory grant distinction.
- The sandbox launcher must not invoke native Omarchy integration. Run helper
  CPU checks inside the sandbox with isolated test state and no development-tree
  helper/data fallback.

Exit: installed Flatpak passes the acceptance matrix, including actual portals.

## Phase 3: AppImage compatibility build (1.5–2.5 days)

Proposed files: `linux/appimage/` container definition, dependency lock, build
script and `AppRun`. Use separate output/cache directories from Arch/Flatpak.

- Build all four executables and libraries in the baseline environment from
  phase 0. Use linuxdeploy and verified GTK4 support where helpful; inspect
  output rather than assuming an existing GTK plugin supports GTK4.
- Create an AppDir with common payload/metadata, license notices and source
  compliance material. Include GTK/libadwaita, OpenBLAS and dynamically loaded
  resources: schemas, GIO modules, pixbuf loaders, required fonts and toolkit
  resources. Generate relocation-safe module/schema caches and test after moving.
- `AppRun` resolves mount/extraction root, sets child-only library/resource paths,
  exports packaged helper/data locations and forwards arguments. Handle spaces,
  non-ASCII and arbitrary working directories. No dependency installation or
  desktop configuration writes on launch.
- Preserve host driver integration without bundling a build-machine NVIDIA/Mesa
  vendor driver. Verify GTK rendering as well as engine Vulkan. Inspect every
  ELF/transitive dependency and its ABI floor; avoid accidental host library mixing.
- Test both normal execution and the pinned runtime's extract-and-run fallback
  where FUSE is unavailable. Document exact runtime requirements. A portable
  executable still keeps settings in normal XDG locations.
- Publish `Negs-VERSION-x86_64.AppImage` directly with execution instructions.
  Updating initially means downloading its replacement; no automatic updater.

Exit: relocated AppImage works on baseline/current systems without development
files or host GTK/OpenBLAS; actual GPU rendering and CPU fallback pass.

## Phase 4: CI, release gates and artifact validation (1–2 days)

Add reusable Flatpak/AppImage workflows called alongside the existing Arch job.
Keep environments/caches separate. Cache keys include architecture, environment/
runtime revision, dependency lock, Rust version and both Cargo lockfiles.
Record source commit/version, dependency versions and artifact hashes.

- Relevant Linux/engine/data/tooling changes trigger package jobs; docs-only
  pushes remain cheap. Every `v*` tag/manual release candidate builds both from
  the exact checkout. Update job outputs, path filters and `publish.needs`
  together so branch skipping cannot accidentally skip tag packaging.
- Extract prefix-based installed CPU verification from `verify_package.py` and
  retain its Arch wrapper. Flatpak runs equivalent checks inside the sandbox;
  AppImage exercises its runtime/extracted payload. Add meaningful tests for
  generation/version validation, file-access changes and export publication.
- CI uses virtual-display/CPU checks. Real portal/session/hardware checks run
  before tagging; save evidence and mark unavailable hardware explicitly.
  Software Vulkan does not count as hardware validation.
- Upload separate `negs-linux-flatpak` and `negs-linux-appimage` artifacts;
  extend the publisher download pattern without filename collisions. Give new
  formats separate checksum files, or explicitly aggregate at the end. Parallel
  jobs must not overwrite `SHA256SUMS-linux`.
- Publish `Negs-VERSION-x86_64.AppImage` and
  `negs-VERSION-linux-x86_64.flatpak`, their checksums and existing assets.
  Include a runtime repository URL in the Flatpak bundle. It requires Flatpak
  and a runtime and does not itself provide an app update repository. After
  acceptance, prefer Flathub's store/ref link; test sideload-to-Flathub migration.
- Both new jobs must succeed before the existing GitHub draft/publish step.
  No overwriting published artifacts; preserve macOS update-feed ordering.

### Acceptance matrix

Apply every row to both real artifacts unless format-specific. Record source,
version, OS, desktop session, GPU/driver and result.

| Area | Required checks |
| --- | --- |
| Platforms | Ubuntu 24.04, supported Fedora, Arch/Omarchy; real Wayland and supported X11 |
| Startup | No dev fallback; launcher/Open With; spaces/non-ASCII; moved AppImage |
| Photos | JPEG/PNG/TIFF, representative RAW, portrait orientation, folder filmstrip, external drive, revoked/denied access |
| Editing | Crop/Post, borders, undo/reset, preset library/import/export, session reopening |
| Export | Preview/Fast/Reference/LUT, overwrite, original protection, cancellation, interrupted staging cleanup |
| Integrity | Dimensions/encoding/metadata; deterministic CPU fixture compared with same source/precision; investigate differences |
| GPU | Real Vulkan preview/Fast export, CPU fallback, at least one hardware GPU initially; record untested vendors |
| Persistence | Settings/presets survive replacement; Flatpak grants survive restart; native/Flatpak state separation |
| Isolation | No broad Flatpak filesystem overrides; no host GTK/OpenBLAS needed; no launch-time config installation |
| Delivery | Flatpak runtime installation and later sideload migration; AppImage extract-and-run; checksums |

## Phase 5: Flathub submission and release instructions

Prepare a reviewable submission; publishing/submitting is future implementation
work. Reference publicly available pinned source at the actual release.

1. Include canonical metadata and complete build sources upstream. Flathub builds
   source; the AppImage/`.flatpak` binary is not its submission payload. Recheck
   current lints, runtime, requirements, PR template and generative-AI policy.
   The maintainer must understand and validate the submission.
2. Establish readiness for stable distribution. Current GitHub release logic
   marks every `0.*` release as prerelease; review that rule against actual
   stability. New Flathub apps cannot begin as beta-only submissions. Do not bump
   the version during this planning task.
3. Fork `flathub/flathub`, branch from `new-pr`, add the root manifest/generated
   dependency manifests and supporting packaging, then use GitHub's web interface
   for the submission PR targeting `new-pr`. Keep its template. Suggested title:
   `Add io.github.noahhaasis.negs`. Address feedback and test builds.
4. Accept the application repository invitation with GitHub 2FA and complete
   Developer Portal verification through the GitHub owner. Future updates use
   tested PRs in that repository. Use `x-checker-data` where appropriate; ensure
   generated dependency sources remain aligned with both lockfiles.
5. Update README/Linux docs and prepare website copy for install/update/removal,
   runtime requirements, hashes and separate Flatpak state. Publish Flathub links
   only after acceptance. Review has no guaranteed deadline; ship the tested
   GitHub bundle while it is pending.

## Effort, build cost and maintenance

Budget 5–10 engineering days including troubleshooting and cross-system testing.
Phase ranges overlap; these are estimates, not a fixed delivery promise. Revisit
after phase 0, especially if maintained GTK dependencies need significant work.
Flathub review waiting time is separate.

Measured baseline: 0.10.0 Linux took 8m30s including 7m24s building/testing in
Actions run `40677221885`. Allow another 20–60 total runner-minutes for both
new jobs as an initial estimate; cold GTK/native builds can exceed that.
Parallelism reduces elapsed waiting, not total compute. Before accepting a
budget, measure cold/warm builds, peak disk and artifact sizes; set timeouts from
measurements. Use prebuilt pinned dependency environments where appropriate.

Standard hosted-runner compute is free for this public repository under current
GitHub policy. Larger runners and storage are separate. Set short CI artifact
retention and deliberate cache limits; distribute releases through GitHub Releases.
Avoid a costly multi-OS package matrix on every docs push.

Routine maintenance estimate for both: 1–3 hours per release after automation;
occasional half-day/day dependency or portal work, potentially 1–2 days for a
difficult runtime/driver migration. Budget for runtime/pin updates, AppImage
bundled-library security rebuilds, regenerated Cargo sources, real-artifact checks
and Flathub PRs. Review dependencies/runtimes periodically as well as at releases.

## References

- [Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
- [Flathub submission](https://docs.flathub.org/docs/for-app-authors/submission)
- [Flathub verification](https://docs.flathub.org/docs/for-app-authors/verification)
- [Flathub maintenance](https://docs.flathub.org/docs/for-app-authors/maintenance)
- [Flatpak permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
- [Flatpak bundles](https://docs.flatpak.org/en/latest/single-file-bundles.html)
- [AppImage packaging](https://docs.appimage.org/packaging-guide/from-source/native-binaries.html)
- [Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions)
