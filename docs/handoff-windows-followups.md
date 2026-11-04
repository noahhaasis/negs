# Handoff: remaining Windows work after 0.13.0

Prepared 2026-11-03. Latest fetched GitHub main: `f0d0ff9` (tone graph merge).
This is a Windows implementation task followed by validation and build checks.

## Checkout and scope

- Repository: `C:\Users\Noah\Projects\negs`. `origin` is now GitHub
  (`git@github.com:noahhaasis/negs.git`); `mac` is the Mac's checkout. GitHub works
  directly from this PC; a Mac connection is unnecessary for ordinary git work.
- Run `git status`, fetch `origin`, and inspect any `AGENTS.md` instructions first.
  Start an isolated worktree from current `origin/main`, on a `codex/` branch.
  The existing `main` branch is checked out in `C:\Users\Noah\Projects\negs-presets`.
  At preparation time both local main and windows-ui were three commits behind GitHub.
- Implement the tone graph on Windows. A Linux port is outside this handoff; Linux
  only needs the build check below. Unmerged Prep branches are also outside scope.
- Preserve user changes, especially on the Mac, which has unrelated modified files.
- Use `NEGS_PROFILE_DIR` for every app test so Noah's settings/session stay intact.
- Commit implementation and validation separately. Do not merge or push without a
  current instruction to do so. Earlier date constraints applied to the November 2
  merge; do not silently backdate new work. Ask for a date if Noah requests one.
- Use accurate attribution for the agent doing the work; don't copy another
  agent's identity from historical commit instructions.

## 1. Port the tone graph to Windows

Read commit `d351348`, merged as `f0d0ff9`, before designing the Windows version.
The graph currently exists only on macOS. Reference files on upstream main:

- `macos/Sources/NegsApp/ToneGraph.swift`: geometry, data, prediction and interaction.
- `macos/Sources/NegsApp/EditorModel.swift`: requests and render histogram handling.
- `macos/Sources/NegsApp/Inspector.swift`: placement in Print Curves.
- `crates/spektrafilm-core/src/tone_curve.rs` and `pipeline.rs`: computation.
- `crates/spektrafilm-native/src/main.rs`: `tone_curve` command and `film_histogram`.

Behaviour to match:

- A graph at the top of Print Curves shows the paper's response to a grey ramp,
  with the photograph's film exposure histogram behind it.
- RGB, R, G and B select the neutral curve or an individual layer.
- Drag the middle handle sideways for Print exposure; the side handles vertically
  for Slow/Fast gamma; the curve elsewhere for Gamma. Each drag is one undo step.
- Double-click resets a handle. Match the Mac's ranges and enable/disable behaviour,
  including direct film scanning and the Print Curves active switch.
- Keep existing sliders synchronized with graph edits, undo/redo and presets.
- Reuse the helper's protocol rather than adding a separate curve implementation.
  Inspect its precise request/reply schema. Rebuild the helper from the same source.
- Follow Windows' existing model/dirty flags, engine request queue, DPI handling,
  focus and keyboard conventions. Keep the main preview responsive: coalesce curve
  requests, reject stale replies and never flood the helper during a drag.
- Use the Mac's local prediction while awaiting a new curve if needed for matching
  responsiveness. Keep prediction separate from authoritative helper results.

Prototype limitations already present upstream: the histogram ignores crop and
spatial effects; toe/shoulder drags can be sensitive; curve agreement with the GPU
render has not yet been established. Record these honestly. Do not claim GPU
agreement or silently widen the feature into an engine redesign.

Acceptance: release app shows the graph in the correct group; all four interaction
types update the expected controls; undo/redo groups drags correctly; resets and
channel selection work; hide/show, photo changes and rapid edits cannot show data
from a previous state. Test meaningful pure geometry/prediction rules and stale
response handling, then exercise the release UI. Record screenshots and limitations.

## 2. Preserve the completed merge validation

