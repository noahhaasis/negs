> Integration has now proceeded beyond this pre-core handoff. For the current
> connected app, build/package commands and evidence, read [linux-app.md](linux-app.md)
> and [linux-core-validation.md](linux-core-validation.md).

# Linux core bring-up and GTK integration handoff

Prepared 2026-10-25. Work in `/home/noah/Work/Negs/.worktrees/linux-omarchy-port`
on the current `linux-omarchy-port` branch. Do not push, merge, publish a
package, or implement the separate Python experimental migration.

## Request and current boundary

The user requested a native GTK4/libadwaita Linux/Omarchy port, then explicitly
asked to build and review the UI before connecting Spektrafilm. That interface
exists and runs in the real desktop. This handoff is for the subsequent agent
to bring up the existing Rust core and connect it to that interface.

Read **all** of [linux-agent-handoff.md](linux-agent-handoff.md) first; its
feature, correctness, startup and packaging requirements remain authoritative.
That supplied file is currently untracked; preserve it. Also read:

1. [linux-app.md](linux-app.md), [linux-ui-parity.md](linux-ui-parity.md) and
   [linux-ui-validation.md](linux-ui-validation.md).
2. [cross-platform-port.md](cross-platform-port.md),
   [windows-ui-features.md](windows-ui-features.md) Behavior sections,
   [native-macos-ui.md](native-macos-ui.md), [windows-app.md](windows-app.md),
   [image-size.md](image-size.md) and [upstream-sync.md](upstream-sync.md).
3. `scripts/golden/README.md` including Watch next, `scripts/golden/gates.json`,
   and `scripts/check_native_macos.py` (portable despite its name).
4. The actual helper, core and native-client sources listed below. Older
   feature inventories are incomplete; current macOS/Windows behavior matters.

**There is no Linux engine baseline yet.** No core/helper build, Spektrafilm
Vulkan computation, rawler decode, developed preview, real image/LUT export,
or numerical parity has been validated here. The GTK renderer's confirmed
NVIDIA Vulkan use proves presentation acceleration only.

UI work is being completed concurrently by a fresh-context agent named
`remaining_linux_ui`. Inspect `git status` and coordinate before touching
`linux/src/`, its tests or the parity audit. Engine setup, helper validation
and the immutable baseline can proceed independently. Re-read the final UI
commit/audit before integrating. Do not overwrite that agent's pending edits.

## Existing work to preserve

Engine starting revision is `93b6a85e23de64c28c4e3f8c11ea6837a37e2359`.
Core sources/root Cargo.lock have not been changed during UI prototyping.
Coherent local UI commits through this handoff:

- `1aa6d08`: native GTK UI and desktop performance harness.
- `f131a1f`: initial validation report.
- `88b3e09`: stable film-strip navigation and stale-load protection.
- `93a1572`: opaque canvas and Negs-only Omarchy opacity rule.
- `7b3ae74`: configurable borders, preview installer and full parity audit.

Later UI commits may supersede these details. Keep the established fixes:

- Clicking the selected photo does not reload or clear history. Same-folder
  navigation retains thumbnail widgets/textures. The existing image stays
  visible until the replacement is ready; stale results cannot replace it.
- The canvas always paints an opaque neutral backing. Source transparency
  never reveals desktop content. Opaque texture pixels alone cannot override
  a compositor's whole-window opacity policy.
- `linux/omarchy/negs-window-rules.lua` removes Omarchy's `default-opacity`
  tag for `dev.negs.app` and `dev.negs.app.UiPreview`, and forces focused,
  inactive and fullscreen opacity to `1 override`. The idempotent installer
  backs up/includes the user config. It is already applied on this desktop.
  Keep the app-specific rule and installation behavior when packaging.
- Border controls include mat/line/outer-film thickness, opaque sRGB colors
  and hex entries, Solid/Film/Keyline/35mm textures, Instagram framing,
  include-in-export, undo and session restoration. Validate against real
  export pixels; the current canvas geometry is still presentation code.
- Deferred inspector construction, native accessible controls, semantic
  light/dark chrome, narrow-window breakpoints and asynchronous photo loading
  must survive engine integration.

## Machine and reproducible build environment

