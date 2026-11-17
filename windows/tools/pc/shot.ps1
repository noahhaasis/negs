# Screenshots of the Windows app, run in Noah's console session (see ../pc_shot.sh).
# Usage: shot.ps1 -JobFile <job.json>
# job.json: { "exe": "...\\Negs.exe", "photo": "...", "out": "C:\\...\\shots\\name",
#   "theme": "light|dark|", "width": 1440, "height": 900, "settle_ms": 4000,
#   "env": {"NAME": "value"}, "keep_profile": "dir (optional)",
#   "steps": [ {"shot": "start"}, {"keys": "{F8}", "wait": 800, "shot": "presets"},
#              {"click": [x, y], "wait": 500}, {"key": "VK name", ...} ] }
# "drag": [x0, y0, x1, y1, steps, ms] presses at the first point, moves to the
# second in steps ms apart, and lets go.
# A step with "front": true clicks in, and shoots, the app's frontmost
# window (Settings, an alert) instead of the main one.
# Every PNG is the window's own rectangle only. Windows close at the end.
param([Parameter(Mandatory=$true)][string]$JobFile)
$ErrorActionPreference = 'Stop'
$job = Get-Content -Raw -LiteralPath $JobFile | ConvertFrom-Json
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class NegsWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
[NegsWin]::SetProcessDPIAware() | Out-Null
$out = $job.out
New-Item -ItemType Directory -Force -Path $out | Out-Null
$prof = if ($job.keep_profile) { $job.keep_profile } else { Join-Path $out 'profile' }
if (!$job.keep_profile -and (Test-Path $prof)) { Remove-Item -Recurse -Force $prof }
New-Item -ItemType Directory -Force -Path $prof | Out-Null
$report = Join-Path $out 'startup.log'
if (Test-Path $report) { Remove-Item $report }

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $job.exe
$psi.WorkingDirectory = Split-Path $job.exe
$psi.UseShellExecute = $false
if ($job.photo) { $psi.Arguments = '"' + $job.photo + '"' }
$psi.EnvironmentVariables['NEGS_PROFILE_DIR'] = $prof
$psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE'] = '1'
$psi.EnvironmentVariables['NEGS_STARTUP_REPORT'] = $report
$psi.EnvironmentVariables['SPEKTRAFILM_ROOT'] = 'C:\Users\Noah\Projects\negs-presets'
if ($job.theme) { $psi.EnvironmentVariables['NEGS_THEME'] = $job.theme }
if ($job.env) { foreach ($p in $job.env.PSObject.Properties) { $psi.EnvironmentVariables[$p.Name] = [string]$p.Value } }
$proc = [System.Diagnostics.Process]::Start($psi)
"started pid $($proc.Id)"

$want = if ($job.photo) { 'first_image_on_screen' } else { 'inspector_shown|first_window_composited' }
$deadline = (Get-Date).AddSeconds(60)
do {
  Start-Sleep -Milliseconds 200
  if ($proc.HasExited) { throw "Negs exited early ($($proc.ExitCode))" }
  $lines = if (Test-Path $report) { Get-Content $report } else { @() }
} until (($lines -match "^($want) ") -or (Get-Date) -gt $deadline)
$proc.Refresh()
while ($proc.MainWindowHandle -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $proc.Refresh() }
$h = $proc.MainWindowHandle
$w = if ($job.width) { [int]$job.width } else { 1440 }
$hh = if ($job.height) { [int]$job.height } else { 900 }
[NegsWin]::ShowWindow($h, 9) | Out-Null   # SW_RESTORE
[NegsWin]::SetWindowPos($h, [IntPtr]::Zero, 40, 40, $w, $hh, 0x0040) | Out-Null
# Foreground from a background task: a synthetic Alt tap lets SetForegroundWindow succeed.
[NegsWin]::keybd_event(0x12, 0, 0, [IntPtr]::Zero); [NegsWin]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
[NegsWin]::SetForegroundWindow($h) | Out-Null
$settle = if ($job.settle_ms) { [int]$job.settle_ms } else { 4000 }
Start-Sleep -Milliseconds $settle

# The app's window in front: the main one unless one of its own is.
function Front {
  $f = [NegsWin]::GetForegroundWindow()
  $fpid = [uint32]0
  [NegsWin]::GetWindowThreadProcessId($f, [ref]$fpid) | Out-Null
  if ($fpid -eq $proc.Id) { $f } else { $h }
}

function Capture([string]$name, [bool]$front) {
  $win = if ($front) { Front } else { [NegsWin]::SetForegroundWindow($h) | Out-Null; $h }
  Start-Sleep -Milliseconds 150
  $r = New-Object NegsWin+RECT
  if ([NegsWin]::DwmGetWindowAttribute($win, 9, [ref]$r, 16) -ne 0) { [NegsWin]::GetWindowRect($win, [ref]$r) | Out-Null }
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $file = Join-Path $out ($name + '.png')
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  "shot $file"
}

foreach ($s in $job.steps) {
  $target = if ($s.front) { Front } else { $h }
  if ($s.keys) { [NegsWin]::SetForegroundWindow($target) | Out-Null; [System.Windows.Forms.SendKeys]::SendWait($s.keys) }
  if ($s.click) {
    $p = New-Object NegsWin+POINT; $p.X = [int]$s.click[0]; $p.Y = [int]$s.click[1]
    [NegsWin]::ClientToScreen($target, [ref]$p) | Out-Null
    [NegsWin]::SetCursorPos($p.X, $p.Y) | Out-Null
    [NegsWin]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); [NegsWin]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
    if ($s.double) { [NegsWin]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); [NegsWin]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero) }
  }
  if ($s.drag) {
    $d = $s.drag
    $p = New-Object NegsWin+POINT; $p.X = [int]$d[0]; $p.Y = [int]$d[1]
    [NegsWin]::ClientToScreen($target, [ref]$p) | Out-Null
    $q = New-Object NegsWin+POINT; $q.X = [int]$d[2]; $q.Y = [int]$d[3]
    [NegsWin]::ClientToScreen($target, [ref]$q) | Out-Null
    [NegsWin]::SetCursorPos($p.X, $p.Y) | Out-Null
    [NegsWin]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero)
    $n = [int]$d[4]
    for ($k = 1; $k -le $n; $k++) {
      Start-Sleep -Milliseconds ([int]$d[5])
      [NegsWin]::SetCursorPos([int]($p.X + ($q.X - $p.X) * $k / $n), [int]($p.Y + ($q.Y - $p.Y) * $k / $n)) | Out-Null
    }
    Start-Sleep -Milliseconds 100
    [NegsWin]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
  }
  $wait = if ($s.wait) { [int]$s.wait } else { 600 }
  Start-Sleep -Milliseconds $wait
  if ($s.shot) { Capture $s.shot ([bool]$s.front) }
}

# Close: any modal first (Escape), then the window.
$proc.Refresh()
if (!$proc.HasExited) { $proc.CloseMainWindow() | Out-Null }
if (!$proc.WaitForExit(5000)) { [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); $proc.CloseMainWindow() | Out-Null }
if (!$proc.WaitForExit(5000)) { $proc.Kill(); 'killed' } else { "exit $($proc.ExitCode)" }
