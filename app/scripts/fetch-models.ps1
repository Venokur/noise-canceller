# Downloads everything the app needs at runtime.
#   native/          sherpa-onnx C API + ONNX Runtime DLLs
#   models/voxlock/  VoxLock int8 model, metadata, test wavs
#   models/denoise/  GTCRN residual denoiser + Silero VAD
param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$native = Join-Path $Root "native"
$models = Join-Path $Root "models\voxlock"
$denoise = Join-Path $Root "models\denoise"
$tmp = Join-Path $Root ".download"

New-Item -ItemType Directory -Force $native, $models, $denoise, $tmp | Out-Null

function Get-File([string]$Url, [string]$Destination) {
    if (Test-Path $Destination) {
        Write-Host "skip  $(Split-Path $Destination -Leaf)"
        return
    }
    Write-Host "fetch $(Split-Path $Destination -Leaf)"

    # BITS is the fastest path locally, but it needs the service and is not
    # always present on a CI runner; fall back instead of failing the build.
    if (Get-Command Start-BitsTransfer -ErrorAction SilentlyContinue) {
        try {
            Start-BitsTransfer -Source $Url -Destination $Destination
            return
        } catch {
            Write-Host "  bits failed, retrying with Invoke-WebRequest"
            Remove-Item $Destination -Force -ErrorAction SilentlyContinue
        }
    }
    # -UseBasicParsing keeps Windows PowerShell from needing the IE engine.
    Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing
}

# 1. VoxLock (sherpa-onnx packaging: adds the metadata the runtime reads)
$modelTar = Join-Path $tmp "sherpa-onnx-voxlock-int8.tar.bz2"
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/speech-enhancement-models/sherpa-onnx-voxlock-int8.tar.bz2" $modelTar

if (-not (Test-Path (Join-Path $models "chunk_int8.onnx"))) {
    Write-Host "extract models"
    tar -xjf $modelTar -C $tmp
    Copy-Item (Join-Path $tmp "sherpa-onnx-voxlock-int8\*") $models -Recurse -Force
}

# 2. Denoiser stages: GTCRN after VoxLock and the Silero VAD gate
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/speech-enhancement-models/gtcrn_simple.onnx" (Join-Path $denoise "gtcrn.onnx")
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx" (Join-Path $denoise "silero_vad.onnx")

# 3. sherpa-onnx native libraries (C API + ONNX Runtime)
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
Write-Host "denoise:"
Get-ChildItem $denoise -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