Current tested desktop: Omarchy 4.0.6, Linux 7.2.8-9-omarchy, Hyprland
0.56.2-4 with Lua configuration, Wayland. Ryzen 5 3600 (6C/12T), 16 GiB RAM,
NVIDIA GTX 1660 SUPER (6 GiB), NVIDIA 615.78.08. GTK 4.24.1,
libadwaita 1.10.0; Dell 3440×1440 at 59.97 Hz and 1.25 scale.
Refresh these observations rather than treating them as permanent.

Rust 1.93.1 (`01f6ddf75`, 2026-02-11), rustfmt and Clippy were installed in
the ignored checkout-local toolchain. Bare `cargo`/`rustc` are not necessarily
on PATH. For **root engine** commands, from the checkout:

```sh
export RUSTUP_HOME="$PWD/target/linux-toolchain/rustup"
export CARGO_HOME="$PWD/target/linux-toolchain/cargo"
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TARGET_DIR="$PWD/target"
cargo +1.93.1 --version
```

`linux/cargo.sh` instead forces `linux/Cargo.toml` and `target/linux-ui`.
Use it for the UI only. The UI has a separate locked workspace using
gtk4-rs 0.10.3/libadwaita 0.8.1, GTK 4.16/adw 1.6 feature levels.

At handoff, `gfortran` was absent and `pkg-config openblas` failed. GTK and
base build tools are installed. Audit the root Linux dependencies before
building: `spektrafilm-gpu` and `spektrafilm-core` select cblas/blas-src with
OpenBLAS on non-macOS/non-Windows targets. Establish the compiler, Fortran,
BLAS/linking dependencies and runtime package dependencies. Make minimal
Linux-specific build fixes; preserve summation order and numerical behavior.
Core crates are not all cached. An offline root build previously stopped at
missing `libm`; UI dependencies being cached does not establish core readiness.

Use the actual logged-in Wayland session for GUI tests. This session currently
exposes `WAYLAND_DISPLAY=wayland-1`, `DISPLAY=:0`,
`XDG_RUNTIME_DIR=/run/user/1000`; discover/verify rather than hard-code them.
GUI/network/system writes may need the environment's normal escalation.
Do not change global desktop opacity, GPU driver watchdogs or timeout policy.

## Bring up and baseline before integration

Build separate f32 Fast/helper and f64 Reference binaries; do not enable
`precision-f64` for the whole workspace or the native helper. Preserve both.

```sh
cargo +1.93.1 build --release --locked -p spektrafilm-cli \
  --bin spektrafilm --bin golden_compare --bin golden_probes --bin golden_inputs
cargo +1.93.1 build --release --locked -p spektrafilm-native
cargo +1.93.1 build --release --locked -p spektrafilm-cli \
  --features precision-f64 --bin spektrafilm-f64
python3 -I -m unittest discover -s scripts/golden -p 'test_*.py'
python3 -I scripts/check_native_macos.py --expected-backend wgpu
SPEKTRAFILM_BACKEND=cpu python3 -I scripts/check_native_macos.py --expected-backend cpu
```

Inspect actual Vulkan adapters/backend logs. Lavapipe/llvmpipe does not prove
hardware GPU use. Test honest CPU fallback. The memory-admission/device-limit
guard in `crates/spektrafilm-gpu/src/wgpu_backend.rs` is currently
Windows-only in several places; it does not protect Linux allocations.
Audit Linux limits/admission before large renders. Begin with small inputs,
use bounded experiments, then attempt the 24 MP TIFF. Report device loss or
recovery limitations without weakening driver safeguards.

Run the existing matrix **before numerical engine changes**, retaining its
manifest, binaries/provenance and dumps as an immutable Linux baseline:

```sh
python3 -I scripts/golden/run.py run --label linux-omarchy-baseline \
  --out target/linux-validation/linux-baseline.json \
  --dump-dir target/golden/dumps/linux-baseline
python3 -I scripts/golden/run.py quality target/linux-validation/linux-baseline.json \
  --dumps target/golden/dumps/linux-baseline \
  --report target/linux-validation/linux-quality.json
```

Matrix: 184 cases / 484 renders (116 f32 CPU, 184 f64, 184 GPU), roughly
6 GiB of dumps. Keep every transferred input unchanged. Minimal changes
needed just to compile/run Linux must be recorded before the first baseline;
do not disguise that bootstrap state as an untouched reference build.

## Existing protocol and code to reuse

Use the persistent serial `crates/spektrafilm-native/src/main.rs` helper:

