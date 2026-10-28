# Plan: AppImage and Flatpak for the next Negs release

Prepared 2026-10-28 against Negs `ae042da`; revised after an independent
[Claude Opus 5.5 High review](linux-distribution-opus-review.md) of draft `ee1246c`.
Status: implemented for 0.11.0; see [Linux Flatpak and AppImage](linux-distribution.md)
for what was built, the phase 0 decisions and test results, and what remains
untested. Flathub submission is a manual step after the release.

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
| `linux/src/exports.rs` | Creates sibling staging file before publishing; test existing single-file grants before redesigning |
| `linux/verify_package.py` | Reuse installed CPU engine checks, removing its Arch-specific extraction assumption |
| `linux/release.py`, release workflows | Immutable archives/version checks, parallel artifacts and publication dependencies |

## Phase 0: prove feasibility and freeze inputs (1.5–2.5 days)

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
   Produce a library ownership ledger: bundled versus host, supported minimum
   versions, and which libraries host graphics drivers can load. Prefer host
   glibc, libgcc/libstdc++ where ABI permits, graphics loaders/drivers and display
   protocol libraries. Bundle the maintained GTK stack under a private prefix;
   explicitly assess GLib/Pango/appstream and every transitive dependency. No
   universal exclusion list is assumed safe without the measured dependency graph.
   Use relative RUNPATHs rather than a blanket LD_LIBRARY_PATH. Prevent toolkit
   module discovery from loading unrelated host GTK modules. Do not claim this
   architecture works until real baseline and current-driver tests pass.
3. Build the same pinned OpenBLAS source for both new formats. Start by validating
   DYNAMIC_ARCH=1, TARGET=GENERIC, NUM_THREADS=128, USE_OPENMP=0, NO_AFFINITY=1,
   NO_LAPACK=1 and NOFORTRAN=1 against that release's supported options and actual
   dgemm use. Record final settings; never guess a supported option combination.
   Verify all native/Rust flags avoid `-march=native`, with pre-AVX2 and high-core
   CPU coverage where available. Existing Linux openblas-src uses `system`;
   expose the staged library through pkg-config, with no Cargo network build.
4. Run the existing Flatpak save dialogs unchanged first: single-file grants may
   already permit temporary siblings and replacement. Test photo and preset save,
   overwrite, cancellation, failure and relaunch on the actual portal backends.
   Separately test folder enumeration/grant persistence for the filmstrip.
   Attempt export onto the opened source through different single-file/folder
   document grants, symlinks and aliases. Device/inode equality alone must not
   be assumed to identify one host file through different portal views.
5. Inventory available GPU hardware and portal backends. Containers establish
   CPU/ABI behavior; they do not establish real GPU/Wayland compatibility.

Exit: recorded input/version and library-ownership ledgers, supported toolchain/
ABI decisions, existing-save-dialog results and source-protection experiments. If
AppImage dependency closure or portal behavior is substantially harder than
expected, revise the estimate and design before declaring implementation ready.
Do not silently raise the baseline or drop a required format.

## Phase 1: common payload and metadata (1–2 days)

Choose `io.github.noahhaasis.negs` as the permanent Linux application ID. It
matches the GitHub repository and permits owner verification. Align GTK ID,
desktop/icon filenames and metainfo ID. The current `dev.negs.app` is unsuitable
for new Flathub submissions.

- Add canonical desktop/metainfo files under `linux/` with description, screenshot
  URLs, project/source/issue links, releases, content rating and accurate licenses.
  Audit application/data/border licenses and the embedded Barlow font's OFL.
  Resolve GPL-3.0-only versus GPL-3.0-or-later from the existing grants before
  aligning Cargo/PKGBUILD/metainfo; do not infer permission to relicense.
  Generate third-party Rust/native notices. Publish exact corresponding source
  for bundled copyleft libraries and any required modifications/build instructions.
- Inventory old-ID references in staging, installation, verification, UI checks
  and Omarchy matching rules. Plan native upgrade cleanup only for obsolete
  desktop/icon files owned by the old installer. Do not edit live configuration
  during validation or delete arbitrary user files.
- Separate common staging from native installer integration. Share executable,
  data and license inventories; keep format launchers separate. Concrete layout:
  `bin/negs` is the format launcher; `lib/negs/` contains all four executables;
  `share/negs/data` contains data, with the respective /app or AppDir prefix.
  Payload:
  `negs`, `spektrafilm-native`, `spektrafilm`, `spektrafilm-f64` and complete
  data. Explicitly set packaged data/helper locations in launchers. Add packaged
  build behavior that disables CARGO_MANIFEST_DIR development fallbacks in both
  UI and engine resolution; verify with the development tree hidden/unavailable.
