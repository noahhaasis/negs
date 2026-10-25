# Run on a disposable Windows user profile with a freshly compiled installer:
# powershell -File windows/test_uninstall.ps1 -SetupExe target/windows-dist/Negs-Setup.exe
# Refuses an existing installation or app-data directory. No real user data is removed.
param([Parameter(Mandatory = $true)][string]$SetupExe)
$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitProcess) { throw 'Run this test in 64-bit PowerShell.' }
$SetupExe = (Resolve-Path -LiteralPath $SetupExe).Path
$roaming = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Negs'
$local = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Negs'
$scratch = Join-Path ([IO.Path]::GetTempPath()) 'Negs'
$registry = 'HKCU:\Software\Negs'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\dev.negs.app_is1'
foreach ($path in @($roaming, $local, $scratch, $registry, $uninstallKey)) {
    if (Test-Path -LiteralPath $path) { throw "Use a disposable Windows user profile; existing data found at $path" }
}
if (Get-Process -Name Negs -ErrorAction SilentlyContinue) { throw 'Close Negs before testing.' }
$root = Join-Path ([IO.Path]::GetTempPath()) ('negs-uninstall-test-' + [guid]::NewGuid().ToString('N'))
$app = Join-Path $root 'App'
New-Item -ItemType Directory -Path $root | Out-Null
$photo = Join-Path $root 'original.raw'
$export = Join-Path $root 'export.png'
[IO.File]::WriteAllText($photo, 'original photograph')
[IO.File]::WriteAllText($export, 'finished export')

function Run([string]$Exe, [string[]]$Arguments, [bool]$ExpectSuccess = $true) {
    $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -PassThru
    if (-not $process.WaitForExit(30000)) {
        $process.Kill()
        throw "Timed out: $Exe"
    }
    if ($ExpectSuccess -and $process.ExitCode -ne 0) { throw "$Exe failed with $($process.ExitCode)" }
    return $process.ExitCode
}

function Install {
    Run $SetupExe @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$app`"") | Out-Null
}

function Seed {
    foreach ($dir in @($roaming, $local, (Join-Path $local 'Cache\gpu'), $scratch)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }
    [IO.File]::WriteAllText((Join-Path $roaming 'settings.json'), (@{recentFiles = @($photo); lastSaveFolder = $root} | ConvertTo-Json))
    [IO.File]::WriteAllText((Join-Path $roaming 'session.json'), (@{source = $photo} | ConvertTo-Json))
    foreach ($name in @('settings.tmp', 'session.tmp', 'panic.log')) {
        [IO.File]::WriteAllText((Join-Path $roaming $name), 'temporary app data')
    }
    foreach ($name in @('catalog.json', 'catalog.tmp', 'rendered-preview.cache', 'rendered-preview.tmp', 'Cache\gpu\test.bin')) {
        [IO.File]::WriteAllText((Join-Path $local $name), 'cached app data')
    }
    [IO.File]::WriteAllText((Join-Path $scratch 'abandoned.rgba'), 'temporary render')
    # Files the app does not own must survive even inside the app/data folders.
    foreach ($dir in @($app, $roaming, $local)) {
        [IO.File]::WriteAllText((Join-Path $dir 'user-photo.raw'), 'user photograph')
    }
    New-Item -Path "$registry\Updates" -Force | Out-Null
    New-ItemProperty -Path "$registry\Updates" -Name 'TestSetting' -Value 'preserve when asked' -Force | Out-Null
}

function CheckUserFiles {
    if ([IO.File]::ReadAllText($photo) -ne 'original photograph') { throw 'Original photograph changed' }
    if ([IO.File]::ReadAllText($export) -ne 'finished export') { throw 'Export changed' }
    foreach ($dir in @($app, $roaming, $local)) {
        if ([IO.File]::ReadAllText((Join-Path $dir 'user-photo.raw')) -ne 'user photograph') { throw "User file changed in $dir" }
    }
}

function CheckCachesRemoved {
    foreach ($path in @(
        (Join-Path $local 'catalog.json'), (Join-Path $local 'catalog.tmp'),
        (Join-Path $local 'rendered-preview.cache'), (Join-Path $local 'rendered-preview.tmp'),
        (Join-Path $local 'Cache'), (Join-Path $roaming 'panic.log'),
        (Join-Path $roaming 'settings.tmp'), (Join-Path $roaming 'session.tmp'), $scratch
    )) {
        if (Test-Path -LiteralPath $path) { throw "App data remained: $path" }
    }
    if (Test-Path -LiteralPath "$registry\Capabilities") { throw 'File association capabilities remained' }
    if (Test-Path -LiteralPath $uninstallKey) { throw 'Installed Apps registration remained' }
}

try {
    Install
    Seed
    $settings = [IO.File]::ReadAllText((Join-Path $roaming 'settings.json'))
    $session = [IO.File]::ReadAllText((Join-Path $roaming 'session.json'))
    # Installing an update must never execute uninstall cleanup.
    Install
    if ([IO.File]::ReadAllText((Join-Path $roaming 'settings.json')) -ne $settings) { throw 'Upgrade lost settings' }
    if ([IO.File]::ReadAllText((Join-Path $roaming 'session.json')) -ne $session) { throw 'Upgrade lost the session' }
    if (-not (Test-Path -LiteralPath (Join-Path $local 'rendered-preview.cache'))) { throw 'Upgrade removed the cache' }

    $uninstaller = (Get-ChildItem -LiteralPath $app -Filter 'unins*.exe' | Select-Object -First 1).FullName
    # Test the same mutex the app holds, including NEGS_ALLOW_MULTIPLE launches.
    $mutex = [Threading.Mutex]::new($false, 'Local\Negs.AppLifetime')
    try {
        $exit = Run $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/KEEPSETTINGS=0') $false
        if ($exit -eq 0 -or -not (Test-Path -LiteralPath (Join-Path $app 'Negs.exe'))) { throw 'Uninstaller ignored the running-app mutex' }
    } finally { $mutex.Dispose() }

    Run $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/KEEPSETTINGS=1') | Out-Null
    CheckCachesRemoved
    CheckUserFiles
    if ([IO.File]::ReadAllText((Join-Path $roaming 'settings.json')) -ne $settings) { throw 'Keep settings lost preferences' }
    if ([IO.File]::ReadAllText((Join-Path $roaming 'session.json')) -ne $session) { throw 'Keep settings lost the session' }
    if (-not (Test-Path -LiteralPath "$registry\Updates")) { throw 'Keep settings lost update preferences' }

    Install
    Seed
    $uninstaller = (Get-ChildItem -LiteralPath $app -Filter 'unins*.exe' | Select-Object -First 1).FullName
    # Silent removal defaults to full cleanup without requiring a switch.
    Run $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') | Out-Null
    CheckCachesRemoved
    CheckUserFiles
    foreach ($name in @('settings.json', 'settings.tmp', 'session.json', 'session.tmp')) {
        if (Test-Path -LiteralPath (Join-Path $roaming $name)) { throw "Settings remained: $name" }
    }
    if (Test-Path -LiteralPath $registry) { throw 'Update preferences remained' }
    Write-Output 'Passed: upgrade preservation, running-app protection, both cleanup modes, and photograph/export preservation.'
} finally {
    # These locations were absent before the test; all their contents are fixtures.
    $remaining = Get-ChildItem -LiteralPath $app -Filter 'unins*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($remaining) { Run $remaining.FullName @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/KEEPSETTINGS=0') | Out-Null }
    foreach ($path in @($roaming, $local, $scratch, $registry, $root)) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
    }
}