```text
spektrafilm-native --data-dir /absolute/installed/data
{"id":1,"command":"catalog"}
```

Each request is one JSON line with a monotonically assigned `u64` id and a
flattened snake_case command. Replies are `{id,result:...}` or `{id,error:...}`.
Stdout is protocol-only; continuously drain stderr. Check catalog protocol
version (currently 1), match response ids, bound/validate input, and treat EOF,
malformed responses and process failure as recoverable editor errors.
There is no quit command: close stdin/EOF and reap the child. The helper
catches panics and clears its Session; a previous retained render is then lost.

| Command | Contract and integration consequence |
|---|---|
| `catalog` | Returns `protocol`, full `profiles` metadata and actual `RuntimeParams::default()`. Profile fields include `id`, `name`, `paper`, `positive`, `bw`, `target_print`, `development_times`. Use profile IDs rather than display labels. |
| `render` | Takes `input`, `source_generation`, `film`, `paper`, `params`, `limit_preview`, `preview_path`, optional `interactive_scale` and `crop`. Returns display, source, photograph, working and retained-output dimensions, `export_resolution`, `raw`, actual `backend`, `elapsed_ms`. Successful noninteractive state can become saveable; validate its revision/generation first. |
| `original` | Takes `input`, `params`, `limit_preview`, `preview_path`. Produces the matched original without replacing the retained developed render. Do not simulate Original by running a second film or clearing the developed result. |
| `save` | Takes current retained `render_id`, `output`, optional `border`, `border_options`, `instagram`, `crop`, clockwise quarter-turn `rotate_cw`, `fit`. Rejects stale render IDs and overwriting the source. Uses retained float pixels, then crop → Post → rotation → border → Instagram → final resize → save. |
| `export_lut` | Takes `film`, `paper`, `params`, `size`, `output`; no photograph required. |

`preview_path` is a **raw tightly packed RGBA8 byte buffer**, not PNG/JPEG:
exactly `width * height * 4`, sRGB display encoding, alpha 255. Dimensions are
positive and at most 8192 per side. Use checked arithmetic and exact byte-count
validation before allocating/uploading a GDK texture. Read off the GTK thread;
do not pass the file to GdkPixbuf's encoded-image loader. Assign the proper
sRGB color state for the installed GTK API, avoiding double encoding. Inspect
`spektrafilm_core::preview::display_rgba_with_post` and existing clients.

Useful shared sources:

- `crates/spektrafilm-core/src/{params,preview,image_size,post}.rs`:
  full runtime schema, cache key, preview sizing, Image Size resolution and
  post-only reuse. Image Size resamples **before film**, once; export Fit
  resamples the finished print. Neither replaces the other.
- `crates/spektrafilm-io/src/{crop,border,film35,resize,white_balance}.rs`:
  crop/straighten validation and rounding, border geometry, RAW WB and fit.
  Crop JSON is `{x,y,width,height,angle}` in normalized unrotated coordinates;
  quarter-turn rotation is separate. `Fit` serializes as e.g.
  `{"long_edge":2048}`. Solid border serializes as `"white"`, 35mm as `"35mm"`.
- `windows/src/{engine,model,rendered_preview}.rs`: proven child protocol,
  scheduling, parameter mapping, reference export and cache behavior. Windows
  process/job-object and Direct2D code is platform-specific; reuse the design.
- `macos/Sources/NegsApp/{Engine,EditorModel,RenderedPreviewCache,Session}.swift`
  and current crop/border/inspector/export sources for behavior and edge cases.

Inspect actual CLI `--help` and IO save APIs before promising quality,
metadata, format and bit-depth options. The helper Save request above does
not expose every desired export setting. Add narrowly scoped protocol/IO
support where required, with compatibility and behavioral checks; a UI field
must not silently do nothing. Fast uses f32; Reference invokes the separate
`spektrafilm-f64` process. A retained helper save is not Reference export.

## Connecting the GTK editor

Current `linux/src/main.rs` owns `Rc<Editor>`. `model.rs` stores local history
with display-label keys (`values`/`choices`), crop/rotation and structured
border state. This is **not RuntimeParams JSON**. `fixtures.rs` contained UI
sample values. The UI agent is adding a repository-derived catalog snapshot
and further presentation state; neither snapshot nor samples should override
the live helper's defaults. Explicitly map/migrate UI state into the real
schema, preserve unknown parameter fields, use stable IDs, and derive Reset
and conditional visibility from the actual workflow/profile/defaults.

