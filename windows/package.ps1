param(
    [string]$WinSparkleDll,
    [string]$AppcastUrl,
    [string]$EdPublicKey,
    [string]$Iscc
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
if ([bool]$AppcastUrl -ne [bool]$EdPublicKey) { throw 'Supply both AppcastUrl and EdPublicKey.' }
if ($AppcastUrl -and -not $WinSparkleDll) { throw 'Configured updates require WinSparkleDll.' }
if ($AppcastUrl -and ([uri]$AppcastUrl).Scheme -ne 'https') { throw 'AppcastUrl must use HTTPS.' }
if ($EdPublicKey -and [Convert]::FromBase64String($EdPublicKey).Length -ne 32) { throw 'EdPublicKey must encode 32 bytes.' }
$env:NEGS_APPCAST_URL = $AppcastUrl
$env:NEGS_ED_PUBLIC_KEY = $EdPublicKey
function Build([string[]]$Arguments) {
    # Windows PowerShell 5 wraps redirected native stderr as ErrorRecord,
    # including harmless compiler warnings. The process exit code is decisive.
    $ErrorActionPreference = 'Continue'
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed.' }
}
# Separate invocations prevent f64 feature unification changing the preview.
Build @('build', '--release', '--locked', '-p', 'spektrafilm-native')
Build @('build', '--release', '--locked', '-p', 'spektrafilm-cli', '--bin', 'spektrafilm')
Build @('build', '--release', '--locked', '-p', 'spektrafilm-cli', '--features', 'precision-f64', '--bin', 'spektrafilm-f64')
Build @('build', '--release', '--locked', '--manifest-path', 'windows/Cargo.toml')
# A fresh staging directory makes repeated packaging safe and reproducible.
$stage = Join-Path $root ('target/windows-package-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item -LiteralPath windows/target/release/Negs.exe -Destination $stage
foreach ($name in @('spektrafilm-native.exe', 'spektrafilm.exe', 'spektrafilm-f64.exe')) {
    Copy-Item -LiteralPath (Join-Path 'target/release' $name) -Destination $stage
}
Copy-Item -LiteralPath data -Destination $stage -Recurse
Copy-Item -LiteralPath LICENSE -Destination $stage
if ($WinSparkleDll) {
    Copy-Item -LiteralPath $WinSparkleDll -Destination (Join-Path $stage 'WinSparkle.dll')
    $license = Join-Path (Split-Path $WinSparkleDll -Parent) 'COPYING'
    if (-not (Test-Path -LiteralPath $license)) { throw 'Place the WinSparkle COPYING license beside the DLL.' }
    Copy-Item -LiteralPath $license -Destination (Join-Path $stage 'WinSparkle-COPYING')
}
$gitRoot = $root.Replace('\', '/')
& git -c "safe.directory=$gitRoot" archive --format=zip "--output=$(Join-Path $stage 'source.zip')" HEAD
if ($LASTEXITCODE -ne 0) { throw 'Source archive failed.' }
$output = Join-Path $root 'target/windows-dist'
New-Item -ItemType Directory -Force -Path $output | Out-Null
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $output 'Negs-portable.zip') -Force
if ($Iscc) {
    & $Iscc "/DStageDir=$stage" "/DOutputDir=$output" (Join-Path $PSScriptRoot 'installer.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
}
Write-Output "Package: $output"
Write-Output "Staging: $stage"
