# Current continuation checkpoint (2026-10-23)

Read windows-ui-validation.md and its machine-readable report first. They
supersede the remaining-work list below. The HINSTANCE Vulkan startup bug,
surface/HWND shutdown abort, software image presentation, numeric/Original
accessibility names, and installer Open With registrations are now fixed.
47 Windows tests pass; Mac workspace build/tests pass. Four instrumented
startup/shutdown runs pass (three Vulkan, one forced software/CPU). The installer
compiler is available and registry lifecycle validation passes for 35 extensions.

Still incomplete: the visual/pointer/modal/monitor/accessibility/shell checks
listed in windows-ui-validation.md. Computer Use app permission succeeds, but
capture and modal element targeting fail. Do not call these checks PASS.
First-interaction readiness and reboot-cold startup are still unmeasured.

The following is preserved historical handoff context:

# Windows UI continuation handoff (2026-10-23)

Continue in the Windows checkout on `windows-ui`. Check `git status` first.
The implementation checkpoint before this handoff is `99668ee`, pushed to
origin (the Mac). This is a working app implementation, not a validated release.
The user explicitly approved building Win32 with the inspector created after
the first painted frame, and now requested a fresh agent with Computer Use.

Read `windows-agent-handoff.md` first, especially Rules, then
`windows-ui-features.md`, `cross-platform-port.md`, `native-macos-ui.md`,
`../scripts/golden/README.md`, and `windows-app.md`. The original handoff's
pending decisions are superseded by the approvals in `windows-app.md`:
matrix/TRC monitor ICC with sRGB fallback; helper-decoded RAW Original;
save-dialog export options; C crop and held O/backslash Original; WinSparkle
and a per-user installer. Do not ask again for those decisions.

## State preserved for continuation

- `88b537c`: checkpoint Claude's Win32 implementation and non-destructive
  Original helper command; Vulkan only; export hidden-attribute fix.
- `63179f0`: accelerator dispatch outside the reentrant App borrow, numeric
  input validation, checked updater public-key acceptance, AppUserModelID,
  packaging and Original integration checks.
- `99668ee`: both-machine exact evidence and portable packaging fixes.
- Startup spike source/results remain on `windows-ui-spike`. During cleanup,
  leftover spike build outputs were moved intact from `spikes/` into ignored
  `target/windows-ui-spike-leftovers-20261023/`. No source or evidence was
  deleted. Existing target logs, golden dumps and package staging remain.
- Portable artifact: `target/windows-dist/Negs-portable.zip`, about 104 MB.
  ZIP CRCs and required app/tools/data/license/source entries passed. Rebuild
  from the final committed tree before distributing a release.
- No Negs or helper process was found running at handoff cleanup.

## Verified results (do not confuse these with interactive coverage)

| Check | Result |
|---|---|
| Windows app unit tests | 43/43 PASS |
| Native helper unit tests, Windows and Mac | 6/6 PASS on each |
| Native bridge integration | 17 requests PASS, both export paths |
| Original integration on a D750 NEF | matching 602 x 402 previews; retained Save byte-identical |
| Windows exact matrix | 116 f32 CPU + 184 f64 + 184 GPU IDENTICAL |
| Mac exact matrix | 116 f32 CPU + 184 f64 + 184 GPU IDENTICAL |
| Release app and engine builds | PASS; Mac engine builds PASS |
| Portable archive integrity/content | PASS |

Committed evidence: `scripts/golden/manifests/{windows,macos}-ui.json` and
`scripts/golden/reports/{windows,macos}-ui-exact.json`. Exact engine evidence
was collected at `88b537c`; subsequent implementation changes were Windows
only. Mac verification used an isolated worktree, not the main checkout.

The app is a separate Cargo workspace (`windows/Cargo.toml`); its executable
is `windows/target/release/Negs.exe`. Engine tools live in `target/release`.
See `windows-app.md` for commands and `windows/package.ps1` for packaging.
Build f32 tools and the f64 exporter separately to avoid feature unification.
Git may need the per-command option
`-c safe.directory=C:/Users/Noah/Projects/negs` in this execution identity.

## Remaining work, in practical order

1. Use Computer Use to launch Negs, inspect the actual window, and audit the
   feature contract. Cover open/drop/Explorer single-instance delivery,
   slider typing/reset/drag undo, Lab/Post controls, crop Enter/Escape and
   rotation, borders/textures including 35mm, pan/zoom, DPI and monitor ICC,
   export-dialog settings, Fast/Reference export cancellation and quit,
   session restoration, errors, thumbnails, C/O/backslash including German
   AltGr, focus/tab order and accessibility. Fix demonstrated defects and
   record what actually passed, failed or could not be tested.
2. Implement image presentation without Vulkan. The engine can render on CPU,
   but the current canvas GDI path paints only the background. Keep Vulkan as
   the GPU backend; do not add DX12. Preserve framing, crop, border and colour
   semantics when implementing the fallback, and validate it explicitly.
3. Measure repeated application startup: first painted frame, first real
   interaction, first usable image, with cold/warm definitions and raw logs.
   Include helper launch, catalog and shader work. `NEGS_STARTUP_REPORT`
   records QPC stamps, but its launch stamp is inside the app, not external
   process creation. Establish the external origin and distinguish queued
   paint/initialization from actual presentation/input. Earlier one-off
   diagnostics are not final benchmark results. Spike fresh-file-copy cold
   runs are not reboot-cold measurements.
4. Compile/test the Inno Setup installer (compiler was absent). Check
   per-user installation, uninstall, Open With, existing defaults, and the
   packaged app. WinSparkle feed/signing material must come from actual release
   configuration; do not invent credentials. Test what can be verified locally
   and report any remaining release configuration dependency.
5. Run appropriate regression checks, update evidence/docs, commit and push
   `windows-ui` to origin only. Report measured numbers, changes, every
   non-PASS with its explanation, and any open decision with a recommendation.

Isolate test state with `NEGS_PROFILE_DIR`. `NEGS_ALLOW_MULTIPLE` is for tests.
`windows/check_original.py INPUT.NEF` checks Original framing and retained
Save. Review Original cache invalidation after WB/resolution changes during
interactive testing; that behaviour has not been established by unit counts.

## Computer Use and boundaries

The previous agent's launch was rejected twice with "Computer Use was not
approved to use negs". The user agreed to enable access; no successful tool
launch was observed afterward. A new chat does not itself grant permission.
Read the installed Computer Use skill and use its documented Windows APIs.
Retry through the approved tool after access is enabled; do not circumvent a
denial with another desktop automation route. Continue independent code work
if UI access still needs the user's attention, but do not claim UI PASS.

Official setup: <https://learn.chatgpt.com/docs/computer-use>. Enable the
Computer Use plugin/server/skill, review Settings > Computer use, and approve
Negs when prompted (Always allow if the user wants future access). Windows
must stay unlocked on the active desktop. Shell access is a separate policy.

Never push to GitHub or merge. Never loosen golden gates. Shared engine
changes require the full matrix on both machines and same-machine `--exact`
comparisons; Windows-only additions still require Mac build/test validation.
No changes to macOS output without the user's approval. Read private Mac
access instructions only from `%USERPROFILE%\.negs\mac-access.md`; never
copy their contents, credentials, RAW files or dumps into git or releases.
Device-loss/watchdog engine work and cross-platform CPU grain changes remain
outside this UI task unless the user explicitly expands scope.
