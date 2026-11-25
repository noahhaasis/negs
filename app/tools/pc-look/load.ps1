# CPU use per process over 5 s, the top ten, and the free memory.
$a = @{}; Get-Process | ForEach-Object { $a[$_.Id] = $_.TotalProcessorTime.TotalMilliseconds }
Start-Sleep -Seconds 5
$rows = Get-Process | ForEach-Object { [pscustomobject]@{ Name = $_.Name; Id = $_.Id; CpuPct = [math]::Round((($_.TotalProcessorTime.TotalMilliseconds - $a[$_.Id]) / 5000.0) * 100 / [Environment]::ProcessorCount, 1) } }
$rows | Sort-Object CpuPct -Descending | Select-Object -First 10 | Format-Table -AutoSize | Out-String -Width 120
"free MB: " + [int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1024)