- Preserve separate helper/Fast f32 and Reference f64 builds to prevent Cargo
  feature unification. Keep both lockfiles authoritative. Add optional toolchain
  and output-path overrides while retaining native build defaults. SDK builds use
  plain cargo (no rustup + selector), verify Rust >=1.93 and record rustc -vV.
  Pin the SDK/toolchain content used per release; deliberately refresh later.
- Native/AppImage keep existing XDG storage. Flatpak uses its app-specific XDG
  directories. Document separate state and preset import/export; do not request
  home access to migrate automatically. Check writable session/cache/scratch paths.
- In packaged cache keys, replace transient AppImage mount paths/inodes for
  immutable payload files with a stable build ID plus data-manifest digest.
  Include actual bundled OpenBLAS and Flatpak graphics-extension driver identity.
  Test cache hits across AppImage relaunch/relocation and invalidation on build,
  data, OpenBLAS or driver changes; retain source-photo invalidation separately.
- Derive the About version from CARGO_PKG_VERSION; update its icon ID. Validate
  both Cargo versions, metainfo release, manifest source and release tag together.
- Define cross-format activation: the same application ID normally forwards a
  launch to an existing instance. Test native/AppImage/Flatpak combinations and
  file grants. If a receiver cannot read the forwarded file, ask for selection
  there and preserve the running session; do not silently drop the request.
- Keep new-format startup free of Omarchy writes. Document optional manual
  opacity matching for the new ID, and preserve old/new matching in the native
  packaged rule during upgrades. Any later live-config work follows its skill.

Exit: validated desktop/metainfo files and one payload contract; existing Arch
staging/install checks still pass.

## Phase 2: Flatpak packaging and file access (3–5 days)

Proposed files: `linux/flatpak/io.github.noahhaasis.negs.yml`, `flathub.json`,
generated Cargo source manifests, build entry point and minimal launcher.
Submission places packaging files at the Flathub repository root.