`actions.rs`/`photo.rs` currently load file/camera JPEGs in background workers;
the RAW scanner extracts an embedded JPEG, not rawler development. Keep that
as an immediate placeholder while real loading/rendering runs. `canvas.rs`
uses a GSK texture with local framing transforms. `border.rs`/`border_ui.rs`
hold structured settings and licensed texture presentation. `perf.rs` and
`validation.rs` provide explicit UI milestones and desktop test scenes.

Implement the following behavior without blocking GTK:

1. Present the native shell first. Start the helper, catalog, decode, GPU
   initialization and shaders asynchronously. Keep input and inspector usable
   during work; distinguish camera/cached placeholders from developed output.
2. Give one worker ownership of the serial helper and its request/response
   stream. Keep at most one render in flight and coalesce pending edits to
   the latest state. Track edit revision, source generation and request ID
   separately from the folder/thumbnail generation. Source replacement or
   modification increments source generation so the helper invalidates decode.
3. During a drag, use the established interactive preview policy, one undo
   transaction per gesture, and a final full-quality request on release.
   Reject obsolete replies using source/revision rules; never mark a cached,
   interactive or outdated frame as current/saveable. Preserve the last visible
   texture and its lifetime until a valid replacement is presented.
4. Use private per-request preview paths with bounded cleanup. Publish pixels,
   dimensions, histogram, backend and saveable render ID together. A helper
   restart invalidates its retained render ID; re-render before enabling Save.
5. Integrate RAW WB, profile/workflow/B&W behavior, Post-only reuse, Image Size,
   crop/straighten and borders using shared semantics. The helper crop in Render
   controls Image Size resolution; it does not crop the preview buffer. Avoid
   applying Post, transfer functions, resizing, crop or borders twice.
6. Show Original through its dedicated command while retaining the developed
   render for Save. Implement the native clients' hold/toggle/release behavior.
   The displayed histogram must follow the displayed developed/cropped image.
7. Fast/Reference/Preview/LUT writes need real destinations, options, progress,
   cancellation and actionable errors. Serialize retained Save against preview
   renders or use a separate export worker/process with a captured immutable
   state. Prevent edits from silently changing an export already requested.
   Use temporary outputs/atomic completion where the format allows it, preserve
   extensions required by the encoder, clean partial files, and never overwrite
   the input. Cancellation must stop/reap owned child processes.
8. Persist versioned session/settings atomically under XDG. Add bounded rendered
   preview caching keyed by source identity/mtime, actual parameters/profiles,
   engine/cache version and sizing. Restore pixels immediately, then rebuild
   the saveable float render; cache hits do not imply a retained helper render.
9. Preserve the UI agent's folder watcher, caching, ordering and accessibility
   work. Complete single-instance file-open forwarding and correct normal quit,
   pending work cancellation, helper EOF/reaping and temporary-file cleanup.

## Assets, packaging and opacity

`linux/install_preview.py` is an idempotent **preview** per-user installer.
It installs UI binary, licensed border data, desktop entry/icon/Open With
declarations, GPL license/source archive and documentation under XDG. It
automatically installs the Negs-only opacity rule on this Hyprland Lua setup.
An isolated XDG installation has been launched in the real Wayland session,
including loading 35mm assets outside the checkout. This is not a production
package, dependency installer, or proof of compatibility on other systems.

Implement the requested reproducible Arch/Omarchy package (PKGBUILD first),
including f32 helper/Fast and f64 Reference executables, **all required profile
and engine data**, border assets, licenses and declared runtime dependencies.
Locate executables/data relative to installation or explicit overrides; do
not embed checkout paths. UI `data_root()` currently supports `NEGS_DATA_DIR`,
installed adjacent data, then development checkout. Keep app ID, desktop entry,
Wayland class and opacity matching consistent. Do not take over MIME defaults.
Do not write a user's home from a root package transaction; provide an explicit
per-user setup path for compositor configuration, update and removal.

## Validation inputs and existing evidence

Private/ignored inputs, hashes and provenance are in `target/linux-validation/`:

- `transfer.json`, `machine.txt` (the latter predates local Rust installation).
- `target/golden/inputs/`: all ten original TIFFs; never regenerate them.
- `inputs/photo_DSC_4990.tif`: 6048×4024, SHA-256
  `d3aa7348d100d9d0545c9b35a46469e7d167357a3f975e67ade25cd60e98516d`.
