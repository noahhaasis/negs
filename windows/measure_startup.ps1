param([Parameter(Mandatory=$true)][string]$Photo, [string]$OutputDirectory)
# Launches test processes only. Supply real keyboard input and close each
# window with Computer Use; does not synthesize input or claim readiness.
$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
$Photo=(Resolve-Path -LiteralPath $Photo).Path
if(!$OutputDirectory){$OutputDirectory=Join-Path $repoRoot 'target/windows-perf'}
$benchRoot=Join-Path $OutputDirectory ('startup-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmss'))
New-Item -ItemType Directory -Path $benchRoot | Out-Null
$runs=@()
for($i=1;$i -le 4;$i++) {
    $software=$i -eq 4
    $report=Join-Path $benchRoot "run-$i.log"
    $psi=New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName=Join-Path $repoRoot 'windows\target\release\Negs.exe'
    $psi.WorkingDirectory=$repoRoot
    $psi.UseShellExecute=$false
    $psi.Arguments='"'+$Photo+'"'
    $psi.EnvironmentVariables['NEGS_PROFILE_DIR']=Join-Path $benchRoot $(if($software){'profile-software'}else{'profile-gpu'})
    $psi.EnvironmentVariables['NEGS_ALLOW_MULTIPLE']='1'
    $psi.EnvironmentVariables['NEGS_STARTUP_REPORT']=$report
    $psi.EnvironmentVariables['SPEKTRAFILM_PIPELINE_CACHE_DIR']=Join-Path $benchRoot $(if($software){'cache-software'}else{'cache-gpu'})
    if($software){$psi.EnvironmentVariables['NEGS_SOFTWARE_CANVAS']='1';$psi.EnvironmentVariables['SPEKTRAFILM_BACKEND']='cpu'}
    $p=New-Object System.Diagnostics.Process
    $p.StartInfo=$psi
    $origin=[Diagnostics.Stopwatch]::GetTimestamp()
    [IO.File]::WriteAllText($report,"external_start_call $origin`nexternal_frequency $([Diagnostics.Stopwatch]::Frequency)`n")
    if(!$p.Start()){throw 'Process start failed'}
    Write-Output "RUN $i started; software=$software; PID=$($p.Id); report=$report"
    $deadline=[DateTime]::UtcNow.AddSeconds(90)
    do {
        Start-Sleep -Milliseconds 200
        if($p.HasExited){throw "Run $i exited before image"}
        $lines=Get-Content -LiteralPath $report
    } until(($lines -match '^first_image_on_screen ') -or [DateTime]::UtcNow -gt $deadline)
    if(!($lines -match '^first_image_on_screen ')){throw "Run $i image timeout"}
    Write-Output "RUN $i image presented. Use Computer Use for a real shortcut, then close this test window."
    $p.WaitForExit()
    $lines=Get-Content -LiteralPath $report
    $entry=[ordered]@{run=$i;software=$software;cache=if($i -eq 1 -or $software){'fresh app/profile and pipeline cache; OS/driver warm'}else{'warm app/profile and pipeline cache; fresh process'};exit_code=$p.ExitCode}
    foreach($stamp in @('launched','first_window_composited','inspector_shown','gpu_ready','first_image_scene','first_image_on_screen','first_input_received')) {
        $line=$lines | Where-Object {$_ -match "^$stamp [0-9]+$"} | Select-Object -First 1
        if($line){$entry[$stamp+'_ms']=([long]($line.Split(' ')[1])-$origin)*1000.0/[Diagnostics.Stopwatch]::Frequency}
    }
    $runs += [pscustomobject]$entry
    $runs | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $benchRoot 'summary.json')
    Write-Output ($entry | ConvertTo-Json -Compress)
}
Write-Output "STARTUP_RESULT $benchRoot"
