# Startup medians, ABBA-interleaved, in Noah's console session (see ../pc_console.sh).
# Usage: startup.ps1 -A <Negs.exe> [-B <Negs.exe>] [-Runs 6] [-Photo docs\card.jpg] [-Theme light|dark]
# Each window closes itself once the photograph is on screen. Warm profile and
# pipeline cache per build (one warm-up each). Prints medians in ms from process start.
param([Parameter(Mandatory=$true)][string]$A, [string]$B, [int]$Runs = 6,
      [string]$Photo = 'C:\Users\Noah\Projects\negs-presets\docs\card.jpg', [string]$Theme)
$ErrorActionPreference = 'Stop'
$root = Join-Path $env:USERPROFILE ('.negs\startup\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$stamps = 'first_window_composited', 'inspector_shown', 'gpu_ready', 'first_image_on_screen'
$builds = [ordered]@{ A = $A }
if ($B) { $builds['B'] = $B }
$results = @{}
foreach ($k in $builds.Keys) { $results[$k] = @() }

function RunOnce([string]$key, [string]$exe, [string]$tag) {
  $report = Join-Path $root "$key-$tag.log"
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $exe; $psi.WorkingDirectory = Split-Path $exe; $psi.UseShellExecute = $false
  $psi.Arguments = '"' + $Photo + '"'
  $psi.EnvironmentVariables['NEGS_PROFILE_DIR'] = Join-Path $root "profile-$key"
  $psi.EnvironmentVariables['SPEKTRAFILM_PIPELINE_CACHE_DIR'] = Join-Path $root "cache-$key"
  $psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE'] = '1'
  $psi.EnvironmentVariables['NEGS_STARTUP_REPORT'] = $report
  $psi.EnvironmentVariables['SPEKTRAFILM_ROOT'] = 'C:\Users\Noah\Projects\negs-presets'
  if ($Theme) { $psi.EnvironmentVariables['NEGS_THEME'] = $Theme }
  $origin = [Diagnostics.Stopwatch]::GetTimestamp()
  $p = [System.Diagnostics.Process]::Start($psi)
  $deadline = (Get-Date).AddSeconds(60)
  do { Start-Sleep -Milliseconds 100; $lines = if (Test-Path $report) { Get-Content $report } else { @() } }
  until (($lines -match '^first_image_on_screen ') -or $p.HasExited -or (Get-Date) -gt $deadline)
  Start-Sleep -Milliseconds 300
  $lines = Get-Content $report
  $p.Refresh(); $p.CloseMainWindow() | Out-Null
  if (!$p.WaitForExit(8000)) { $p.Kill(); Write-Output "  $key $tag killed" }
  $e = [ordered]@{}
  foreach ($s in $stamps) {
    $l = $lines | Where-Object { $_ -match "^$s [0-9]+$" } | Select-Object -First 1
    $e[$s] = if ($l) { [math]::Round(([long]($l.Split(' ')[1]) - $origin) * 1000.0 / [Diagnostics.Stopwatch]::Frequency, 1) } else { $null }
  }
  return [pscustomobject]$e
}

foreach ($k in $builds.Keys) { RunOnce $k $builds[$k] 'warmup' | Out-Null }
for ($i = 0; $i -lt $Runs; $i++) {
  $order = @($builds.Keys)
  if ($B -and ($i % 4 -eq 1 -or $i % 4 -eq 2)) { [array]::Reverse($order) }
  foreach ($k in $order) {
    $r = RunOnce $k $builds[$k] "run$i"
    $results[$k] += $r
    Write-Output ("  $k run$i " + ($r | ConvertTo-Json -Compress))
  }
}
function Median($xs) { $s = @($xs | Where-Object { $_ -ne $null } | Sort-Object); if ($s.Count -eq 0) { return $null }; $m = [int][math]::Floor($s.Count / 2); if ($s.Count % 2) { $s[$m] } else { ($s[$m - 1] + $s[$m]) / 2 } }
foreach ($k in $builds.Keys) {
  $line = "MEDIAN $k ($($builds[$k])):"
  foreach ($s in $stamps) { $line += " $s=" + (Median ($results[$k] | ForEach-Object { $_.$s })) }
  Write-Output $line
}
Write-Output "logs $root"
