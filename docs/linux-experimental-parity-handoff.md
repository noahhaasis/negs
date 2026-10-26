# Linux handoff: experimental parity and the missing inspector controls

Prepared 2026-10-26 on the Mac. Hand this to an agent working on the Omarchy
machine, in its Negs checkout (`/home/noah/Work/Negs`). The machine has a
Ryzen 5 3600, 16 GiB and a GTX 1660 SUPER, and is the same hardware as the
Windows test box.

## Where things stand

Negs follows Python spektrafilm **experimental `28bf883`** (ticket 16). That
is now verified on two platforms:

- **macOS** (M1 Pro, main `84129ee`/`dd870f0`): `scripts/parity/sweep_experimental.py`
  gives 556/556 cases within one 8-bit step of Python. `grain_compare.py`
  matches within 0.01 RMS. GPU vs f64 passes 76/76.
- **Windows** (GTX 1660 SUPER on Vulkan, `dd870f0`): the same sweep and grain
  results. GPU vs f64 passes 76/76 (ΔE00 max 0.0016). Against the Mac, f64
  and GPU pass everywhere, and f32 CPU grain keeps the known 82 FAIL-GRAIN.
  Branch `windows-parity` has the regenerated manifests and the results in
  `scripts/golden/README.md` and `scripts/parity/README.md`.

**Linux has not been checked against Python.** Its GTK inspector also still
lacks the controls that landed on macOS and Windows with the port. This is
item 1 of "Work before the first public Linux release" in
`docs/linux-release.md`. Start from `windows-parity`, or from main once Noah
has merged it. Make your own branch, `linux-experimental-parity`.

## Ground rules

- Don't push, merge into main or publish anything. Commit on your branch, then
  report back with the measured numbers.
- Date each commit one minute after main's tip (author and committer), unless
  Noah says otherwise.
- Accuracy comes first. Don't loosen a gate, approximate maths or change
  numerics to make a comparison pass. If something fails, find out why and
  report it.
- Pixel comparisons run with grain **and** glare off. Their random streams
  differ per engine; grain only gets statistical checks.
- Treat macOS (`macos/Sources/NegsApp/Inspector.swift`, `EditorModel.swift`,
  `Parameters.swift`) as the source of truth for controls. Windows
  (`windows/src/state.rs`, `windows/src/model.rs`) is a second, Rust
  implementation of the same behaviour and is the easiest one to port from.

## 1. Inspector controls missing on Linux

The Linux groups live in `linux/src/fixtures.rs` (`GROUPS`, `Knob`), the
parameter bindings in `linux/src/parameters.rs` (`path`, `TOGGLES`,
`CHOICES`, `validate`, `install_defaults`), and the widgets in
`linux/src/inspector.rs`. Compared with Windows `state.rs` (the
`AdjustmentGroup` list, around lines 340–445), Linux lacks:

**Film Base** (film section, after DIR Couplers; enable switch
`film_render.base.active`, off by default):

| Slider | Path | Range, step |
|---|---|---|
| Density | `film_render.base.scale` | 0–2, 0.01 |
| Tilt | `film_render.base.tilt` | −1–1, 0.01 |
| Cyan | `film_render.base.cyan` | 0–2, 0.01 |
| Magenta | `film_render.base.magenta` | 0–2, 0.01 |
| Yellow | `film_render.base.yellow` | 0–2, 0.01 |

**Print Base** (print section, after Enlarger; enable switch
`print_render.base.active`): Density `print_render.base.scale`, and Cyan,
Magenta and Yellow `print_render.base.{cyan,magenta,yellow}`, each 0–2 in
steps of 0.01. There's no Tilt here.

**Balance Print**, a button at the end of the Enlarger group (commit
`9077740`). It sends the helper
`{"command":"balance_print","film":…,"paper":…,"params":…}` and gets back
`m_filter_shift` and `y_filter_shift`. Set both as **one** undo step named
"Balance Print", and report "Print balanced: magenta M, yellow Y" in the
status. The result doesn't depend on the photo. Disable the button while a
solve is running, while cropping, and when `io.scan_film` is on. Tooltip, as
on Windows: "Sets the magenta and yellow shifts so a mid-grey prints neutral
with the current film, bases, paper and enlarger. Use it after changing the
film base." Reference: `Model::balance_print` in `windows/src/model.rs`.

