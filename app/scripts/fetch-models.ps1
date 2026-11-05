# Downloads everything the app needs at runtime.
#   native/          sherpa-onnx C API + ONNX Runtime DLLs
#   models/voxlock/  VoxLock int8 model, metadata, test wavs
param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$native = Join-Path $Root "native"
$models = Join-Path $Root "models\voxlock"
$tmp = Join-Path $Root ".download"

New-Item -ItemType Directory -Force $native, $models, $tmp | Out-Null

function Get-File([string]$Url, [string]$Destination) {
    if (Test-Path $Destination) {
        Write-Host "skip  $(Split-Path $Destination -Leaf)"
        return
    }
    Write-Host "fetch $(Split-Path $Destination -Leaf)"
    Start-BitsTransfer -Source $Url -Destination $Destination
}

# 1. VoxLock (sherpa-onnx packaging: adds the metadata the runtime reads)
$modelTar = Join-Path $tmp "sherpa-onnx-voxlock-int8.tar.bz2"
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/speech-enhancement-models/sherpa-onnx-voxlock-int8.tar.bz2" $modelTar

if (-not (Test-Path (Join-Path $models "chunk_int8.onnx"))) {
    Write-Host "extract models"
    tar -xjf $modelTar -C $tmp
    Copy-Item (Join-Path $tmp "sherpa-onnx-voxlock-int8\*") $models -Recurse -Force
}

# 2. sherpa-onnx native libraries (C API + ONNX Runtime)
$libsTar = Join-Path $tmp "sherpa-onnx-libs.tar.bz2"
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-win-x64-shared-MD-Release-lib.tar.bz2" $libsTar

if (-not (Test-Path (Join-Path $native "sherpa-onnx-c-api.dll"))) {
    Write-Host "extract native libs"
    tar -xjf $libsTar -C $tmp
    $dir = Get-ChildItem $tmp -Directory | Where-Object { $_.Name -like "sherpa-onnx-*" } | Select-Object -First 1
    Get-ChildItem $dir.FullName -Recurse -Filter "*.dll" | ForEach-Object {
        Copy-Item $_.FullName $native -Force
    }
}

Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "native:"
Get-ChildItem $native -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
Write-Host "models:"
Get-ChildItem $models -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
