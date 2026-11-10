# Fills portable\ with everything the app needs to run, and with the licenses
# that have to travel with it.
#
#   powershell -File scripts\package-portable.ps1
#   powershell -File scripts\package-portable.ps1 -Zip
#
# What it copies: the exe, the two models, the native DLLs, licenses\ and
# THIRD_PARTY_NOTICES.txt. What it leaves alone: portable\data (the user's own
# settings, profiles and recordings) and the reference wavs, which are only
# needed by `nc selftest`.
#
# The archive is noise-canceller-<version>-portable.zip and holds a single
# top-level folder, so "extract here" does not spill files into Downloads.
# The same layout is what the NSIS installer puts into the install directory:
# see `bundle.resources` in src-tauri\tauri.conf.json.
param(
    [string]$Exe = "",
    [string]$Out = "",
    [string]$Destination = "",
    [string]$Version = "",
    [switch]$SkipExe,
    [switch]$Zip
)

$ErrorActionPreference = "Stop"

$app = Split-Path $PSScriptRoot -Parent
$repo = Split-Path $app -Parent
if (-not $Out) {
    $Out = Join-Path $repo "portable"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (-not (Test-Path $Out)) {
    New-Item -ItemType Directory -Force $Out | Out-Null
}

# Version comes from the same file Tauri bundles with: the archive name has to
# match the installer, otherwise a release holds two files from different builds.
if (-not $Version) {
    $config = Get-Content (Join-Path $app "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
    $Version = $config.version
}

if (-not $SkipExe) {
    if (-not $Exe) {
        $Exe = Join-Path $app "target\release\noise-canceller.exe"
    }
    if (-not (Test-Path $Exe)) {
        throw "exe not found: $Exe (build it with: npm run tauri build -- --no-bundle)"
    }

    # A running instance holds the exe open; fail before copying anything, not
    # halfway through.
    $targetExe = Join-Path $Out "noise-canceller.exe"
    if (Test-Path $targetExe) {
        try {
            $handle = [System.IO.File]::Open($targetExe, "Open", "ReadWrite", "None")
            $handle.Close()
        } catch {
            throw "cannot replace $targetExe - the app is running, close it first"
        }
    }
}

function Copy-Models {
    $src = Join-Path $app "models"
    $dst = Join-Path $Out "models"

    $voxlock = Join-Path $dst "voxlock"
    New-Item -ItemType Directory -Force $voxlock | Out-Null
    foreach ($name in @("enroll.onnx", "chunk_int8.onnx", "README.md")) {
        Copy-Item (Join-Path $src "voxlock\$name") $voxlock -Force
    }

    # test_wavs are for the CLI, not for the app
    $stale = Join-Path $voxlock "test_wavs"
    if (Test-Path $stale) { Remove-Item $stale -Recurse -Force }

    # Only the two stages the app can actually load: dpdfnet2.onnx is a leftover
    # from a removed experiment and must not travel with the build.
    $denoise = Join-Path $dst "denoise"
    New-Item -ItemType Directory -Force $denoise | Out-Null
    foreach ($name in @("gtcrn.onnx", "silero_vad.onnx")) {
        $from = Join-Path $src "denoise\$name"
        if (Test-Path $from) { Copy-Item $from $denoise -Force }
    }
    Get-ChildItem $denoise -Filter "*.onnx" |
        Where-Object { $_.Name -ne "gtcrn.onnx" -and $_.Name -ne "silero_vad.onnx" } |
        ForEach-Object {
            Write-Host "  drop $($_.Name)"
            Remove-Item $_.FullName -Force
        }
}

function Copy-Native {
    $src = Join-Path $app "native"
    $dst = Join-Path $Out "native"
    New-Item -ItemType Directory -Force $dst | Out-Null
    foreach ($dll in (Get-ChildItem $src -Filter "*.dll")) {
        $target = Join-Path $dst $dll.Name
        if (Test-Path $target) {
            try {
                $handle = [System.IO.File]::Open($target, "Open", "ReadWrite", "None")
                $handle.Close()
            } catch {
                throw "cannot replace $target - the app is running, close it first"
            }
        }
        Copy-Item $dll.FullName $target -Force
    }
}

function Copy-Licenses {
    $dst = Join-Path $Out "licenses"
    New-Item -ItemType Directory -Force $dst | Out-Null
    Get-ChildItem (Join-Path $repo "licenses") -File | ForEach-Object {
        Copy-Item $_.FullName $dst -Force
    }
    Copy-Item (Join-Path $repo "LICENSE") (Join-Path $Out "LICENSE.txt") -Force
    Copy-Item (Join-Path $repo "THIRD_PARTY_NOTICES.md") (Join-Path $Out "THIRD_PARTY_NOTICES.txt") -Force
}

if (-not $SkipExe) {
    Copy-Item $Exe $targetExe -Force
}
Copy-Models
Copy-Native
Copy-Licenses

Write-Host "package: $Out"
foreach ($f in (Get-ChildItem $Out -File)) {
    Write-Host ("  {0,-34} {1,8:0.0} MB" -f $f.Name, ($f.Length / 1MB))
}
foreach ($d in (Get-ChildItem $Out -Directory | Where-Object { $_.Name -ne "data" })) {
    $size = (Get-ChildItem $d.FullName -Recurse -File | Measure-Object Length -Sum).Sum
    Write-Host ("  {0,-34} {1,8:0.0} MB" -f ($d.Name + "\"), ($size / 1MB))
}

if ($Zip) {
    if (-not $Destination) {
        $Destination = Split-Path $Out -Parent
    }
    $Destination = [System.IO.Path]::GetFullPath($Destination)
    if (-not (Test-Path $Destination)) {
        New-Item -ItemType Directory -Force $Destination | Out-Null
    }

    # A single top-level folder inside the archive: extracting "here" keeps the
    # app in one place instead of scattering models\ and native\ into Downloads.
    $folder = "noise-canceller-$Version-portable"
    $archive = Join-Path $Destination "$folder.zip"
    if (Test-Path $archive) { Remove-Item $archive -Force }

    $stage = Join-Path $env:TEMP "noise-canceller-package"
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Force $stage | Out-Null

    # data\ holds the author's own voice profiles and recordings; it must never
    # leave the machine.
    Get-ChildItem $Out -Force | Where-Object { $_.Name -ne "data" } | ForEach-Object {
        Copy-Item $_.FullName $stage -Recurse -Force
    }

    # Zipping half a package is worse than not zipping at all.
    foreach ($required in @("noise-canceller.exe", "models\voxlock\chunk_int8.onnx", "native\sherpa-onnx-c-api.dll")) {
        if (-not (Test-Path (Join-Path $stage $required))) {
            throw "incomplete package: $required is missing from $Out"
        }
    }

    # Build the archive entry by entry: Windows PowerShell zips names with
    # backslashes, which the zip spec does not allow and some tools reject.
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zipStream = [System.IO.Compression.ZipFile]::Open(
        $archive,
        [System.IO.Compression.ZipArchiveMode]::Create
    )
    try {
        foreach ($file in (Get-ChildItem $stage -Recurse -File)) {
            $relative = $file.FullName.Substring($stage.Length).TrimStart('\', '/') -replace '\\', '/'
            [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zipStream,
                $file.FullName,
                "$folder/$relative",
                [System.IO.Compression.CompressionLevel]::Optimal
            )
        }
    } finally {
        $zipStream.Dispose()
    }
    Remove-Item $stage -Recurse -Force
    Write-Host ("archive: {0} ({1:0.0} MB)" -f $archive, ((Get-Item $archive).Length / 1MB))
}