**Per-stock grain presets** (commit `6e30221`). Put these at the top of the
Grain group, before Dye clouds:

| Slider | Path | Range, step |
|---|---|---|
| Red/Green/Blue granularity | `film_render.grain.rms_granularity.{0,1,2}` | 0–80, 0.1 |
| Red/Green/Blue uniformity | `film_render.grain.uniformity.{0,1,2}` | 0–1, 0.01 |
| Coarse/Medium/Fine grain size | `film_render.grain.particle_scale_sublayers.{0,1,2}` | 0.05–2, 0.01 |

These arrays work differently from every other control:

- In the edit they stay `null` until the user changes them, so the film's own
  preset applies. The helper's `catalog` reply gives each film's resolved
  preset under `grain`
  (`rms_granularity`, `density_min`, `uniformity`, `particle_scale_sublayers`;
  see `crates/spektrafilm-native/src/main.rs` around line 156). The sliders
  show that preset and reset to it.
- When every lane is back at the preset, the field goes back to `null`.
  Choosing another film clears the fields, so the new film's preset applies,
  as upstream does.
- `density_min` belongs to the same field set (`GRAIN_STOCK_FIELDS`) but has
  no slider.
- B&W films have one emulsion. Show only lane 0 of granularity and uniformity,
  titled "Granularity" and "Uniformity". All three grain sizes stay
  (`color_only`, `bw_title` in `windows/src/state.rs`).
- `parameters::validate` currently requires a numeric default for every
  knob. The preset lanes are `null` in the defaults, so validate them against
  the catalog's `grain` instead. Don't relax the check for the other knobs.

Also check:

- The release docs list "camera colour filters" as missing
  (`docs/linux-release.md`, `docs/linux-post-main-validation.md`). Neither
  macOS nor Windows has such a control. Confirm this against
  `Inspector.swift` and correct both docs, rather than inventing a control.
- New controls need defaults, per-control reset, Reset All, undo/redo and
  session restore, like the existing ones. Extend
  `parameters::tests::live_helper_schema`, the Linux unit tests and
  `linux/check_controls.py` / `linux/check_ui.py` to cover them. Run the
  Wayland desktop checks (`docs/linux-ui-validation.md`) afterwards.
- Compare the remaining ranges and defaults in `fixtures.rs` with
  `Inspector.swift`, and the default paper and film→paper pairing with
  `EditState` (the default paper is `kodak_portra_endura`, and choosing a film
  switches to its `target_print`). Fix only real mismatches.

## 2. Run the experimental sweep and grain comparison on Linux

The Python runtime only needs the core dependencies. Keep it isolated from
the system, for example with `uv` and a managed Python 3.13:

```sh
uv venv --python 3.13 ~/.negs/parity/env
uv pip install --python ~/.negs/parity/env/bin/python \
  colour-science==0.4.7 ImageIO==2.37.3 llvmlite==0.47.0 matplotlib==3.11.0 \
  numba==0.65.1 numpy==2.4.6 OpenImageIO==3.1.14.1 opt_einsum==3.4.0 \
  pillow==12.2.0 pyFFTW==0.15.1 scikit-image==0.26.0 scipy==1.18.0 \
  exiv2==0.18.1 lensfunpy==1.18.0 rawpy==0.27.0
```

These are the Mac's versions. `spektrafilm.utils` imports exiv2, rawpy and
lensfunpy, so they're needed even though the sweep doesn't decode RAW.

You need a **clean** `28bf883` tree; never use a working checkout. Ask Noah
for the tarball (on the Mac: `git -C ~/Projects/spektrafilm archive 28bf883 >
exp-28bf883.tar`) and unpack it into `~/.negs/parity/exp-28bf883`.

Build the engine tools (`source linux/core-env.sh` if the system OpenBLAS is
missing):

```sh
cargo build --release --locked -p spektrafilm-cli --bin spektrafilm --bin golden_compare --bin golden_probes --bin golden_inputs
cargo build --release --locked -p spektrafilm-native
cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
```

Then run (inputs: the ten transferred TIFFs in `target/golden/inputs`, which
must stay unchanged):

