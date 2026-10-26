param(
    [string]$IconSource = "C:\Users\syntxbench\Downloads\photo_2026-06-10_17-59-14.jpg",
    [string]$BuildTargetDir = "target\windows-app",
    [string]$DistDir = "dist\spektrafilm-windows-cuda",
    [switch]$Aio
)

$ErrorActionPreference = "Stop"

function Write-U16([System.IO.BinaryWriter]$Writer, [int]$Value) {
    $Writer.Write([uint16]$Value)
}

function Write-U32([System.IO.BinaryWriter]$Writer, [long]$Value) {
    $Writer.Write([uint32]$Value)
}

function New-AppIcon {
    param(
        [string]$Source,
        [string]$Destination
    )

    Add-Type -AssemblyName System.Drawing

    $sizes = @(256, 128, 64, 48, 32, 16)
    $src = [System.Drawing.Image]::FromFile((Resolve-Path -LiteralPath $Source))
    try {
        $side = [Math]::Min($src.Width, $src.Height)
        $cropX = [int](($src.Width - $side) / 2)
        $cropY = [int](($src.Height - $side) / 2)
        $crop = [System.Drawing.Rectangle]::new($cropX, $cropY, $side, $side)
        $images = @()

        foreach ($size in $sizes) {
            $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            try {
                $g = [System.Drawing.Graphics]::FromImage($bmp)
                try {
                    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
                    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                    $g.Clear([System.Drawing.Color]::Transparent)
                    $g.DrawImage($src, [System.Drawing.Rectangle]::new(0, 0, $size, $size), $crop, [System.Drawing.GraphicsUnit]::Pixel)
                } finally {
                    $g.Dispose()
                }

                $ms = [System.IO.MemoryStream]::new()
                try {
                    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
                    $images += [pscustomobject]@{ Size = $size; Bytes = $ms.ToArray() }
                } finally {
                    $ms.Dispose()
                }
            } finally {
                $bmp.Dispose()
            }
        }

        $parent = Split-Path -Parent $Destination
        if ($parent) {
            New-Item -ItemType Directory -Force -Path $parent | Out-Null
        }

        $fs = [System.IO.File]::Open($Destination, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
        try {
            $bw = [System.IO.BinaryWriter]::new($fs)
            try {
                Write-U16 $bw 0
                Write-U16 $bw 1
                Write-U16 $bw $images.Count

                $offset = 6 + 16 * $images.Count
                foreach ($img in $images) {
                    $encodedSize = if ($img.Size -eq 256) { 0 } else { $img.Size }
                    $bw.Write([byte]$encodedSize)
                    $bw.Write([byte]$encodedSize)
                    $bw.Write([byte]0)
                    $bw.Write([byte]0)
                    Write-U16 $bw 1
                    Write-U16 $bw 32
                    Write-U32 $bw $img.Bytes.Length
                    Write-U32 $bw $offset
                    $offset += $img.Bytes.Length
                }

                foreach ($img in $images) {
                    $bw.Write($img.Bytes)
                }
            } finally {
                $bw.Dispose()
            }
        } finally {
            $fs.Dispose()
        }
    } finally {
        $src.Dispose()
    }
}

function Set-ExeIcon {
    param(
        [string]$ExePath,
        [string]$IconPath
    )

    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class NativeResource {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr BeginUpdateResource(string pFileName, bool bDeleteExistingResources);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool UpdateResource(IntPtr hUpdate, IntPtr lpType, IntPtr lpName, ushort wLanguage, byte[] lpData, uint cbData);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool EndUpdateResource(IntPtr hUpdate, bool fDiscard);
}
"@

    $icoBytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $IconPath))
    $reader = [System.IO.BinaryReader]::new([System.IO.MemoryStream]::new($icoBytes))
    try {
        $reserved = $reader.ReadUInt16()
        $type = $reader.ReadUInt16()
        $count = $reader.ReadUInt16()
        if ($reserved -ne 0 -or $type -ne 1 -or $count -lt 1) {
            throw "Invalid .ico file: $IconPath"
        }

        $entries = @()
        for ($i = 0; $i -lt $count; $i++) {
            $width = $reader.ReadByte()
            $height = $reader.ReadByte()
            $colorCount = $reader.ReadByte()
            $entryReserved = $reader.ReadByte()
            $planes = $reader.ReadUInt16()
            $bitCount = $reader.ReadUInt16()
            $bytesInRes = $reader.ReadUInt32()
            $imageOffset = $reader.ReadUInt32()
            $imageBytes = New-Object byte[] $bytesInRes
            [Array]::Copy($icoBytes, [int]$imageOffset, $imageBytes, 0, [int]$bytesInRes)
            $entries += [pscustomobject]@{
                Width = $width
                Height = $height
                ColorCount = $colorCount
                Reserved = $entryReserved
                Planes = $planes
                BitCount = $bitCount
                BytesInRes = $bytesInRes
                Bytes = $imageBytes
                ResourceId = 100 + $i
            }
        }
    } finally {
        $reader.Dispose()
    }

    $exe = (Resolve-Path -LiteralPath $ExePath).Path
    $handle = [NativeResource]::BeginUpdateResource($exe, $false)
    if ($handle -eq [IntPtr]::Zero) {
        throw "BeginUpdateResource failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    $discard = $true
    try {
        foreach ($entry in $entries) {
            $ok = [NativeResource]::UpdateResource(
                $handle,
                [IntPtr]3,
                [IntPtr]$entry.ResourceId,
                [uint16]1033,
                $entry.Bytes,
                [uint32]$entry.Bytes.Length
            )
            if (-not $ok) {
                throw "UpdateResource RT_ICON failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
            }
        }

        $groupStream = [System.IO.MemoryStream]::new()
        $groupWriter = [System.IO.BinaryWriter]::new($groupStream)
        try {
            Write-U16 $groupWriter 0
            Write-U16 $groupWriter 1
            Write-U16 $groupWriter $entries.Count
            foreach ($entry in $entries) {
                $groupWriter.Write([byte]$entry.Width)
                $groupWriter.Write([byte]$entry.Height)
                $groupWriter.Write([byte]$entry.ColorCount)
                $groupWriter.Write([byte]$entry.Reserved)
                Write-U16 $groupWriter $entry.Planes
                Write-U16 $groupWriter $entry.BitCount
                Write-U32 $groupWriter $entry.BytesInRes
                Write-U16 $groupWriter $entry.ResourceId
            }
            $groupBytes = $groupStream.ToArray()
        } finally {
            $groupWriter.Dispose()
            $groupStream.Dispose()
        }

        $ok = [NativeResource]::UpdateResource(
            $handle,
            [IntPtr]14,
            [IntPtr]1,
            [uint16]1033,
            $groupBytes,
            [uint32]$groupBytes.Length
        )
        if (-not $ok) {
            throw "UpdateResource RT_GROUP_ICON failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }

        $discard = $false
    } finally {
        [NativeResource]::EndUpdateResource($handle, $discard) | Out-Null
    }
}

function ConvertTo-RustString {
    param([string]$Value)

    return $Value.Replace("\", "\\").Replace('"', '\"')
}

function New-EmbedManifest {
    param(
        [string]$DataDir,
        [string]$F64Exe,
        [string]$Destination
    )

    $dataRoot = (Resolve-Path -LiteralPath $DataDir).Path
    $f64Path = (Resolve-Path -LiteralPath $F64Exe).Path
    $files = Get-ChildItem -LiteralPath $dataRoot -Recurse -File | Sort-Object FullName

    $hashLines = New-Object System.Collections.Generic.List[string]
    $entries = New-Object System.Collections.Generic.List[string]
    $dataPrefix = $dataRoot.TrimEnd("\", "/") + [System.IO.Path]::DirectorySeparatorChar
    foreach ($file in $files) {
        $rel = $file.FullName.Substring($dataPrefix.Length).Replace("\", "/")
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $hashLines.Add("data/$rel|$($file.Length)|$hash")
        $entries.Add("    (`"$(ConvertTo-RustString $rel)`", include_bytes!(r#`"$($file.FullName)`"#) as &[u8]),")
    }
    $f64Hash = (Get-FileHash -LiteralPath $f64Path -Algorithm SHA256).Hash.ToLowerInvariant()
    $hashLines.Add("spektrafilm-f64.exe|$((Get-Item -LiteralPath $f64Path).Length)|$f64Hash")

    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hashBytes = [System.Text.Encoding]::UTF8.GetBytes(($hashLines -join "`n"))
        $bundleHash = [System.BitConverter]::ToString($sha.ComputeHash($hashBytes)).Replace("-", "").ToLowerInvariant().Substring(0, 16)
    } finally {
        $sha.Dispose()
    }

    $parent = Split-Path -Parent $Destination
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }

    $manifest = New-Object System.Collections.Generic.List[string]
    $manifest.Add("// Generated by scripts/package_windows_app.ps1 -Aio.")
    $manifest.Add("pub const BUNDLE_ID: &str = `"0.1.2-$bundleHash`";")
    $manifest.Add("pub static DATA_FILES: &[(&str, &[u8])] = &[")
    foreach ($entry in $entries) {
        $manifest.Add($entry)
    }
    $manifest.Add("];")
    $manifest.Add("pub const F64_EXE: &[u8] = include_bytes!(r#`"$f64Path`"#);")
    Set-Content -LiteralPath $Destination -Value $manifest -Encoding UTF8
}

$repo = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")
$iconPath = Join-Path $repo "target\packaging\spektrafilm.ico"

Push-Location $repo
try {
    New-AppIcon -Source $IconSource -Destination $iconPath

    if ($Aio) {
        cargo build --release -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64 --target-dir $BuildTargetDir

        $releaseDir = Join-Path $BuildTargetDir "release"
        $manifestPath = Join-Path $repo "target\packaging\embedded_bundle.rs"
        New-EmbedManifest -DataDir "data" -F64Exe (Join-Path $releaseDir "spektrafilm-f64.exe") -Destination $manifestPath

        $oldEmbed = $env:SPEKTRAFILM_EMBED_MANIFEST
        try {
            $env:SPEKTRAFILM_EMBED_MANIFEST = (Resolve-Path -LiteralPath $manifestPath).Path
            cargo build --release -p spektrafilm-gui --features spektrafilm-gpu/cuda-backend --target-dir $BuildTargetDir
        } finally {
            if ($null -eq $oldEmbed) {
                Remove-Item Env:\SPEKTRAFILM_EMBED_MANIFEST -ErrorAction SilentlyContinue
            } else {
                $env:SPEKTRAFILM_EMBED_MANIFEST = $oldEmbed
            }
        }

        $guiExe = Join-Path $releaseDir "spektrafilm-gui.exe"
        Set-ExeIcon -ExePath $guiExe -IconPath $iconPath

        if (Test-Path -LiteralPath $DistDir) {
            Remove-Item -LiteralPath $DistDir -Recurse -Force
        }
        New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
        $aioExe = Join-Path $DistDir "spektrafilm-gui-aio.exe"
        Copy-Item -LiteralPath $guiExe -Destination $aioExe
        Set-ExeIcon -ExePath $aioExe -IconPath $iconPath
        @"
spektrafilm-rs Windows AIO

Run spektrafilm-gui-aio.exe.

This single exe embeds:
- application data/profiles/LUTs
- spektrafilm-f64.exe for CPU f64 Export

On first launch it extracts the embedded payload to:
%LOCALAPPDATA%\spektrafilm\embedded

Backends:
- Default launch uses WGSL/wgpu when available, then CPU.
- Set SPEKTRAFILM_BACKEND=cuda to request native CUDA on NVIDIA systems.
"@ | Set-Content -Encoding UTF8 (Join-Path $DistDir "README.txt")

        Write-Host "Packaged AIO: $(Resolve-Path -LiteralPath $aioExe)"
        Write-Host "Icon:         $(Resolve-Path -LiteralPath $iconPath)"
    } else {
        cargo build --release -p spektrafilm-gui --features spektrafilm-gpu/cuda-backend --target-dir $BuildTargetDir
        cargo build --release -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64 --target-dir $BuildTargetDir

        $releaseDir = Join-Path $BuildTargetDir "release"
        $guiExe = Join-Path $releaseDir "spektrafilm-gui.exe"
        Set-ExeIcon -ExePath $guiExe -IconPath $iconPath

        if (Test-Path -LiteralPath $DistDir) {
            Remove-Item -LiteralPath $DistDir -Recurse -Force
        }
        New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
        Copy-Item -LiteralPath $guiExe -Destination (Join-Path $DistDir "spektrafilm-gui.exe")
        Copy-Item -LiteralPath (Join-Path $releaseDir "spektrafilm-f64.exe") -Destination (Join-Path $DistDir "spektrafilm-f64.exe")
        Copy-Item -LiteralPath "data" -Destination (Join-Path $DistDir "data") -Recurse

        Set-ExeIcon -ExePath (Join-Path $DistDir "spektrafilm-gui.exe") -IconPath $iconPath

        Write-Host "Packaged: $(Resolve-Path -LiteralPath $DistDir)"
        Write-Host "Icon:     $(Resolve-Path -LiteralPath $iconPath)"
    }
} finally {
    Pop-Location
}
