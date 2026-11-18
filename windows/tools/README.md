# Windows parity tooling

Helpers for checking the Windows app against the macOS one
([docs/windows-parity-coordinator.md](../../docs/windows-parity-coordinator.md)).

| Script | Where it runs | What it does |
|---|---|---|
| `pc_install.sh` | Mac | Copies `pc/shot.ps1` and `pc/startup.ps1` to `C:\Users\Noah\.negs` on the PC. Run after changing them. |
| `pc_console.sh '<powershell>' [timeout]` | Mac → PC | Runs a PowerShell command in Noah's console session (a scheduled task; ssh's session 0 has no desktop), waits, prints its output. |
| `pc_shot.sh <job.json> <dir>` | Mac → PC | Launches `Negs.exe` with a scratch profile, sends keys and clicks, captures the window only, closes it, copies the PNGs to `<dir>`. Job format in `pc/shot.ps1`. |
| `pc/startup.ps1` | PC console | Startup medians, one or two builds interleaved ABBA, one warm-up each, windows closed automatically: `pc_console.sh '& C:\Users\Noah\.negs\startup.ps1 -A <exe> -B <exe> -Runs 6'`. |
| `mac_shot.sh <steps> <dir> [photo] [theme]` | Mac | The same views from an isolated copy of `dist/negs-macos-dev/Negs.app` (own bundle id, `HOME`, defaults), keys and clicks through System Events, by PID. Steps format in the script. |

On the PC, `C:\Users\Noah\.negs\build-parity.cmd <branch>` fetches a branch from the
Mac into `C:\Users\Noah\Projects\negs-parity` and builds `windows\` (LTO off, 16
codegen units, incremental). The reference photographs are `C:\Users\Noah\.negs\photos\ref`
on the PC and `~/.negs/windows-parity/photos` on the Mac (the same eight D750 NEFs);
reference screenshots are in `~/.negs/windows-parity/ref/`.

Window coordinates: Windows job clicks are client pixels at 100 % scaling with a
1440×900 window; macOS steps are window points with a 1440-point-wide window.

**Drawing sampler:** `NEGS_DRAW_SAMPLER=<theme>` (darkroom, contactSheet, lightTable,
daylight, plain) makes `Negs.exe` open a window of every Direct2D primitive in that theme
instead of the app: the type ramp, the pencil, rings, stars, crosses, pick boxes and every
token, with what the fonts cost in the title band. Plain follows `NEGS_THEME`. In a
`pc_shot.sh` job, pass it under `"env"` with `"width": 1900, "height": 1150` (the PC runs
at 125 %). The marks' geometry is tested on the Mac:
`cargo test --manifest-path windows/marks/Cargo.toml --target-dir target/marks`.
