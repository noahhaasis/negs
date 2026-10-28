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

The visible film strip watches its current folder using
`FindFirstChangeNotificationW`, including file names, attributes, sizes and
last-write times. One background worker waits on OS notifications and a stop
event; there is no idle polling. Bursts debounce for 200 ms (at most one second
during a continuing import), scans run serially, and at most one snapshot waits
for the UI thread. Refreshes preserve selection and scroll position, retain
unchanged thumbnails, and reject stale thumbnail results after folder switches
or file changes. Hiding the strip stops the watch; showing it rescans even the
same folder. `cargo test --locked --manifest-path windows/Cargo.toml folder_watch`
exercises arrivals, later writes, renames, removals, idle waiting and cancellation
on Windows.

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

## Presets

Step 5 of [presets-design.md](presets-design.md), matching the macOS app.
`src/presets.rs` handles the files in `%APPDATA%\Negs\Presets` (under
`NEGS_PROFILE_DIR` when set): the naming policy (NFC + case-folded key via
`NormalizeString`, Windows device names also with an extension), the
`.order.json` order file in the Mac's format (hidden), and the identity check
(size and modification time, read fresh) before rename, delete and read.
New files are written to a dot-prefixed temporary file and moved into place
with `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`. That never replaces,
like the hard link the handoff suggested, and also works off NTFS; a
case-only rename of the same file succeeds. Sharing violations (Explorer's
preview pane) are reported, never retried into another name.

The model (`// MARK: Presets` in `model.rs`) applies a preset as one undo
step, `Apply Preset “Name”`, replacing the params with the helper's
`preset_import` answer and setting film and paper without `choose_film`'s
resets; it asks again if the edit changed while the helper answered. Undo
names are now `Cow<'static, str>`. Unchecked files are sent to the helper
only once the photograph is on screen, so a remembered Presets panel never
delays the first render.

`ui/presets.rs` is the panel: a header with "+" and a "…" menu, then a
report-view ListView (`LVS_EDITLABELS`, custom-drawn two-line rows, a
one-pixel image list for the row height). Refused files are greyed with the
reason as subtitle and info tip. Click or arrow keys apply, double-click and
Enter apply again, a drag doesn't; F2, Delete and the context menu (Apply,
Rename, Export…, Delete…) work as specified. The label edit is moved onto the
name's line. Dragging rows draws its own insertion line (the list view's
insertion mark isn't drawn in report view) and saves the order at once; the
OLE drop target in `ui/drop.rs` imports files dropped on the panel, between
the rows they are dropped between. The panel is created the first time it
shows, after the window's first frame. While cropping, the column shows the
crop panel. The shown pane is the `inspectorPane` preference.

Toolbar Presets button (checked state) beside Adjustments; View ▸ Presets
(F8), File ▸ New Preset (Ctrl+Shift+S) and Import Presets… (Ctrl+Shift+O). The
uninstaller's full cleanup deletes the Presets folder, and its second button
reads "Keep settings, presets and editing session".

Validation (2026-10-28, this machine): 73 Windows tests pass, among them 9
storage tests and 6 that drive the model with the release helper (apply and
undo/redo keep Image Size, rotation, Post, CAT16 and the preview limit; film
A → preset → B → A restores the film's own DIR, halation and grain presets; a
saved session restores the same params; New Preset → Escape leaves no file;
refused files carry the reason; the frozen default GUI state and any files in
`NEGS_PRESETS_TEST_DIR` import, apply and re-export). With the 19 community
and 5 spektrafilm-generated test presets in `%USERPROFILE%\negs-presets-test`
(not in the repository), all 24 and the default state pass that test, and
the panel lists all 24 as valid. Driven in the running app with a scratch
profile: apply by click, New Preset → Escape and → Enter, F2 with an invalid
name, the delete prompt, Import Presets… with one good and one refused file,
a real OLE file drop between rows, dragging a row, Export… (byte-identical
copy), the empty state, and relaunch with the panel remembered.
`check_original.py` passes. The uninstaller test wasn't run: it needs Inno
Setup and a disposable user profile.

Startup, release builds, `docs/card.jpg` passed on the command line, warm
profile, 6 runs each after a warm-up, medians in ms from process start
(`NEGS_STARTUP_REPORT` stamps; each window closed automatically once the
photograph was on screen, all exits clean):

| Build | first window composited | adjustments column shown | photograph on screen |
|---|---|---|---|
| branch before presets (`8b1d9a8`) | 98.0 | 339.0 | 947.8 |
| presets, Adjustments panel | 97.7 | 333.4 | 947.9 |
| presets, Presets panel remembered (8 presets) | 98.4 | 307.9 | 925.5 |

## Packaging

`windows/package.ps1` builds the three engine tools separately and the app,
then creates `target/windows-dist/Negs-portable.zip` with data, license and a
source archive of HEAD. Package from a committed tree so source and binary
match. Optionally pass `-Iscc` with an Inno Setup 6 compiler to build the
per-user installer. Its Open With registration does not select a default app.
Pass `-WinSparkleDll`, `-AppcastUrl` and `-EdPublicKey` for a configured release;
place WinSparkle's COPYING license beside the supplied DLL.

The GitHub Release workflow builds Windows x64 packages alongside macOS.
Main-branch pushes and manual runs provide a `negs-windows-experimental` artifact; tagged releases
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

Restored sessions first check `%LOCALAPPDATA%\Negs\rendered-preview.cache`
(`Cache` under `NEGS_PROFILE_DIR` for isolated profiles). This single atomic file
holds the exact RGBA display frame, dimensions, checksum and full canonical
identity of the rendering settings, source, helper and engine data. The canvas
reapplies the session's crop, rotation and border. A match replaces the raw
startup thumbnail while the helper rebuilds its saveable float output in the
background, with no Updating badge. Rendering edits delete the frame through a
serial I/O queue; only a finished, current, noninteractive render writes it back.
Quit drains that queue. An unfinished edit, missing frame or identity/checksum
mismatch keeps the existing raw-thumbnail and Developing-badge behavior.

The installed app can be removed through **Settings → Apps → Installed apps**.
The uninstaller requires running copies of Negs to close, then offers **Remove
all app data** or **Keep settings and editing session**. Both remove the catalog,
rendered-preview and GPU caches, diagnostic logs, unfinished settings/session
writes and temporary engine files. Full cleanup also removes `settings.json`,
`session.json` and the app's updater preferences under `HKCU\Software\Negs`.
File associations and shortcuts are removed by the normal installer records.
Only known app-owned files/subdirectories are deleted; unrelated files in the
installation or data directories and all original photographs/exports are kept.

Silent uninstall defaults to full cleanup. Add `/KEEPSETTINGS=1` to preserve
settings and the session (`/KEEPSETTINGS=0` explicitly selects full cleanup).
Installing an update does not run this cleanup. Custom `NEGS_PROFILE_DIR` and
engine cache overrides are developer-managed and are not followed by the
uninstaller. Removal failures are logged and reported during interactive use.

`windows/test_uninstall.ps1 -SetupExe target/windows-dist/Negs-Setup.exe` tests
upgrade preservation, running-app protection, both cleanup modes, and preservation
of fixture photographs/exports, including unrelated files in the installation
and data directories. Run it under a disposable Windows user profile; it refuses
existing app-data directories or an existing installation. Installer compilation
and this native uninstall test require Windows and Inno Setup.