- `raw/DSC_4990.NEF`: test actual Linux rawler orientation/WB/decode separately.
- `macos-reference-smoke.json` and
  `target/golden/dumps/macos-backend-report/`: 49 historical dumps/19 cases.

Demo JPEGs prepared for UI review are camera/tonemapped placeholders, not
substitutes for these engine correctness or full-resolution benchmark inputs.
Never commit/package validation photos, dumps, renders, caches or toolchains.

The historical Mac dumps use `f27722c` and predate the October 25 Velvia
DIR-gamma correction. Follow the original handoff's subset-comparison command,
but report intentional revision differences separately. Obtain fresh
same-revision Mac outputs before claiming final Mac/Linux parity. Do not alter
portable math, stochastic behavior or gates to force agreement. Accepted GPU
grain statistical gates and the deferred f32 CPU shared-MT19937 grain failure
are distinct statuses; report counts honestly, including known FAIL-GRAIN.

After integration/package changes, rerun into a second dump directory and
require **all 484** same-machine outputs to remain exact:

```sh
python3 -I scripts/golden/run.py run --label linux-omarchy-final \
  --out target/linux-validation/linux-final.json \
  --dump-dir target/golden/dumps/linux-final
python3 -I scripts/golden/run.py compare \
  target/linux-validation/linux-baseline.json target/linux-validation/linux-final.json \
  --exact --report target/linux-validation/linux-exact.json
```

Use `scripts/golden/benchmark.py --help` with the full-resolution TIFF. Separate
pipeline and wall time, warm/empty application cache state, actual backend and
memory. Do not write raw dumps in ordinary export latency measurements.
The script's watchdog-event query is Windows-specific; examine actual Linux
driver/session logs when validating device stability.

Existing UI checks (run these again after integration):

```sh
bash linux/cargo.sh fmt -- --check
bash linux/cargo.sh test --release --locked
bash linux/cargo.sh clippy --locked -- -D warnings
bash linux/cargo.sh build --release --locked
python3 linux/check_ui.py
python3 linux/check_borders.py
python3 linux/measure_ui.py --help
```

The last pre-completion UI revision passed four model/border tests, eight
real AT-SPI interaction checks and four border/persistence/pixel checks.
The UI agent will extend these. Historical startup results in the validation
report do not measure engine load. `developed_image_ms` is currently null;
set it only when a valid developed frame is actually presented. Preserve
separate first submission, compositor presentation, input reception and
developed-image milestones. Repeat normal/restored launches, warm vs empty
application caches, interactive edits under compute load, large-image memory
and idle tests. Do not label an existing driver/OS cache run cold.

In the real desktop verify RAW/TIFF/JPEG/PNG, rapid photo/parameter changes,
undo, crop/angle/rotation, every border, Original, Image Size, actual Fast and
Reference exports, LUT without a photo, cancellation, helper failure/restart,
folder mutations, session/cache restoration, dark/light, narrow tiling,
scale/focus/accessibility and installed launch/Open With. Compare exported
dimensions/framing/colors with the canvas; GUI screenshots alone cannot
establish numerical correctness or color management on other displays.

## Completion and suggested next-agent prompt

Commit coherent changes on the current branch. Update `docs/linux-app.md`,
the parity audit and a core/integration validation report with exact source
revision/toolchain, commands, measured results, package/run paths and real
remaining limitations. Do not claim unfinished integration or known failures
as passing. Preserve the user-visible responsive shell and opaque photograph.

Suggested prompt:

> Work in /home/noah/Work/Negs/.worktrees/linux-omarchy-port on the current
> branch. Read docs/linux-agent-handoff.md and
> docs/linux-core-agent-handoff.md fully, then the linked current UI audit and
> validation docs. Bring up and validate the existing Rust engine on Linux,
> preserve an immutable numerical baseline, and wire the native GTK4/libadwaita
> editor to the persistent spektrafilm-native helper. Complete real RAW,
> preview, Fast/Reference/Preview/LUT exports, caching, session and shutdown
> behavior; package for Arch/Omarchy and test in the actual desktop. Preserve
> the UI, film-strip and opacity fixes. Coordinate with any still-active UI
> agent before editing its files. Follow the existing correctness and startup
> gates, report limitations accurately, and continue through verification.
> Commit locally; do not push or merge.
