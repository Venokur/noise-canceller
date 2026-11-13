# Runs the saved A/B takes through several chain configurations and prints the
# average metrics, so a new model or slider can be compared on this machine
# instead of by ear alone.
#
#   powershell -File scripts\bakeoff.ps1 [-Count 3]
#
# Each config is the portable settings.json with a few fields replaced; the
# replay uses the same `nc replay` as the regression test.

param(
    [int]$Count = 3
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$app = Join-Path $root "app"
. (Join-Path $PSScriptRoot "target-dir.ps1")
$recordings = Join-Path $root "portable\data\recordings"
$profiles = Join-Path $root "portable\data\profiles"
$settings = Join-Path $root "portable\data\settings.json"
$nc = Join-Path (Get-CargoTargetDir -App $app) "release\nc.exe"

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

$pairs = Get-ChildItem $recordings -Filter "ab-*-raw.wav" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First $Count

if (-not $pairs) {
    Write-Host "no A/B recordings in $recordings" -ForegroundColor Yellow
    exit 2
}

$base = Get-Content $settings -Raw | ConvertFrom-Json

# Ordered so the baseline is first in the output.
$configs = [ordered]@{
    "baseline"     = @{}
    "residual_off" = @{ residualDenoise = "off" }
    "gtcrn"        = @{ residualDenoise = "gtcrn"; residualMix = 1.0 }
    "dpdfnet"      = @{ residualDenoise = "dpdfnet"; residualMix = 1.0 }
    "residual_75"  = @{ residualMix = 0.75 }
    "air_0"        = @{ airAmount = 0.0 }
    "air_15"       = @{ airAmount = 0.15 }
    "deess"        = @{ deessEnabled = $true; deessDb = 6.0 }
    "cap_8"        = @{ maxSuppressionDb = 8.0 }
    "cap_0"        = @{ maxSuppressionDb = 0.0 }
}

$settingsFiles = @{}
foreach ($name in $configs.Keys) {
    $cfg = @{}
    foreach ($property in $base.PSObject.Properties) {
        $cfg[$property.Name] = $property.Value
    }
    foreach ($key in $configs[$name].Keys) {
        $cfg[$key] = $configs[$name][$key]
    }
    $path = Join-Path $env:TEMP "nc-bakeoff-$name.json"
    $cfg | ConvertTo-Json -Depth 20 | Set-Content -Path $path -Encoding utf8
    $settingsFiles[$name] = $path
}

$rows = @()
foreach ($pair in $pairs) {
    $raw = $pair.FullName
    foreach ($name in $configs.Keys) {
        $out = Join-Path $env:TEMP ("nc-bakeoff-" + $name + "-" + $pair.BaseName + ".wav")
        & $nc replay $raw $out $profile $settingsFiles[$name] | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Host ("{0}: replay failed for {1}" -f $name, $pair.BaseName) -ForegroundColor Red
            continue
        }
        $m = & $nc metrics $raw $out --json | ConvertFrom-Json
        $rows += [pscustomobject]@{
            take     = $pair.BaseName
            config   = $name
            speech   = [double]$m.speech_level_db
            level    = [double]$m.level_delta_db
            contrast = [double]$m.pause_contrast_db
            dropout  = [double]$m.longest_dropout_s
        }
    }
}

$summary = $rows | Group-Object config | ForEach-Object {
    $group = $_.Group
    [pscustomobject]@{
        config      = $_.Name
        speech_db   = [math]::Round(($group | Measure-Object speech -Average).Average, 1)
        level_db    = [math]::Round(($group | Measure-Object level -Average).Average, 1)
        contrast_db = [math]::Round(($group | Measure-Object contrast -Average).Average, 1)
        dropout_max = [math]::Round(($group | Measure-Object dropout -Maximum).Maximum, 2)
        takes       = $group.Count
    }
}

$summary | Sort-Object config | Format-Table -AutoSize | Out-String -Width 200
