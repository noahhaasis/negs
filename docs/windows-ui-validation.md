# Windows continuation validation — 2026-10-23

Implemented and validated: Vulkan surface creation now supplies HINSTANCE;
surface destruction precedes HWND destruction (fixes repeated 0xC0000409
shutdowns); GDI software presentation uses the canvas draw list for crop,
rotation, borders, alpha textures, text and monitor matrix/TRC conversion.
Linear-light minification is cached. NEGS_SOFTWARE_CANVAS forces this path
for testing; no DX12 backend was added. Numeric editors and Original now have
accessible names, and Original participates in tab navigation. GUI panics
write a local panic.log in the profile directory.

All 47 Windows unit tests and the release build pass. The Mac workspace build
and tests pass (one optional blur benchmark ignored); the shared engine tree
is identical. Existing both-machine exact golden evidence remains applicable
because no shared engine source or gates changed.

The per-user Inno installer compiles. Registry lifecycle validation found and
fixed 35 missing OpenWithProgids values: Inno ValueType none creates no value.
Install/uninstall now verifies all 35 empty REG_SZ values, the open command,
required payload files, cleanup, and unchanged existing default associations.
This first lifecycle check used the previous portable payload; final packaging
must include the new executable and committed source.

Four corrected fresh-process startup runs all exit 0. From an external QPC
origin: fresh app caches on Vulkan: first composition 431 ms, image 1114 ms;
two warm app-cache runs: composition 111/113 ms, image 812/696 ms; forced
software/CPU: composition 104 ms, image 1304 ms. OS/driver caches were warm;
these are not reboot-cold results. First F7 events were real but delayed by
Computer Use orchestration (17–41 s), so first-interaction readiness latency
remains unmeasured. Presentation stamps bound compositor completion; they do
not substitute for visual inspection. windows/measure_startup.ps1 reproduces
the collection with an explicit photo path. Private raw logs remain in target.

Keyboard/accessibility checks establish C crop, crop rotation, Escape discarding
rotation, Enter exiting crop, F6/F7 panel toggles, numeric commit and malformed
text recovery, and document Undo/Redo. A user selected the test photo in the
native Open dialog after Ctrl+O opened it. Final accessibility names were read
back from the running app.

Remaining release blockers: visual/pointer checks (crop appearance, borders,
textures, zoom/pan, thumbnails, drop and drag undo); export-dialog interactions,
cancellation and quit-during-export; German AltGr/held Original and WB/resolution
cache invalidation; real monitor ICC/DPI changes; complete screen-reader/custom
control/tab/high-contrast audit; Explorer Open With/single-instance UI; reboot-cold
startup and a low-overhead input readiness measurement. Computer Use launches
and reads accessibility, but captures fail with window capture timed out and
coordinate input geometry is unavailable. Modal filename controls are listed
but rejected as unavailable by set_value. No alternate desktop automation was
used to bypass these failures. Fix the capture/modal-targeting runtime and
finish this checklist before marking the app release-ready.

Updates remain unconfigured until actual WinSparkle release material is supplied.
No feed credentials or signing keys were invented. Full machine-readable evidence:
scripts/golden/reports/windows-ui-validation-2026-10-23.json.
