# Cheap guard for the Windows installer: it runs without the models (they are
# not in git), but still catches the mistakes that only show up on a real
# release - a deleted entry, an absolute path, two sources writing to the same
# file, or a missing model the app cannot start without.
#
#   powershell -File scripts\check-bundle.ps1
param()

$ErrorActionPreference = "Stop"

$app = Split-Path $PSScriptRoot -Parent
$configPath = Join-Path $app "src-tauri\tauri.conf.json"
$config = Get-Content $configPath -Raw | ConvertFrom-Json

$problems = @()

if ($config.bundle.targets -notcontains "nsis") {
    $problems += "bundle.targets does not include nsis"
}

$resources = $config.bundle.resources
if (-not $resources) {
    $problems += "bundle.resources is empty: the installer would ship without models"
    $resources = [pscustomobject]@{}
}

$destinations = @{}
foreach ($source in $resources.PSObject.Properties.Name) {
    $destination = $resources.$source
    if ([System.IO.Path]::IsPathRooted($source)) {
        $problems += "resource source is absolute: $source"
    }
    if ([System.IO.Path]::IsPathRooted($destination) -or $destination -match '(^|[\\/])\.\.([\\/]|$)') {
        $problems += "resource destination escapes the install directory: $destination"
    }
    if ($destinations.ContainsKey($destination)) {
        $problems += "two resources write to $destination"
    }
    $destinations[$destination] = $source
}

# The app resolves these next to the exe (core/src/paths.rs, core/src/model.rs);
# without them it starts but cannot load a model or a DLL.
foreach ($required in @(
        "models/voxlock/enroll.onnx",
        "models/voxlock/chunk_int8.onnx",
        "models/denoise/gtcrn.onnx",
        "models/denoise/dpdfnet4.onnx",
        "models/denoise/silero_vad.onnx",
        "models/bwe/novasr.onnx",
        "native/sherpa-onnx-c-api.dll",
        "native/onnxruntime.dll",
        "THIRD_PARTY_NOTICES.txt"
    )) {
    if (-not $destinations.ContainsKey($required)) {
        $problems += "bundle.resources does not install $required"
    }
}

if ($problems.Count -gt 0) {
    Write-Host "bundle config:"
    $problems | ForEach-Object { Write-Host "  $_" }
    exit 1
}

Write-Host "bundle config ok: $($destinations.Count) files"
