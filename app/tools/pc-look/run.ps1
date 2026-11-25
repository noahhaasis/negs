# One measured run of a Negs build in Noah's console session (schtasks /it).
#   run.ps1 -Exe <Negs.exe> -Photo <file> -Profile <dir> -Out <prefix> [-Script <meter.txt>] [-Shot] [-Theme <name>] [-Settle 4] [-Keep]
# Writes <prefix>.startup.log, <prefix>.json (meter), <prefix>.png (window
# after settling), <prefix>.mem.txt, and <prefix>.done last.
param([Parameter(Mandatory=$true)][string]$Exe, [Parameter(Mandatory=$true)][string]$Photo,
      [Parameter(Mandatory=$true)][string]$Profile, [Parameter(Mandatory=$true)][string]$Out,
      [string]$Script = '', [string]$Env = '', [switch]$Shot, [string]$Theme = '', [int]$Settle = 4, [string]$Meter = 'C:\Users\Noah\.negs\scratch\gpui-look\meter\target\release\negs-meter.exe')
$ErrorActionPreference = 'Continue'
$log = "$Out.log"
Remove-Item -Force -ErrorAction SilentlyContinue "$Out.done", "$Out.json", "$Out.png", "$Out.startup.log", "$Out.mem.txt", $log
New-Item -ItemType Directory -Force -Path $Profile | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class NW {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
}
'@
[NW]::SetProcessDPIAware() | Out-Null
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Exe
$psi.WorkingDirectory = Split-Path $Exe
$psi.UseShellExecute = $false
$psi.Arguments = '"' + $Photo + '"'
$psi.EnvironmentVariables['NEGS_PROFILE_DIR'] = $Profile
$psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE'] = '1'
$psi.EnvironmentVariables['SPEKTRAFILM_ROOT'] = 'C:\Users\Noah\Projects\negs'
$psi.EnvironmentVariables['NEGS_STARTUP_REPORT'] = "$Out.startup.log"
foreach ($kv in ($Env -split ';')) { if ($kv) { $k, $v = $kv -split '=', 2; $psi.EnvironmentVariables[$k] = $v } }
if ($Theme) { $psi.EnvironmentVariables['NEGS_THEME'] = $Theme }
$origin = [Diagnostics.Stopwatch]::GetTimestamp()
$p = [System.Diagnostics.Process]::Start($psi)
"origin $origin pid $($p.Id)" | Out-File -Append $log
$deadline = (Get-Date).AddSeconds(90)
$p.Refresh()
while ($p.MainWindowHandle -eq [IntPtr]::Zero -and -not $p.HasExited -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50; $p.Refresh() }
$h = $p.MainWindowHandle
[NW]::ShowWindow($h, 9) | Out-Null
[NW]::SetWindowPos($h, [IntPtr]::Zero, 40, 40, 1600, 1050, 0x0040) | Out-Null
[NW]::keybd_event(0x12, 0, 0, [IntPtr]::Zero); [NW]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
[NW]::SetForegroundWindow($h) | Out-Null
do { Start-Sleep -Milliseconds 100; $lines = if (Test-Path "$Out.startup.log") { Get-Content "$Out.startup.log" } else { @() } }
until (($lines -match '^first_image_on_screen ') -or $p.HasExited -or (Get-Date) -gt $deadline)
[NW]::SetCursorPos(1800, 1120) | Out-Null
Start-Sleep -Seconds $Settle
[NW]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 300
if ($Shot) {
  $rc = New-Object NW+RECT
  if ([NW]::DwmGetWindowAttribute($h, 9, [ref]$rc, 16) -ne 0) { [NW]::GetWindowRect($h, [ref]$rc) | Out-Null }
  $bmp = New-Object System.Drawing.Bitmap ($rc.R - $rc.L), ($rc.B - $rc.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($rc.L, $rc.T, 0, 0, $bmp.Size)
  $bmp.Save("$Out.png", [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  "rect $($rc.L) $($rc.T) $($rc.R) $($rc.B)" | Out-File -Append $log
}
if ($Script) {
  & $Meter ([int64]$h) $p.Id $Script "$Out.json" 2>&1 | Out-File -Append $log
}
# Memory: the app and its engine helper(s).
$p.Refresh()
$mem = "app peak_ws $($p.PeakWorkingSet64) ws $($p.WorkingSet64) private $($p.PrivateMemorySize64) peak_paged $($p.PeakPagedMemorySize64)`n"
foreach ($c in Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)") {
  $cp = Get-Process -Id $c.ProcessId -ErrorAction SilentlyContinue
  if ($cp) { $mem += "child $($c.Name) peak_ws $($cp.PeakWorkingSet64) ws $($cp.WorkingSet64) private $($cp.PrivateMemorySize64)`n" }
}
$mem | Out-File "$Out.mem.txt"
$p.CloseMainWindow() | Out-Null
if (!$p.WaitForExit(10000)) { $p.Kill(); "killed" | Out-File -Append $log }
"exit $($p.ExitCode)" | Out-File -Append $log
New-Item -ItemType File -Force -Path "$Out.done" | Out-Null