- Use the GNOME runtime/SDK and pinned Rust SDK extension. Generate dependency
  sources for both Cargo lockfiles; neither currently has git dependencies.
  Fail dependency-generation checks if a new source type is not captured. Build `--locked
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
- Keep the existing save portal if phase 0 confirms temporary-sibling/rename
  behavior. Preserve its overwrite confirmation. Only if that fails, obtain a
  writable directory grant and choose the filename in Negs. That fallback must
  add in-app overwrite confirmation, filename validation (no separators/..),
  correct extension handling and migration/validation of last_save_folder.
  Budget another 2–3 days if this redesign is necessary. Never replace atomic
  publication with truncating writes. Cover Preview/Fast/Reference/LUT.
- Strengthen original protection across portal aliases: retain native inode
  checks and compare portal-provided host identity when available. Documents
  GetHostPaths (interface v5) or user.document-portal.host-path may help; feature
  detect and test older backends. Host paths are identity/display metadata, not
  authority to bypass grants. When alias identity cannot be established, refuse
  replacing an existing destination and ask for a new filename. Matching only
  size/mtime is not a reliable file-identity algorithm. Recheck before publication.
- Preset import/export uses portals; audit atomic preset writes for the same
  file-versus-directory grant distinction.
- Put Flatpak crash-recovery records in a supported per-application runtime
  location shared across instances; account for already scoped runtime paths.
  Test SIGKILL during export, relaunch and removal of the owned leftover staging
  file. If grants are revoked, preserve the error and provide a recovery message.
  Display friendly document names/host paths where safely available instead of
  raw portal paths; persisted grants remain the actual access mechanism.
- The sandbox launcher must not invoke native Omarchy integration. Run helper
  CPU checks inside the sandbox with isolated test state and no development-tree
  helper/data fallback.

Exit: installed Flatpak passes the acceptance matrix, including actual portals.

## Phase 3: AppImage compatibility build (4–7 days)

Proposed files: `linux/appimage/` container definition, dependency lock, build
script and `AppRun`. Use separate output/cache directories from Arch/Flatpak.

- Build all four executables and libraries in the baseline environment from
  phase 0. Use linuxdeploy and verified GTK4 support where helpful; inspect
  output rather than assuming an existing GTK plugin supports GTK4.
- Create an AppDir with common payload/metadata, license notices and source
  compliance material. Include GTK/libadwaita, OpenBLAS and dynamically loaded
  resources: schemas, GIO modules, pixbuf loaders, required fonts and toolkit
  resources and Adwaita symbolic icons. Generate relocation-safe module/schema
  caches and test after moving. Check a KDE-only host without Adwaita installed
  and both portal/non-portal chooser paths; preserve schema lookup requirements.
- `AppRun` resolves mount/extraction root, uses the phase-0 RUNPATH policy and
  narrowly scoped resource paths,
  exports packaged helper/data locations and forwards arguments. Handle spaces,
  non-ASCII and arbitrary working directories. No dependency installation or
  desktop configuration writes on launch.
- Save original resource-environment values as NEGS_ORIG_* and restore/unset
  bundle variables in AppLaunchContext when launching host programs. Audit
  Show presets folder / gio::AppInfo launch paths and test the real host file
  manager; setting variables for Negs alone does not prevent child inheritance.
- Preserve host driver integration without bundling a build-machine NVIDIA/Mesa
  vendor driver. Verify GTK rendering as well as engine Vulkan. Inspect every
  ELF/transitive dependency and its ABI floor; avoid accidental host library mixing.
- Select a pinned static type-2 runtime with verified FUSE/fusermount3 behavior
  on stock Ubuntu 24.04, without assuming libfuse2 is installed. Test both normal
  execution and the pinned runtime's extract-and-run fallback
  where FUSE is unavailable. Document exact runtime requirements. A portable
  executable still keeps settings in normal XDG locations.
- Publish `Negs-VERSION-x86_64.AppImage` directly with execution instructions.
  Updating initially means downloading its replacement; no automatic updater.

Exit: relocated AppImage works on baseline/current systems without development
files or host GTK/OpenBLAS; actual GPU rendering and CPU fallback pass.

## Phase 4: CI and release gates (2–3 days), desktop validation (1.5–3 days)

Add reusable Flatpak/AppImage workflows called alongside the existing Arch job.
Keep environments/caches separate. Cache keys include architecture, environment/
runtime revision, dependency lock, Rust version and both Cargo lockfiles.
Record source commit/version, dependency versions and artifact hashes.
Prove flatpak-builder's user-namespace/container requirements on the actual
runner, and measure disk usage before adding a larger matrix. Start with cached
local dependency builds; publishing a GHCR build image is a later concrete
infrastructure decision, not an implicit requirement of this plan.

- Relevant Linux/engine/data/tooling changes trigger package jobs; docs-only
  pushes remain cheap. Every `v*` tag/manual release candidate builds both from
  the exact checkout. Update job outputs, path filters and `publish.needs`
  together so branch skipping cannot accidentally skip tag packaging.
- Call the existing release metadata validator in each new job. Generate a local
  CI manifest source pointing at the exact checkout/source archive for pre-release
  builds; generate the separate Flathub manifest source from the published source
  hash or tag plus immutable commit. Never need an unpublished release URL to
  build the release, or recompute a published archive's hash on another machine.
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
  Before creating the draft, verify an explicit versioned list of expected
  assets and all checksums, failing on missing or mismatched outputs. A broad
  artifact download glob alone is insufficient. Test missing/failed/skipped job
  cases and successful tags. Both formats are required: if either fails, defer
  the tag/publication until fixed; do not silently publish one format.

### Acceptance matrix

Apply every row to both real artifacts unless format-specific. Record source,
version, OS, desktop session, GPU/driver and result.

| Area | Required checks |
| --- | --- |
| Platforms | Ubuntu 24.04, supported Fedora, Arch/Omarchy; real Wayland and supported X11 |
| Startup | No dev fallback; launcher/Open With and cross-format activation; spaces/non-ASCII; moved AppImage; host file-manager launch |
| Photos | JPEG/PNG/TIFF, representative RAW, portrait orientation, folder filmstrip, external drive, revoked/denied access |
| Editing | Crop/Post, borders, undo/reset, preset library/import/export, session reopening |
| Export | Preview/Fast/Reference/LUT, overwrite, original protection through different portal aliases, cancellation, SIGKILL/relaunch staging cleanup |
| Integrity | Dimensions/encoding/metadata; same-source/precision CPU fixtures with fixed BLAS kernel and one thread; existing golden tolerance categories for cross-runtime numeric comparison, not byte equality |
| GPU | Real Vulkan preview/Fast export, CPU fallback, at least one hardware GPU initially; record untested vendors |
| Persistence | Settings/presets survive replacement; Flatpak grants survive restart; native/Flatpak separation; AppImage cache hits across mounts; library/driver cache invalidation |
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

Budget 15–25 engineering days (roughly 3–5 working weeks) for the retained scope,
including cross-system testing and troubleshooting. Claude estimated 12–20 days
with Pictures access and a narrower GTK strategy; this plan keeps explicit folder
access and maintained dependencies. The earlier 5–10 days was optimistic. Phase
ranges are planning allowances, not a fixed promise; add 2–3 days if a new export
folder/filename flow proves necessary. Re-estimate after phase 0; Flathub review
waiting time is separate.

Measured baseline: 0.10.0 Linux took 8m30s including 7m24s building/testing in
Actions run `40677221885`. Reviewer planning estimates for the new jobs: cold
Flatpak 25–45 minutes, cold AppImage 35–70 minutes, or 60–115 extra runner-minutes
combined; warm jobs approximately 25–45 combined. These are unmeasured allowances,
and a maintained GTK stack may exceed them. The earlier 20–60 estimate assumed
effective caching and did not cover the full cold dependency build.
Parallelism reduces elapsed waiting, not total compute. Before accepting a
budget, measure cold/warm builds, peak disk and artifact sizes; set timeouts from
measurements. Use prebuilt pinned dependency environments where appropriate.

Standard hosted-runner compute is free for this public repository under current
GitHub policy. Larger runners and storage are separate. Set short CI artifact
retention and deliberate cache limits; distribute releases through GitHub Releases.
Avoid a costly multi-OS package matrix on every docs push.

Routine maintenance estimate for both: 2–4 hours per release after automation,
plus roughly 3–6 engineering days per year for runtime upgrades, bundled-library
security updates and compatibility work. A difficult migration may take longer.
These are estimates and assume the phase-0 strategy succeeds. Budget for AppImage
bundled-library security rebuilds, regenerated Cargo sources, real-artifact checks
and Flathub PRs. Review dependencies/runtimes periodically as well as at releases.

## Review disposition and remaining gates

The [unedited review](linux-distribution-opus-review.md) used the exact requested
model and high effort. It reviewed the original draft, not these revisions.

| Finding | Disposition |
| --- | --- |
| F1 / D3: library strategy | Added ownership ledger, private prefix, RUNPATH and driver/module tests. Retain maintained libraries; decline shipping an EOL GTK as the default. Ubuntu 24.04 remains a feasibility gate, not an unproved guarantee. |
| F2: portal alias source protection | Added destructive-target tests and portal host-identity checks with conservative refusal when unresolved. Declined size/mtime alone as identity. |
| F3 / D1–D2: file-access scope | Test unchanged save portals first; redesign only if necessary. Keep explicit folder grants for filmstrip; decline blanket Pictures access as the default under current Flathub portal requirements. |
| F4: host program environment | Added original-environment restoration and host file-manager test. |
| F5: FUSE | Added static-runtime selection and stock-baseline normal-launch test. |
| F6 / D4: toolchain and numeric comparison | SDK plain cargo with minimum check and recorded version; fixed BLAS fixtures and existing tolerance categories. |
| F7: crash recovery | Added shared per-app runtime records and actual crash/relaunch cleanup tests; verify backend behavior. |
| F8: cache identity | Stable build/data identity plus relocated BLAS/driver identity and cache hit/invalidation checks. |
| F9: payload and development fallback | Specified layout, explicit launcher environment and disabled packaged fallbacks. |
| F10 / D5: OpenBLAS | Same pinned source and explicitly validated portable options for both formats. |
| F11 / D6: CI/publication | Per-job metadata, runner feasibility, explicit expected-asset checks and no partial-format release. |
| F12 / D7 / D9: IDs, versions, activation, opacity | Added About/metadata version checks, owned-file cleanup, activation matrix and manual opacity guidance. Stable-channel readiness remains required before Flathub submission; no version chosen now. |
| F13: licenses | Font and third-party notices, license consistency audit and copyleft corresponding sources. |
| F14: icons | Bundle Adwaita symbols and test without a host Adwaita theme. |
| F15: dependency/archive identity | Current dependency-type guard and published source hash/commit; no regenerated hash assumption. |
| F16: portal display paths | Friendly display identity while grants remain authoritative. |
| D8: naming/build images | Keep explicitly specified filenames; initial builds need no newly published GHCR image. |

No empirical AppImage/Flatpak feasibility, portal behavior, GPU compatibility or
build timing is certified by this review. Start phase 0 next; only promote the
later phases to implementation-ready after its documented exits pass.

## References

- [Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
- [Flathub submission](https://docs.flathub.org/docs/for-app-authors/submission)
- [Flathub verification](https://docs.flathub.org/docs/for-app-authors/verification)
- [Flathub maintenance](https://docs.flathub.org/docs/for-app-authors/maintenance)
- [Flatpak permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
- [Document portal identity](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Documents.html)
- [Flatpak bundles](https://docs.flatpak.org/en/latest/single-file-bundles.html)
- [AppImage packaging](https://docs.appimage.org/packaging-guide/from-source/native-binaries.html)
- [Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions)
