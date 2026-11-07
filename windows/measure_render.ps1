param(
    [Parameter(Mandatory = $true)][string]$Photo,
    [string]$Films = 'kodak_gold_200,kodak_portra_400',
    [int]$Steps = 10,
    [ValidateSet('on', 'off')][string]$Match = 'off',
    [ValidateSet('vulkan', 'cpu')][string]$Backend = 'vulkan',
    # Rayon threads for the CPU fallback (the helper's RAYON_NUM_THREADS).
    [int]$Threads = 6,
    # The window's client size in DIPs, at the top left of the main monitor.
    [int]$Width = 1600,
    [int]$Height = 1000,
    [int]$SettleMs = 1500,
    [int]$TimeoutS = 900,
    [string]$OutputDirectory,
    [string]$Label = ''
)
# One render benchmark run of the release app (docs/windows-app.md, "Render
# timing"): a fresh profile, the photograph on the command line, and
# NEGS_RENDER_BENCH alternating the films. Writes the trace, the helper's
# memory samples and a summary (windows/render_trace.py) to a new folder.
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$Photo = (Resolve-Path -LiteralPath $Photo).Path
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repoRoot 'target/windows-perf' }
$name = 'render-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmss') + $(if ($Label) { "-$Label" } else { '' })
$run = Join-Path $OutputDirectory $name
$profile = Join-Path $run 'profile'
New-Item -ItemType Directory -Force -Path $profile | Out-Null
$settings = [ordered]@{
    previews = $false; autoPlaceNewImages = ($Match -eq 'on'); checkForUpdates = $false
    window = @(1, 0, 0, $Width, $Height); inspectorTabChosen = $true; inspectorTab = 'Prep'
}
$settings | ConvertTo-Json | Set-Content -Encoding ascii (Join-Path $profile 'settings.json')
$trace = Join-Path $run 'trace.jsonl'
$exe = Join-Path $repoRoot 'windows\target\release\Negs.exe'
$helper = (Resolve-Path (Join-Path $repoRoot 'target\release\spektrafilm-native.exe')).Path
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $exe
$psi.WorkingDirectory = $repoRoot
$psi.UseShellExecute = $false
$psi.Arguments = '"' + $Photo + '"'
$psi.EnvironmentVariables['NEGS_PROFILE_DIR'] = $profile
$psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE'] = '1'
$psi.EnvironmentVariables['NEGS_RENDER_TRACE'] = $trace
$psi.EnvironmentVariables['NEGS_RENDER_BENCH'] = $Films
$psi.EnvironmentVariables['NEGS_RENDER_BENCH_STEPS'] = "$Steps"
$psi.EnvironmentVariables['NEGS_RENDER_BENCH_SETTLE_MS'] = "$SettleMs"
$psi.EnvironmentVariables['NEGS_RENDER_BENCH_TIMEOUT_S'] = "$TimeoutS"
# The Vulkan pipeline cache is kept across runs (warm), apart from Noah's.
$psi.EnvironmentVariables['SPEKTRAFILM_PIPELINE_CACHE_DIR'] = Join-Path $OutputDirectory 'render-pipeline-cache'
if ($Backend -eq 'cpu') {
    $psi.EnvironmentVariables['SPEKTRAFILM_BACKEND'] = 'cpu'
    $psi.EnvironmentVariables['RAYON_NUM_THREADS'] = "$Threads"
}
$p = [System.Diagnostics.Process]::Start($psi)
$samples = New-Object System.Collections.Generic.List[string]
$samples.Add('qpc,pid,working_set,peak_working_set')
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutS * ($Steps + 2))
while (!$p.HasExited) {
    foreach ($h in Get-Process spektrafilm-native -ErrorAction SilentlyContinue) {
        try {
            if ($h.Path -eq $helper -and $h.StartTime -ge $p.StartTime) {
                $samples.Add("$([Diagnostics.Stopwatch]::GetTimestamp()),$($h.Id),$($h.WorkingSet64),$($h.PeakWorkingSet64)")
            }
        } catch {}
    }
    if ([DateTime]::UtcNow -gt $deadline) { $p.Kill(); throw "timed out: $run" }
    Start-Sleep -Milliseconds 100
}
$samples | Set-Content -Encoding ascii (Join-Path $run 'helper-memory.csv')
$meta = [ordered]@{
    photo = $Photo; films = $Films; steps = $Steps; match = $Match; backend = $Backend
    threads = $(if ($Backend -eq 'cpu') { $Threads } else { $null }); window_dip = @($Width, $Height)
    exit_code = $p.ExitCode; commit = (git -C $repoRoot rev-parse --short HEAD)
}
$meta | ConvertTo-Json | Set-Content -Encoding ascii (Join-Path $run 'run.json')
python -I (Join-Path $PSScriptRoot 'render_trace.py') $run
Write-Output "RENDER_RUN $run"
