# PC look and speed harness (from Phase 6)

These scripts drive Negs builds in Noah's console session on the PC (`ssh pc`). They take screenshots and measure frames from outside the app, so the GPUI app and the Win32 `windows/` app are measured the same way. The scratch name is `gpui-look`: the scratch dir is `C:\Users\Noah\.negs\scratch\gpui-look` and the scheduled task is `negs-gpui-look`. Change it in every file if you use another name.

## Setup

1. Create the worktree with `ssh pc "C:\Users\Noah\.negs\wt.cmd gpui-look <mac-branch>"`. `pcbuild.sh` updates it and builds `app/`.
2. Copy these files into the scratch dir: `scp app/tools/pc-look/* pc:C:/Users/Noah/.negs/scratch/gpui-look/`.
3. Build `negs-meter`. Copy `negs-meter/` to `scratch\gpui-look\meter`, then run `cargo build --release` there. `run.ps1` expects `meter\target\release\negs-meter.exe`.
4. Make the scratch photos and profiles:
   - `photos-gpui` and `photos-win32` hold scratch copies of the 67 NEFs from `C:\Users\Noah\Downloads\102ND750` and `C:\Users\Noah\Downloads\raws`.
   - `prof-gpui` and `prof-win32` are empty profile dirs.
   - Each app gets its own copies, so neither reads the other's sidecars. Delete any `*.negs` a run leaves behind before you take shots.
5. Copy the GPUI build to `bin-look\Negs.exe`, outside the worktree's target dir, so a rebuild during a run can't swap it. The Win32 reference is main's `C:\Users\Noah\Projects\negs\windows\target\release\Negs.exe`. Never use Noah's installed copy.
6. Create the task once: `schtasks /create /it /ru Noah /sc once /st 00:00 /tn negs-gpui-look /f /tr "C:\Users\Noah\.negs\scratch\gpui-look\job.cmd"`. Delete it when you finish: `schtasks /delete /tn negs-gpui-look /f`.

## Use

- `pcjob.sh <job.ps1> <timeout s>` copies the job to `job.ps1`, runs the task, waits for `job.done`, and prints `job.out`.
- `run.ps1 -Exe -Photo -Profile -Out [-Shot] [-Theme <name>] [-Script scen.txt] [-Settle s]` runs one launch:
  - places the window at 40,40, 1600×1050;
  - with `-Shot`, captures the screen;
  - with `-Script`, plays the `negs-meter` scenario;
  - then closes the app.
- `job-themes-abba.ps1` takes both apps in all six themes, then runs a GUI/Win32/Win32/GUI comparison.
- `job-abba.ps1` runs 8 measured runs, then `startup.ps1` with 8 interleaved launches.
- `analyze.py out/*.json` turns `negs-meter` output into fps, percentiles and latencies per phase. Its header lists the mark syntax.
- `verify-ui.ps1` drives the GPUI app from the keyboard and mouse: menus, tooltips with `-Tips`, end session with `-EndSession`.
- `load.ps1` prints the busiest processes and free memory. Run it before and after each measurement.

Coordinates in `scen-*.txt` and `verify-ui.ps1` are physical pixels for the Phase 6 layout: 3440×1440 at 125%, with the window at 1600×1050. When the layout moves, re-measure the probe rows and click points from a fresh screenshot.