Local branch `windows-merge-validation`, commit `46ebfd8`, contains a 23-line result
section in `scripts/golden/README.md`. It has not been merged or pushed. Inspect and
bring that documentation onto your branch without losing later README changes.
It describes tests at `4659754`, not the current main; retain that distinction.

Completed at that commit:

- All release builds passed; workspace: 381 passed, 0 failed, 4 ignored.
- Windows app: 75/75 tests passed. Later Windows UI work had 88 passing tests.
- Strict GPU preflash: three cases below 1e-4 (maximum 2.7e-6).
- Native bridge passed with GPU and CPU; Show Original passed with a D750 NEF.

Run the checks appropriate to the new graph on current code, including the Windows
suite, core/native tests affected by the protocol, and the release build. Historical
counts are evidence, not required totals for a changed test suite.

## 3. Finish the golden matrix and app export check

Read `docs/handoff-merge-windows.md` steps 4–6 and `scripts/golden/README.md`.
The commands and gates there are authoritative for that matrix; verify whether the
matrix has changed upstream before interpreting counts.

The local `target/golden/inputs` still dates from 2026-10-22 and predates the input
colour change. Do not use it or regenerate substitutes and call them the reference.
Obtain matching inputs, Mac dumps and their manifest/checksums from the Mac's
reference run (the old handoff points at `~/Projects/Negs-wasm/target/golden/`,
reference commit `53186e4`). Verify that location and provenance before copying.
If unavailable, clearly report this blocker and complete the independent UI work.

Once correct data is available:

1. Run the Windows Vulkan matrix, then its GPU-vs-f64 quality check. The handoff's
   matrix expected 496 renders and 80/80 quality cases passing.
2. Repeat with the `windows-ui` label and compare exactly; expect all renders equal.
3. Compare against the matching Mac dumps and record the report. Known f32 CPU
   grain divergence is documented; non-grain failures or f64/GPU failures need
   investigation. Never relax a gate to pass.
4. Run the release app with a NEF, adjust Exposure and enlarger M/Y, and test
   preflash at Print Exposure 0.7. Export with GPU and f64 and compare as specified.

Record manifests, exact comparison, cross-platform report and a dated README result
section, including source commit, GPU/driver, commands and failures. Large dump files
stay out of git. Keep existing reference files until their provenance is verified.

## 4. Verify removal of measurement hooks on Mac and Linux

Commit `2b51f0b` removed previews-only measurement switches on all three platforms,
two Linux scripts and `StockPreviewBenchmark.swift`. Windows was built/tested;
the other two platforms were only inspected. Confirm build evidence before rerunning
checks that may already have passed since then.

Mac checks from the relevant checkout:

```sh
swift build --package-path macos
swift build --package-path macos -Xswiftc -DSPEKTRAFILM_BENCHMARK
swift test --package-path macos --filter PreviewTests
```

Run Linux's documented build or its GitHub workflow, on the intended revision.
Inspect the workflow before dispatching, record its run and result, and do not claim
a local Windows check establishes Linux success. Remote machines may have user edits;
use an isolated worktree and don't reset or stash their work.

If Mac access is needed, the previously working connection was user `noahhaasis`
at `192.168.178.88`, using `C:\Users\Noah\.ssh\negs_mac` with IdentitiesOnly.
Only use access still available; don't change SSH or repository receive settings.

## Local test material and delivery

RAW samples in `C:\Users\Noah\Downloads`:
`3365170890.rw2`, `4446046906.raf`, `7219684599.arw`, `9234477241.orf`;
D750 NEFs in `Downloads\102ND750`. All four vendor samples were checked with lens
corrections on/off; the switch and framing work. These are local test material,
not assets to commit. Presets in `C:\Users\Noah\negs-presets-test` include unlicensed
community files: never commit or bundle them.

Deliver a Windows tone graph implementation, updated validation documentation and
reports, and a concise account of checks passed, checks blocked, prototype limitations
and exact branch/commit locations. Update `docs/windows-app.md` and the old merge
handoff's status so the next agent can distinguish historical instructions from work
already completed. No unrelated Prep, lensfun or release work is required.
