# Drives the GPUI app from the keyboard and mouse and captures the screen.
param([string]$Exe, [string]$Photo, [string]$Profile, [string]$Out, [switch]$EndSession, [switch]$Tips)
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $Out, $Profile | Out-Null
$log = Join-Path $Out 'log.txt'
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class V {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
'@
[V]::SetProcessDPIAware() | Out-Null
function Key($vk) { [V]::keybd_event($vk, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 30; [V]::keybd_event($vk, 0, 2, [IntPtr]::Zero); Start-Sleep -Milliseconds 250 }
function Chord($mod, $vk) { [V]::keybd_event($mod, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 40; Key $vk; [V]::keybd_event($mod, 0, 2, [IntPtr]::Zero); Start-Sleep -Milliseconds 250 }
function Shot($name) {
  Start-Sleep -Milliseconds 400
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap 1700, 1150
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  "shot $name" | Out-File -Append $log
}
function Hover($x, $y) { [V]::SetCursorPos($x - 6, $y); Start-Sleep -Milliseconds 60; [V]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 1300 }
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Exe; $psi.WorkingDirectory = Split-Path $Exe; $psi.UseShellExecute = $false
$psi.Arguments = '"' + $Photo + '"'
$psi.EnvironmentVariables['NEGS_PROFILE_DIR'] = $Profile
$psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE'] = '1'
$psi.EnvironmentVariables['SPEKTRAFILM_ROOT'] = 'C:\Users\Noah\Projects\negs'
$psi.EnvironmentVariables['NEGS_STARTUP_REPORT'] = (Join-Path $Out 'startup.log')
$p = [System.Diagnostics.Process]::Start($psi)
$deadline = (Get-Date).AddSeconds(60)
while ($p.MainWindowHandle -eq [IntPtr]::Zero -and -not $p.HasExited -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $p.Refresh() }
$h = $p.MainWindowHandle
[V]::ShowWindow($h, 9) | Out-Null
[V]::SetWindowPos($h, [IntPtr]::Zero, 40, 40, 1600, 1050, 0x0040) | Out-Null
Key 0x12
[V]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Seconds 7
[V]::SetCursorPos(600, 500)
Shot '01-start'
if ($Tips) {
  Hover 1240 787; Shot 'tip-highlights'
  Hover 1310 606; Shot 'tip-match'
  [V]::SetCursorPos(1800, 1120)
} elseif (-not $EndSession) {
  Key 0x12; Shot '02-alt-cues'; Key 0x1B
  Chord 0x12 0x50; Shot '03-alt-p-photo'
  Key 0x27; Shot '04-right-view'
  Key 0x28; Key 0x28; Shot '05-down-down'
  Key 0x1B; Key 0x1B; Key 0x1B
  Key 0x79; Key 0x28; Shot '06-f10-down-file'
  Key 0x1B; Key 0x1B; Key 0x1B
  # An edit, then Edit shows Undo with its name.
  [V]::SetCursorPos(1559, 668); Start-Sleep -Milliseconds 100
  [V]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero)
  for ($x = 1559; $x -ge 1460; $x -= 9) { [V]::SetCursorPos($x, 668); Start-Sleep -Milliseconds 30 }
  [V]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Seconds 2
  Chord 0x12 0x45; Shot '07-alt-e-edit'
  Key 0x1B; Key 0x1B
  # Alt+P, then the Filter submenu by its key.
  Chord 0x12 0x50; Key 0x49; Shot '08-photo-filter'
  Key 0x1B; Key 0x1B; Key 0x1B
  Hover 1227 643; Shot '09-tip-exposure'
  Hover 685 65; Shot '10-tip-grid'
  Hover 1360 1046; Shot '11-tip-export-or-row'
}
if ($EndSession) {
  $before = Get-ChildItem $Profile | Select-Object Name, LastWriteTime, Length | Out-String
  "before:`n$before" | Out-File -Append $log
  Start-Sleep -Seconds 2
  $r = [IntPtr]::Zero
  $ok = [V]::SendMessageTimeoutW($h, 0x11, [IntPtr]::Zero, [IntPtr]::Zero, 0, 5000, [ref]$r)
  "WM_QUERYENDSESSION sent=$ok result=$r" | Out-File -Append $log
  $ok = [V]::SendMessageTimeoutW($h, 0x16, [IntPtr]1, [IntPtr]::Zero, 0, 10000, [ref]$r)
  "WM_ENDSESSION sent=$ok result=$r alive=$(-not $p.HasExited)" | Out-File -Append $log
  $after = Get-ChildItem $Profile | Select-Object Name, LastWriteTime, Length | Out-String
  "after:`n$after" | Out-File -Append $log
  Get-Content (Join-Path $Profile 'settings.json') -ErrorAction SilentlyContinue | Select-String -Pattern 'window' | Out-File -Append $log
  Stop-Process -Id $p.Id -Force
} else {
  $p.CloseMainWindow() | Out-Null
  if (!$p.WaitForExit(10000)) { $p.Kill(); "killed" | Out-File -Append $log }
}
"exit" | Out-File -Append $log