```sh
~/.negs/parity/env/bin/python -I scripts/parity/sweep_experimental.py \
  --python-source ~/.negs/parity/exp-28bf883 --output-dir target/parity/sweep --workers 4
~/.negs/parity/env/bin/python -I scripts/parity/grain_compare.py \
  --python-source ~/.negs/parity/exp-28bf883 --output-dir target/parity/grain
```

Use `--workers 4`. On this 16 GiB machine, 10 workers ran out of memory under
Windows: 244 cases failed with `OSError: … requested and 0 written` and left
128-byte `python.npy` files behind. The sweep takes about 17 minutes there.

Expected results, as on macOS and Windows:

- Sweep: 556/556 with no errors. App `max_steps` ≤ 1 everywhere, ΔE00 max
  about 1.23, at most about 0.04% of pixels differing. f64 `max_abs`
  ≤ 1.4e-6 outside the diffusion filter (about 8e-6 with it; Python's float32
  FFT). Largest slider-response gap about 0.0002 ΔE00. Check the backend is
  `wgpu (GPU)`, not the CPU fallback.
- Grain: RMS within 0.01 of Python for all four stocks at 640 px, 2016 px and
  1008 px ×2, and grain size 0.86–0.87 px for every engine.

The sweep drives `spektrafilm-native` directly, so it tests the engine, not
the GTK bindings. Once part 1 is done, also check that the Linux inspector
produces the same `params` as macOS for a few edits. For example, compare
the params the app sends to the helper with what `sweep_experimental.py`
builds for the same slider.

## 3. Golden manifests for Linux

No Linux manifest is committed yet. The post-merge validation's first
matrix ran before the Vulkan reflect-index fix (`92a559e`) and failed
quality, so don't count it. A later run passed 76/76
(`docs/linux-post-main-validation.md`), but nothing was compared across
platforms. On your branch:

```sh
python3 -I scripts/golden/run.py run --label linux-gtx1660s-vulkan \
  --out scripts/golden/manifests/linux-gtx1660s-vulkan.json --dump-dir target/golden/dumps/linux
python3 -I scripts/golden/run.py quality scripts/golden/manifests/linux-gtx1660s-vulkan.json \
  --dumps target/golden/dumps/linux
```

Expect GPU vs f64 to pass 76/76 at about the Windows numbers (ΔE00 max
0.0016, ≤ 1 8-bit step). Do a second, independent run, after the GTK app
build and its tests, and check it with `compare --exact` (expect 484/484
IDENTICAL).

For the cross-platform comparison against `macos-arm64.json` (refreshed at
`dd870f0` on `windows-parity`), you need the Mac's raw dumps from that run.
They stay on the Mac (5.9 GB, at
`.claude/worktrees/windows-parity/target/golden/dumps/macos-arm64`). Ask Noah
to transfer them directly, never through git or a release, then run:

```sh
python3 -I scripts/golden/run.py compare scripts/golden/manifests/macos-arm64.json \
  scripts/golden/manifests/linux-gtx1660s-vulkan.json \
  --dumps-a target/golden/dumps/macos-arm64 --dumps-b target/golden/dumps/linux \
  --report scripts/golden/reports/linux-vs-macos.json
```

Windows gave f64 180 PASS / 2 PASS-BORDER / 2 PASS-GRAIN, GPU 184 PASS, and
f32 CPU 26 PASS / 8 PASS-GRAIN / 82 FAIL-GRAIN. The f32 CPU grain failures
are a known, deferred divergence of the shared MT19937 stream; don't try to
fix them. Anything else that fails is new: run `probe` / `compare-probe` to
find the first stage that differs.

Saves embed an ICC profile since `06410bd`, so TIFF hashes differ from older
manifests even when the pixels match. Compare raw (`f64`) hashes when you
judge what changed.

Commit the manifests and the report, not the dumps. Record the results in
`scripts/golden/README.md` and `scripts/parity/README.md` next to the
Windows ones, and update `docs/linux-release.md`.

## Report back

- What you changed in the Linux inspector, with test and desktop-check
  results.
- Sweep, grain, quality, exact and cross-platform numbers, including any
  failures with their cause.
- Anything you left undone, and why.
