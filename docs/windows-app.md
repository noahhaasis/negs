# Windows app

The Win32 client lives in `windows/`, in its own Cargo workspace. Build with
`cargo build --release --locked --manifest-path windows/Cargo.toml` and test
with `cargo test --locked --manifest-path windows/Cargo.toml`.
Run `windows/target/release/Negs.exe` from the checkout; the helper and exporters
must already be built in `target/release` (see `windows-agent-handoff.md`).

## Approved decisions (2026-10-23)

The startup spike was approved: use Win32 common controls and build the
inspector after the first frame. Preview colour uses matrix/TRC monitor ICC
profiles with an sRGB fallback. RAW Show Original uses a helper command with
the same decoder/framing and no film processing. Export options belong in
the save dialog. Crop uses C; Show Original uses backslash and O. Updates use
WinSparkle and a per-user installer; winget is deferred.

## Implementation and validation in progress

Both the adjustments panel and film strip start enabled on every launch,
regardless of their saved visibility. F6/F7 can hide them during the session;
the film strip appears once a photograph is open.

The client includes the shell, inspector, Vulkan canvas, crop/border
presentation, session storage, helper client, exports, film strip, ICC parser
and dynamic WinSparkle integration. It is not yet a validated release.
The `original` helper command leaves the retained film render available for
Save and uses the existing image loader and preview sizing policy.

On resuming the implementation, the ICC test fixture did not compile; its
byte buffer now has an explicit type. All 43 Windows unit tests pass. All six
helper unit tests pass, including Original followed by Save of the retained
render. The native integration harness passes 17 requests, including both
export paths. DX12 has been removed to respect the Vulkan-only decision.
Successful exports no longer acquire the Windows hidden attribute.

Further review fixed keyboard accelerators being discarded by a reentrant
App borrow, malformed numeric text being silently rewritten, and unchecked
WinSparkle key rejection. The app now sets the installer’s AppUserModelID.
The WinSparkle return-value contract is documented in its
[official header](https://github.com/vslavik/winsparkle/blob/master/include/winsparkle.h).

Both exact matrices pass: each has 116 f32 CPU, 184 f64 and 184 GPU renders
IDENTICAL to its previous same-machine backend-report manifest. The Mac
engine tools build and all six helper tests pass in the isolated verification
worktree. Reports are `scripts/golden/reports/{windows,macos}-ui-exact.json`.
`python -I windows/check_original.py INPUT.NEF` additionally checks matching
preview dimensions and byte-identical Save before/after Original; it passes
on the test D750 NEF (602 × 402 at 10% resolution).

## Packaging

`windows/package.ps1` builds the three engine tools separately and the app,
then creates `target/windows-dist/Negs-portable.zip` with data, license and a
source archive of HEAD. Package from a committed tree so source and binary
match. Optionally pass `-Iscc` with an Inno Setup 6 compiler to build the
per-user installer. Its Open With registration does not select a default app.
Pass `-WinSparkleDll`, `-AppcastUrl` and `-EdPublicKey` for a configured release;
place WinSparkle's COPYING license beside the supplied DLL.

The GitHub Release workflow builds Windows x64 packages alongside macOS.
Manual runs provide a `negs-windows-experimental` artifact; tagged releases
include those assets after both platform jobs pass. Python 3.11+ can run
`windows/verify_package.py` after packaging to check archive integrity, bundled
executables and committed source, then prepare versioned experimental assets
and SHA-256 checksums in `target/windows-release`. Workspace, Windows app and
installer versions must match.

The portable packaging smoke test passes, including ZIP CRCs and presence of
the app, three helpers, data, license and source archive. The output is
`target/windows-dist/Negs-portable.zip` (approximately 104 MB).
The PowerShell script handles compiler warnings on stderr and normalises the
per-command Git ownership exception. The final portable and Inno installer
payloads passed archive, executable-hash and install/uninstall checks on
2026-10-24. The portable app also loaded a photograph, responded to F7 and
closed cleanly. See the validation report for coverage limits.

Continuation results and the current release checklist are in
[windows-ui-validation.md](windows-ui-validation.md). Software presentation
is implemented, and the Vulkan startup/shutdown defects found interactively
are fixed. 47 Windows tests pass; the Mac workspace build/tests pass. Inno
Setup 6.7.3 compilation and all 35 association install/uninstall checks pass.
Visual, pointer and modal testing remains incomplete due to Computer Use
capture/targeting failures; the app is not yet a validated release.

NEGS_SOFTWARE_CANVAS forces software image presentation for validation;
SPEKTRAFILM_BACKEND=cpu also forces CPU development. Test profiles can be
isolated with NEGS_PROFILE_DIR. NEGS_STARTUP_REPORT records QPC stamps,
including actual input reception and compositor completion. Run
windows/measure_startup.ps1 -Photo PATH for repeated process-start measurements
with real keyboard input supplied separately. NEGS_ALLOW_MULTIPLE is for tests.
