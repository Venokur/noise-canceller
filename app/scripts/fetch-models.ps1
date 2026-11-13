# Downloads everything the app needs at runtime.
#   native/          sherpa-onnx C API + ONNX Runtime DLLs
#   models/voxlock/  VoxLock int8 model, metadata, test wavs
#   models/denoise/  GTCRN and DPDFNet residual denoisers + Silero VAD
param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$native = Join-Path $Root "native"
$models = Join-Path $Root "models\voxlock"
$denoise = Join-Path $Root "models\denoise"
$bwe = Join-Path $Root "models\bwe"
$speaker = Join-Path $Root "models\speaker"
$tmp = Join-Path $Root ".download"

New-Item -ItemType Directory -Force $native, $models, $denoise, $bwe, $speaker, $tmp | Out-Null

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
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/speech-enhancement-models/dpdfnet4.onnx" (Join-Path $denoise "dpdfnet4.onnx")
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx" (Join-Path $denoise "silero_vad.onnx")
# 2b. Optional "air" stage: NovaSR 16 kHz -> 48 kHz bandwidth extension
Get-File "https://huggingface.co/TigreGotico/audiosronnx-novasr/resolve/main/novasr.onnx" (Join-Path $bwe "novasr.onnx")
# 2c. Speaker verification (CAM++ English VoxCeleb, 512-dim): compares the live
# voice with the profile and feeds the confidence meter
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx" (Join-Path $speaker "campplus.onnx")

# 3. sherpa-onnx native libraries (C API + ONNX Runtime)
$libsTar = Join-Path $tmp "sherpa-onnx-libs.tar.bz2"
Get-File "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-win-x64-shared-MD-Release-lib.tar.bz2" $libsTar

# The archive also carries sherpa-onnx-cxx-api.dll and
# onnxruntime_providers_shared.dll. This app loads only the C API and the
# ONNX Runtime core; the extra DLLs would still be counted as "shipped" by
# scripts\licenses.ps1 and fail the policy unless they are listed. Keep an
# explicit allow-list, and also drop leftovers from an older fetch or a
# restored CI cache.
$wanted = @("sherpa-onnx-c-api.dll", "onnxruntime.dll")
$missing = @($wanted | Where-Object { -not (Test-Path (Join-Path $native $_)) })
if ($missing.Count -gt 0) {
    Write-Host "extract native libs"
    tar -xjf $libsTar -C $tmp
    # `$tmp` may still hold the extracted VoxLock folder: find the library
    # directory by the DLL itself, not by the first `sherpa-onnx-*` name.
    $srcDir = Get-ChildItem $tmp -Recurse -File -Filter "sherpa-onnx-c-api.dll" |
        Select-Object -First 1 -ExpandProperty Directory
    if (-not $srcDir) { throw "sherpa-onnx archive did not contain sherpa-onnx-c-api.dll" }
    foreach ($name in $wanted) {
        $src = Get-ChildItem $srcDir.FullName -File -Filter $name | Select-Object -First 1
        if (-not $src) { throw "native file not found in the archive: $name" }
        Copy-Item $src.FullName (Join-Path $native $name) -Force
    }
}
Get-ChildItem $native -File | Where-Object {
    $_.Extension -eq ".dll" -and $wanted -notcontains $_.Name
} | ForEach-Object {
    Write-Host "drop  $($_.Name)"
    Remove-Item $_.FullName -Force
}

Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "native:"
Get-ChildItem $native -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
Write-Host "models:"
Get-ChildItem $models -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
Write-Host "denoise:"
Get-ChildItem $denoise -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
Write-Host "bwe:"
Get-ChildItem $bwe -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
Write-Host "speaker:"
Get-ChildItem $speaker -File | ForEach-Object { "  {0,-28} {1,8:0.0} MB" -f $_.Name, ($_.Length / 1MB) }
