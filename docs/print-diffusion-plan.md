# Handoff: expose Print Diffusion on macOS, Windows, and Linux

Prepared 2026-10-28 against Negs `def3716`. This is an implementation plan;
the feature has not been implemented by the agent preparing this document.

## Objective

Add a **Print Diffusion** effect group to the Lab inspector's print controls
on all three native platforms. Expose the existing
`enlarger.diffusion_filter` parameters, with live previews, exports, undo,
reset, session restoration, and preset import/export working consistently.

User request: [pixls.us #1062](https://discuss.pixls.us/t/48209/1062).
This is a UI omission in Negs' port, not a request for a new diffusion model.
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
- Location: Lab, in the print controls immediately after **Enlarger**, before
  **Print Curves**. Linux currently collects these in DEVELOPMENT; place the
  new group immediately after Enlarger there.
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
- Short help where the existing UI permits it: "Diffuses light in the
  enlarger before the print develops. Skipped when scanning film directly."

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

The parameter coverage and detailed-control limits follow upstream. Strength
uses Negs' existing camera UI step, rather than upstream's 0.125 step. Spatial
scale uses the camera panel's practical slider window; upstream permits zero
and has no declared upper bound. These are UI choices, not new engine or
preset validity restrictions. Opening a panel, syncing controls, restoring a
session, or importing a valid preset must not silently rewrite values outside
the visible slider window. Verify this with a non-default spatial-scale value
outside that window. If the existing widget cannot display it faithfully,
extend its displayed bounds to include the loaded value without changing the
stored parameter; retain the normal range for fresh settings.

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
- Extend appropriate existing tests under `macos/Tests/NegsTests/`, especially
  `EngineTests`, `SessionTests`, and `PresetTests`, as needed for the acceptance
  checks below. `testCatalogMatchesEveryInspectorControl` already checks
  control paths against the real helper catalog.

### Windows

- `windows/src/state.rs`: add the matching `ADJUSTMENT_GROUPS` entry in
  `Section::Print`, using the same labels, paths, ranges, and steps.
- `windows/src/ui/inspector.rs`: generic row construction consumes those
  definitions and marks print rows. Confirm the new group uses the existing
  scan-directly disabling, effect enabling, reset, scroll, and history paths.
- Follow existing model, session, and preset handling; verify new controls
  reflect restored/imported values rather than copying camera state.
- Extend existing Windows tests where they exercise the actual behaviors.

### Linux

Linux needs both the group and its translation from presentation keys to
helper parameters. Merely adding rows will not wire the feature correctly.

- `linux/src/fixtures.rs`: add the group and numeric knobs. This file defines
  inspector metadata as well as UI-only fixtures; real defaults still come
  from the helper.
- `linux/src/parameters.rs`:
  - Extend `path()` for `Print Diffusion.<numeric label>` →
    `enlarger.diffusion_filter.<field>`.
  - Add `Print Diffusion enabled` to `TOGGLES`.
  - Add a distinct choice key, e.g. **Print diffusion filter**, to `CHOICES`,
    mapped to `enlarger.diffusion_filter.filter_family`.
  - The existing global `Filter` choice belongs to camera diffusion. Preserve
    it for backward compatibility; do not bind the new dropdown to that key.
  - Check `install_defaults`, `runtime`, reset, session restore, and preset
    application all populate the new choice, toggle, and numeric state.
    In particular, adding control-backed paths must not let UI defaults
    overwrite previously stored/imported enlarger parameters.
- `linux/src/inspector.rs`:
  - Include the group in the header-effect switch list and print sensitivity
    list; add its dropdown in `group_controls` with a separate state key.
  - Give it a false disabled fallback in `effect_default`.
  - If needed, let the combo helper separate its visible label from the
    internal key. Keep the implementation small and preserve old saved keys.
  - Keep lazy group construction and expanded-state persistence working.
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
   still load with the existing helper defaults. No schema migration is
   required merely to expose the fields.

Run focused checks first, then the relevant native suites once:

```sh
# Shared parameter/preset behavior (use the documented platform BLAS env).
cargo test --locked -p spektrafilm-core presets

# macOS: build the helper first using the existing project instructions.
swift test --package-path macos

# Windows, on Windows:
cargo test --locked --manifest-path windows/Cargo.toml
cargo build --release --locked --manifest-path windows/Cargo.toml

# Linux:
bash linux/cargo.sh test --locked --offline
bash linux/build.sh
# From the logged-in desktop, with the environment in linux/desktop-env.sh:
python3 linux/check_controls.py
python3 linux/check_engine.py
```

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
