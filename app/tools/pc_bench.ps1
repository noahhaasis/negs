# Runs the GPUI app's NEGS_FRAME_BENCH in Noah's console session (start it
# with schtasks /it; see docs/gpui-app-handoff-p5.md), capturing the window
# whenever the app writes <name>.ready.
#   pc_bench.ps1 -Root <scratch dir with photos\> -Exe <worktree>\app\target\release\Negs.exe [-Tour print]
# -Tour print walks the photograph view for pictures instead (NEGS_BENCH_TOUR).
# Writes <Root>\bench-pc.json, <Root>\shots\*.png, <Root>\run.log, and
# <Root>\finished.txt last. The profile is <Root>\profile.
param([Parameter(Mandatory=$true)][string]$Root, [Parameter(Mandatory=$true)][string]$Exe, [string]$Tour = '')
$ErrorActionPreference = 'Continue'
$root = $Root
$shots = Join-Path $root 'shots'
$prof = Join-Path $root 'profile'
$log = Join-Path $root 'run.log'
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $root 'finished.txt'), (Join-Path $root 'bench-pc.json'), $log
if (Test-Path $shots) { Remove-Item -Recurse -Force $shots }
New-Item -ItemType Directory -Force -Path $shots, $prof | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class NegsWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
}
'@
[NegsWin]::SetProcessDPIAware() | Out-Null
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Exe
$psi.WorkingDirectory = Split-Path $psi.FileName
$psi.UseShellExecute = $false
$psi.RedirectStandardError = $true
$psi.Arguments = '"' + (Join-Path $root 'photos') + '"'
$psi.EnvironmentVariables['NEGS_PROFILE_DIR'] = $prof
$psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE'] = '1'
$psi.EnvironmentVariables['SPEKTRAFILM_ROOT'] = 'C:\Users\Noah\Projects\negs'
$psi.EnvironmentVariables['NEGS_FRAME_BENCH'] = (Join-Path $root 'bench-pc.json')
$psi.EnvironmentVariables['NEGS_BENCH_SHOTS'] = $shots
if ($Tour) { $psi.EnvironmentVariables['NEGS_BENCH_TOUR'] = $Tour }
$proc = [System.Diagnostics.Process]::Start($psi)
$errTask = $proc.StandardError.ReadToEndAsync()
"started pid $($proc.Id)" | Out-File -Append $log
$deadline = (Get-Date).AddMinutes(12)
$proc.Refresh()
while ($proc.MainWindowHandle -eq [IntPtr]::Zero -and -not $proc.HasExited -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $proc.Refresh() }
$h = $proc.MainWindowHandle
[NegsWin]::ShowWindow($h, 9) | Out-Null
[NegsWin]::SetWindowPos($h, [IntPtr]::Zero, 40, 40, 1600, 1050, 0x0040) | Out-Null
if ([NegsWin]::GetForegroundWindow() -ne $h) {
  [NegsWin]::keybd_event(0x12, 0, 0, [IntPtr]::Zero); [NegsWin]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
}
[NegsWin]::SetForegroundWindow($h) | Out-Null
while (-not $proc.HasExited -and (Get-Date) -lt $deadline) {
  foreach ($r in Get-ChildItem -Path $shots -Filter '*.ready' -ErrorAction SilentlyContinue) {
    $name = [IO.Path]::GetFileNameWithoutExtension($r.Name)
    $done = Join-Path $shots ($name + '.done')
    if (Test-Path $done) { continue }
    [NegsWin]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 250
    $rc = New-Object NegsWin+RECT
    if ([NegsWin]::DwmGetWindowAttribute($h, 9, [ref]$rc, 16) -ne 0) { [NegsWin]::GetWindowRect($h, [ref]$rc) | Out-Null }
    $bmp = New-Object System.Drawing.Bitmap ($rc.R - $rc.L), ($rc.B - $rc.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($rc.L, $rc.T, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $shots ($name + '.png')), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    New-Item -ItemType File -Path $done | Out-Null
    "shot $name" | Out-File -Append $log
  }
  Start-Sleep -Milliseconds 100
}
if (-not $proc.HasExited) { "timeout, killing" | Out-File -Append $log; $proc.Kill() }
$proc.WaitForExit()
$errTask.Result | Out-File -Append $log
"exit $($proc.ExitCode)" | Out-File -Append $log
New-Item -ItemType File -Force -Path (Join-Path $root 'finished.txt') | Out-Null
