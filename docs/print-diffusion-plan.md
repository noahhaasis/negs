# Handoff: expose Print Diffusion on macOS, Windows, and Linux

Prepared 2026-10-28 against Negs `def3716`; reviewed adversarially the same
day against `167602b` (see [Review notes](#review-notes)).

**Status:** ready for implementation. Nothing below has been implemented yet.

## Objective

Add a **Print Diffusion** effect group to the Lab inspector's print controls
on all three native platforms. Expose the existing
`enlarger.diffusion_filter` parameters, with live previews, exports, undo,
reset, session restoration, and preset import/export working consistently.

User request: [pixls.us #1062](https://discuss.pixls.us/t/48209/1062).
This is a UI omission in Negs' port, not a request for a new diffusion model.
The omission is also a hidden state today: applying an upstream preset whose
`enlarger_diffusion` is active turns print diffusion on in every app, with no
control that shows it or turns it off (only Reset Adjustments clears it).
Photographs and sessions with such values may already exist and must keep
them.
The original Spektrafilm exposes it under **PRINT → Diffusion** on all three
checked branch tips:

| Branch | Revision | UI placement |
| --- | --- | --- |
| main | `3bb2c2d` | [`napari_layout.py:425`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d/src/spektrafilm_gui/napari_layout.py#L425) |
| experimental | `28bf883` | [`napari_layout.py:450`](https://github.com/andreavolpato/spektrafilm/blob/28bf883/src/spektrafilm_gui/napari_layout.py#L450) |
| dev | `6cd00c8` | [`napari_layout.py:450`](https://github.com/andreavolpato/spektrafilm/blob/6cd00c8/src/spektrafilm_gui/napari_layout.py#L450) |

Use experimental `28bf883` as the reference. Its complete UI manifest is
[`params_manifest.py:752–804`](https://github.com/andreavolpato/spektrafilm/blob/28bf883/src/spektrafilm_gui/params_manifest.py#L752-L804).

## Workflow and boundaries

- Read the current `AGENTS.md`. Negs changes go directly to `main`, committed
  and pushed; no PR unless Noah asks. Recheck the working tree and remote
  before starting and before pushing, since another agent may have advanced it.
- At preparation time `linux/run.sh` was modified and
  `docs/highlights-shadows-plan.md` was untracked. Preserve both and any newer
  unrelated work. Stage explicit files; do not include unrelated changes.
- Implement the three maintained native apps, not the old egui app.
- Keep this scoped to exposing the existing print effect. Do not change
  camera diffusion, engine defaults, physical algorithms, output encoding,
  border processing, or the highlights/shadows feature.
- Build and test on the native OS where available. Report unavailable native
  validation explicitly; a cross-compilation check is not a native UI check.
  Do not publish releases or install over the user's app as part of this task.

## Existing implementation to reuse

1. `crates/spektrafilm-core/src/params.rs`: `DiffusionFilterParams` already
   contains every field below; `EnlargerParams.diffusion_filter` uses it.
2. `crates/spektrafilm-core/src/stages/printing.rs`: diffusion acts on linear
   print exposure before print development. CPU and GPU-backed paths already
   exist. It is distinct from camera/lens diffusion before film development.
3. Follow the enlarger diffusion through the resident preview/export paths
   in `crates/spektrafilm-core/` and `crates/spektrafilm-gpu/` during validation.
   Existing support is not a reason to skip checking the live UI route.
4. `crates/spektrafilm-core/src/presets.rs`: `SECTIONS` already maps upstream
   `enlarger_diffusion` to `enlarger.diffusion_filter`. Keep this preset format.
5. The native apps already have generic effect groups, parameter bindings,
   enable switches, per-control resets, history, and session persistence.
   Reuse them. No new helper command, schema version, or dependency should
   be necessary just to expose existing parameters.

## UI specification

Use the same name, order, field set, defaults, and ranges on all platforms.

- Group title: **Print Diffusion**.
- Location: Lab, as the first print effect group. The neighbouring groups
  already differ per platform, so place it by platform:
  - macOS: immediately after **Enlarger**, before **Print Curves**.
  - Windows: immediately after **Enlarger**, before **Print Base** (Windows
    orders Print Base before Print Curves; do not reorder existing groups).
  - Linux: DEVELOPMENT hoists Enlarger to the top (`inspector.rs`, the
    `groups` chain) and then lists every other group in `fixtures::GROUPS`
    order, so Enlarger is followed by Film Curves. Put the new group in
    `GROUPS` between **Lens Diffusion** and **Print Curves**, so it leads the
    print groups. Don't hoist it next to Enlarger: that would put a print
    effect above the film groups.
- Header enable switch: `enlarger.diffusion_filter.active`, initially false.
- Start collapsed, following the existing persistence of expanded groups.
- When off, disable the group's controls but leave its header switch usable.
- With **Scan film directly** on (`io.scan_film`), disable the entire group,
  just as other print groups are disabled. Retain its settings; restoring the
  print route reactivates them without losing edits.
- Use the existing per-control reset, modified indicators, keyboard editing,
  and one-undo-step-per-drag behavior. Reset to helper defaults, not a
  separately maintained set of processing defaults.
- No new nested advanced-panel framework is needed. Place the detailed
  controls below Filter, Strength, Spatial scale, and Halo warmth.
- No group-level help exists on any platform; don't add a mechanism for it.
  The existing print footer ("Printing is skipped while scanning film
  directly.") already covers direct scan.

All paths below are relative to `enlarger.diffusion_filter`:

| Label | Field | Default | UI range / options | Step |
| --- | --- | --- | --- | --- |
| Enable (header) | `active` | false | boolean | — |
| Filter | `filter_family` | `black_pro_mist` | Black Pro-Mist, Glimmerglass, Pro-Mist, CineBloom | — |
| Strength | `strength` | 0.5 | 0–2 | 0.01 |
| Spatial scale | `spatial_scale` | 1 | 0.1–3 | 0.01 |
| Halo warmth | `halo_warmth` | 0 | −1.5–1.5 | 0.01 |
| Core intensity | `core_intensity` | 1 | 0–4 | 0.05 |
| Core size | `core_size` | 1 | 0.1–4 | 0.05 |
| Halo intensity | `halo_intensity` | 1 | 0–4 | 0.05 |
| Halo size | `halo_size` | 1 | 0.1–4 | 0.05 |
| Bloom intensity | `bloom_intensity` | 1 | 0–4 | 0.05 |
| Bloom size | `bloom_size` | 1 | 0.1–4 | 0.05 |

Filter values, in that order:
`black_pro_mist`, `glimmerglass`, `pro_mist`, `cinebloom`.

The parameter coverage, detailed-control limits, and detailed-control steps
follow upstream (`params_manifest.py` in the frozen checkout at
`~/.negs/parity/exp-28bf883`). Strength, Spatial scale, and Halo warmth use
Negs' existing camera-panel step of 0.01 instead of upstream's 0.125, 0.1,
and 0.05. Spatial scale uses the camera panel's practical slider window;
upstream permits zero and has no declared upper bound. These are UI choices,
not new engine or preset validity restrictions.

Opening a panel, syncing controls, restoring a session, or importing a valid
preset must not silently rewrite values outside the visible slider window.
Test with `spatial_scale` 4.0, which is valid upstream and above the
window. Zero is valid too, but the engine clamps it to an effectively empty
kernel, so it makes a poor visual test. Current behaviour by platform:

- Linux already widens a slider's bounds to a loaded value in `sync_state`
  (`fixtures::range`), as long as the knob is in `fixtures::GROUPS`. Check
  that it also holds for a lazily built group that is opened after the
  value is loaded.
- macOS: the `number` binding's getter returns the stored value and only
  the setter snaps and clamps. Confirm the row shows 4 and that opening the
  group or applying a preset writes nothing back.
- Windows: confirm the same for the trackbar and its value text.

Where a widget can't show the value faithfully, widen its displayed bounds
to include it without changing the stored parameter. Keep the normal range
for fresh settings.

Do not simply copy the existing Lens Diffusion group and stop there: that
group currently omits `core_size` and has narrower detailed-control ranges.
Expose all ten controls plus the header switch (eleven fields total) for print.

## Platform changes

### macOS

- `macos/Sources/NegsApp/Inspector.swift`: add an `AdjustmentGroup` in
  `.print`, enabled by `enlarger.diffusion_filter.active`, with the controls
  above. Inspect the section renderer to confirm direct-scan gating and the
  new group's identity/expanded state work through the generic path.
- Reuse `EditorModel` and `Parameters` bindings; only change them if a
  concrete sync, history, reset, or persistence issue is found.
- Group identity and the expanded state come from the title
  (`expandedAdjustments` is a comma-joined list of titles), so the title has
  to stay unique. Groups are `.disabled(scanning)` as a whole section, header
  switch included.
- Extend appropriate existing tests under `macos/Tests/NegsTests/`, especially
  `EngineTests`, `SessionTests`, and `PresetTests`, as needed for the acceptance
  checks below. `testCatalogMatchesEveryInspectorControl` already checks
  control paths against the real helper catalog, but it `XCTSkip`s unless
  `target/release/spektrafilm-native` exists. A skipped run is not a pass.

### Windows

- `windows/src/state.rs`: add the matching `ADJUSTMENT_GROUPS` entry in
  `Section::Print`, directly after Enlarger, using the same labels, paths,
  ranges, and steps. Rows, resets, and choices are keyed by path, so reusing
  the labels "Filter" and "Strength" is safe.
- `windows/src/ui/inspector.rs`: generic row construction consumes those
  definitions and marks print rows. Confirm the new group uses the existing
  scan-directly disabling, effect enabling, reset, scroll, and history paths.
- Follow existing model, session, and preset handling; verify new controls
  reflect restored/imported values rather than copying camera state.
- Extend existing Windows tests where they exercise the actual behaviors.

### Linux

Linux needs both the group and its translation from presentation keys to
helper parameters. Merely adding rows will not wire the feature correctly.

- `linux/src/fixtures.rs`: add the group and its ten numeric knobs, between
  Lens Diffusion and Print Curves (see Location). This file defines inspector
  metadata as well as UI-only fixtures; real defaults still come from the
  helper. `presets::value_keys`, `parameters::validate`, and
  `install_defaults` all iterate `GROUPS`, so they pick the knobs up.
- `linux/src/parameters.rs`:
  - Extend `path()` for `Print Diffusion.<numeric label>` →
    `enlarger.diffusion_filter.<field>`. The Lens Diffusion branch's
    lowercase-and-underscore rule fits these labels too.
  - Add `Print Diffusion enabled` to `TOGGLES`. The inspector builds the
    `{group} enabled` key itself, so the spelling must match.
  - Add a distinct choice key, e.g. **Print diffusion filter**, to `CHOICES`,
    mapped to `enlarger.diffusion_filter.filter_family`.
  - The existing global `Filter` choice belongs to camera diffusion. Preserve
    it for backward compatibility; do not bind the new dropdown to that key.
- **Session-restore regression to fix in `install_defaults`.** Linux keeps
  the full params in `State.params`, but `runtime` overwrites every
  control-backed path with `State.values` / `State.choices`. Today
  `enlarger.diffusion_filter` has no controls, so an applied preset's print
  diffusion survives in `State.params` and the saved session. After this
  change, restoring such a session runs `install_defaults(…, reset: false)`
  (`runtime.rs`), finds no `Print Diffusion.*` values or
  `Print Diffusion enabled` / `Print diffusion filter` choices, and fills
  them from the helper defaults. The next `runtime` call then switches the
  effect off and resets its values without telling anyone. Fix: when a key
  is missing and `reset` is false, seed it from the restored `State.params`
  at its path, and fall back to the helper default only when that is null
  or missing. Apply the same rule to numeric keys, `TOGGLES`, and `CHOICES`.
  This runs before `merge(defaults, params)` in `runtime.rs`, so read the
  unmerged params. Keep `reset: true` (Reset Adjustments) using helper
  defaults. Add a `parameters` unit test: a state with no Print Diffusion
  keys and `params.enlarger.diffusion_filter` = {active: true, cinebloom,
  strength 1.25, spatial_scale 4}. After `install_defaults` and `runtime`,
  every field must be unchanged.
- `linux/src/inspector.rs`:
  - Add "Print Diffusion" to the header-effect switch list
    (`matches!(group.name, …)`) and the print sensitivity list
    (`"Enlarger" | "Print Curves" | …`); add its dropdown in `group_controls`.
  - Give it a false disabled fallback in `effect_default`. This matters in
    `NEGS_UI_ONLY` mode, where there are no helper defaults.
  - `combo` uses its title as the state key, the `choice_default` lookup,
    the reset key, and the `ui.choices` sync key. A "Filter" row in Print
    Diffusion would therefore drive the camera filter. Add a separate key
    parameter, e.g. a `combo_keyed(ui, list, title, key, options)` that
    `combo` delegates to. Keep the visible label **Filter** and leave the
    existing key and callers unchanged.
  - Keep lazy group construction and expanded-state persistence working.
  - Accessible names repeat across the camera and print groups ("Filter",
    "Strength value", "Reset Strength"). AT-SPI checks must find these
    controls within the Print Diffusion expander, not by first match.
- Extend existing `parameters` tests and the relevant validation hooks in
  `linux/src/validation.rs` / `linux/src/engine_validation.rs`. The important
  regression is independent camera and print state through runtime rebuilding
  and preset/session restoration, not just a copied list of expected fields.

## Implementation order

1. Recheck current source and upstream reference. Confirm defaults and preset
   mapping; identify the actual preview and export routes.
2. Add the macOS and Windows definitions and Linux metadata/bindings. Ensure
   all three use the UI specification above.
3. Verify state round-trips, independent camera/print controls, direct-scan
   gating, defaults/reset, and imported values outside normal slider bounds.
4. Validate real rendering and exports with the new controls. Fix engine
   wiring only if a demonstrated defect prevents this existing feature from
   working; document any such change separately in the implementation report.
5. Run the relevant native checks, review the diff, commit the implementation
   and push to `main` under the current repository workflow. Do not claim a
   platform was validated when its native tests were unavailable.

## Acceptance and verification

Use a small synthetic scene with a bright patch or point on a dark background,
plus a gradient. Flat mid-gray alone can hide an energy-preserving diffusion
effect. Keep grain off for repeatable comparisons and choose visibly effective
diffusion settings. Reuse existing fixtures/harnesses instead of creating a
parallel validation framework.

The implementation is complete when these behaviors hold:

1. **Coverage:** each app exposes the full field set, with correct labels,
   runtime paths, defaults, and enable behavior. Every filter can be selected.
2. **Isolation:** edit camera and print filters/strengths to different values;
   changing or resetting either leaves the other intact. On Linux assert the
   rebuilt `parameters::runtime` object retains that independence.
3. **State:** undo/redo and per-control resets work; a drag is one history
   step. Session restart restores settings. Reset Adjustments returns the
   print effect to helper defaults, including disabled state.
4. **Presets:** import an upstream-format preset with non-default
   `enlarger_diffusion` values, show them correctly in every UI, save/export,
   re-import, and retain every field including `core_size`. Include a value
   outside the normal spatial-scale slider bounds and check that sync alone
   does not mutate it.
5. **Rendering:** with printing enabled, print diffusion changes the synthetic
   scene in both live preview and export. Disabling it restores the baseline.
   Confirm Fast/GPU and Reference/CPU export both receive the edited values;
   judge numerical agreement with existing diffusion/parity tolerances, not
   bit identity between CPU and GPU.
6. **Direct scan:** controls disable when scanning film directly; changing
   the stored print-diffusion settings has no effect on a direct scan.
   Switching back preserves them and resumes their effect.
7. **UI behavior:** no inspector movement during reduced-resolution drags or
   release; ordinary keyboard editing and accessible labels remain usable.
8. **Compatibility:** existing sessions/presets without explicit print values
   still load with the existing helper defaults. A session saved by the
   current release after applying a preset with active, non-default
   `enlarger_diffusion` restores with the same values on every platform, and
   the new controls show them. This matters most on Linux; see the
   `install_defaults` item. No schema migration is required merely to
   expose the fields.

Run focused checks first, then the relevant native suites once:

```sh
# Shared parameter/preset behavior. On Linux, `source linux/core-env.sh` first
# (OpenBLAS); macOS uses Accelerate; Windows needs no BLAS.
cargo test --locked -p spektrafilm-core presets

# macOS (docs/native-macos-ui.md). Without the release helper,
# testCatalogMatchesEveryInspectorControl skips.
cargo build --release -p spektrafilm-native
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
swift test --package-path macos --scratch-path target/swift-native
python3 scripts/check_native_macos.py

# Windows, on Windows:
cargo test --locked --manifest-path windows/Cargo.toml
cargo build --release --locked --manifest-path windows/Cargo.toml

# Linux:
bash linux/cargo.sh test --locked --offline
bash linux/build.sh
# From the logged-in Wayland desktop, with the environment in linux/desktop-env.sh:
python3 linux/check_controls.py   # NEGS_UI_ONLY: header, dropdown, disabled states
python3 linux/check_engine.py     # real helper: preview, Fast and Reference export
python3 linux/check_presets.py    # real helper: preset apply/export round trip
```

`check_controls.py` runs without the engine, so it can't show that the
controls reach the renderer. Each script exercises only the controls it
already knows about, so extend it to cover Print Diffusion. Running it
unchanged proves nothing about the new group.

Extend/run the existing rendering/parity coverage for enlarger diffusion if
the UI-to-render checks reveal a gap. `scripts/parity/sweep_experimental.py`
currently has camera-diffusion sweeps; do not mistake those for print coverage.
Avoid rerunning an unrelated full upstream parity matrix unless an engine
change or failure justifies it. Check existing fixture
`scripts/gpu_cpu_quality/params/fx/diff_enlarger.json` for reuse.

Native UI checks must interact with the actual group on each platform; model
tests alone do not prove that the header, dropdown, and disabled states work.
Record what was run, results, and any unavailable OS/backend in the handoff
back to Noah. Update this document's status when implementation is complete.

## Review notes

Adversarial review, 2026-10-28, against `167602b`.

Checked and still accurate:

- `DiffusionFilterParams` fields and defaults match the table and the frozen
  upstream default state (`data/presets/spektrafilm-gui-default-28bf883.json`).
- The upstream manifest (`params_manifest.py:752–804` at `28bf883`) matches
  the table's ranges. `napari_layout.py:450` puts it on the PRINT tab.
- The engine is wired: `printing.rs` diffuses linear print exposure on CPU and
  GPU, and `pipeline.rs` builds the resident GPU plan only when
  `!io.scan_film && active`.
- Presets: `SECTIONS` maps `enlarger_diffusion` both ways, and `check`
  rejects unknown families for both stages.
- The camera group omits `core_size` and caps intensities at 2 and sizes at
  3 on all three platforms.
- macOS and Windows effect groups, direct-scan disabling, and resets are
  generic, as stated.

Corrected in this revision:

- **Placement** couldn't be met: Windows orders Print Base before Print
  Curves, and Linux hoists Enlarger above the film groups. Placement is now
  per platform.
- **Linux session restore** would silently switch off print diffusion that
  an applied preset had saved in `State.params`. The fix and its test are
  now required.
- **Linux combo key**: `combo` keys state, defaults, reset, and sync by its
  title. A second "Filter" would drive the camera filter. Splitting label
  from key is now required, not optional.
- Compatibility now covers sessions *with* print values, not only without.
- Out-of-range values: recorded each platform's current behaviour and named
  a concrete test value (4.0).
- Commands: the macOS helper build and scratch path, Linux `core-env.sh`,
  `check_presets.py`, and `check_native_macos.py`. Noted that
  `check_controls.py` is UI-only and that a skipped Swift catalog test is
  not a pass.
- Dropped the group help text: no platform has group-level help.
