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

Still required before completion: interactive feature/accessibility checks,
installer packaging and validation, repeated application startup measurements,
and the Windows/macOS exact golden matrices and Mac build/test checks.
Earlier single-launch timings in the conversation are diagnostic, not final
startup results. Release update feeds and signing keys must come from release
configuration; local builds do not invent them.

Test profiles can be isolated with `NEGS_PROFILE_DIR`. `NEGS_STARTUP_REPORT`
records QPC timestamps. `NEGS_ALLOW_MULTIPLE` is intended only for testing.
