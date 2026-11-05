# Regression check for the audio pipeline.
#
# Runs the newest A/B pairs through `nc replay` (same chain as the live engine)
# and checks the metrics from scripts/ab_metrics.py. Needs Python with numpy.
#
#   powershell -File scripts\regression.ps1 [-Count 3] [-Seconds 30]

param(
    [int]$Count = 3,
    [int]$Seconds = 30
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$app = Join-Path $root "app"
$recordings = Join-Path $root "portable\data\recordings"
$profiles = Join-Path $root "portable\data\profiles"
$settings = Join-Path $root "portable\data\settings.json"
$nc = Join-Path $app "target\release\nc.exe"

if (-not (Test-Path $nc)) {
    Write-Host "building nc..."
    Push-Location $app
    cargo build --release -p noise-canceller-cli
    Pop-Location
}

$profile = Get-ChildItem $profiles -Filter *.wav -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $profile) {
    Write-Host "no profile in $profiles" -ForegroundColor Yellow
    exit 2
}

$pairs = Get-ChildItem $recordings -Filter "ab-*-raw.wav" |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First $Count

if (-not $pairs) {
    Write-Host "no A/B recordings in $recordings" -ForegroundColor Yellow
    exit 2
}

$failed = 0
foreach ($pair in $pairs) {
    $clean = $pair.FullName -replace "-raw\.wav$", "-clean.wav"
    if (-not (Test-Path $clean)) { continue }
    $out = Join-Path $env:TEMP ("nc-regression-" + $pair.BaseName + ".wav")

    & $nc replay $pair.FullName $out $profile $settings | Out-Null
    Write-Host ("{0}" -f $pair.BaseName)

    python (Join-Path $PSScriptRoot "ab_metrics.py") $pair.FullName $out
    if ($LASTEXITCODE -ne 0) { $failed++ }
}

if ($failed -gt 0) {
    Write-Host "$failed pair(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host "regression OK" -ForegroundColor Green
