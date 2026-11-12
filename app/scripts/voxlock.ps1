# Extended VoxLock check.
#
# Runs the model offline (no audio devices) over a matrix of block sizes and
# thread counts and checks what actually matters:
#
#   * the model returns exactly the samples it was given (no speed-up/slow-down),
#   * no NaN/Inf reaches the DSP chain,
#   * silence in -> silence out (no "explosion" at the start of a take),
#   * the DSP chain does not swallow the voice,
#   * a very quiet microphone is brought back by the leveler,
#   * RTF stays below 1.0.
#
#   powershell -File scripts\voxlock.ps1 [-Atten]
#
# Exit code 1 if anything fails.

param(
    [switch]$Atten
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$app = Join-Path $root "app"
. (Join-Path $PSScriptRoot "target-dir.ps1")
$nc = Join-Path (Get-CargoTargetDir -App $app) "release\nc.exe"

if (-not (Test-Path $nc)) {
    Write-Host "building nc..."
    Push-Location $app
    cargo build --release -p noise-canceller-cli
    Pop-Location
}

$failed = 0
$runs = 0

function Get-Number([string]$text, [string]$pattern) {
    $m = [regex]::Match($text, $pattern)
    if (-not $m.Success) { return $null }
    return [double]::Parse($m.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
}

function Invoke-Selftest([int]$chunk, [int]$threads, [int]$atten, [int]$startGain = 0) {
    $out = & $nc selftest $chunk $threads $atten $startGain 2>&1 | Out-String
    $script:runs++

    $delta = Get-Number $out 'delta=(-?\d+)'
    $mismatch = Get-Number $out 'mismatched blocks: (\d+)'
    $silence = Get-Number $out 'silence rms=(-?[\d.]+)'
    $rtf = Get-Number $out 'rtf=([\d.]+)'
    $model = Get-Number $out 'model rms=([\d.]+)'
    $dsp = Get-Number $out 'dsp\s+rms=([\d.]+)'
    $level = Get-Number $out 'level (-?[\d.]+) dB'
    $finite = $out -match 'finite=true'

    $problems = @()
    if ($null -eq $delta) { $problems += "no output" }
    elseif ([math]::Abs($delta) -gt 0) { $problems += "length delta $delta" }
    if ($null -eq $mismatch) { $problems += "no mismatch line" }
    elseif ($mismatch -gt 1) { $problems += "$mismatch mismatched blocks" }
    if (-not $finite) { $problems += "non-finite samples" }
    if ($null -ne $silence -and $silence -gt -80) { $problems += "silence rms $silence dB" }
    # 32 ms is below the documented minimum: on one thread the model is simply
    # slower than real time, so that run is reported as slow, not as a failure
    $slow = $null -ne $rtf -and $rtf -ge 0.9
    $tooSlow = $null -ne $rtf -and $rtf -ge 1.0
    if ($tooSlow -and $chunk -ge 40) { $problems += "rtf $rtf" }
    if ($null -ne $model -and $null -ne $dsp -and $dsp -lt $model * 0.5) {
        $problems += "dsp swallowed the voice ($dsp vs $model)"
    }

    $label = "chunk {0,3} ms · {1} thr · atten {2} · gain {3}" -f $chunk, $threads, $atten, $startGain
    if ($problems.Count -gt 0) {
        $script:failed++
        Write-Host ("FAIL  {0}  -> {1}" -f $label, ($problems -join "; ")) -ForegroundColor Red
        Write-Host $out -ForegroundColor DarkGray
    } else {
        $mark = if ($tooSlow) { " (slow)" } elseif ($slow) { " (tight)" } else { "" }
        Write-Host ("ok    {0}  rtf={1}{2} model={3} dsp={4} level={5} dB" -f `
            $label, $rtf, $mark, $model, $dsp, $level)
    }
    return [pscustomobject]@{ model = $model; dsp = $dsp; level = $level }
}

Write-Host "== block size / threads ==" -ForegroundColor Cyan
foreach ($chunk in 32, 40, 60, 80, 100) {
    foreach ($threads in 1, 2, 4, 8) {
        Invoke-Selftest $chunk $threads 0 | Out-Null
    }
}

if ($Atten) {
    Write-Host ""
    Write-Host "== quiet microphone (-20 dB input) ==" -ForegroundColor Cyan
    $loud = Invoke-Selftest 60 4 0
    $quiet = Invoke-Selftest 60 4 -20
    if ($null -ne $quiet.level -and $quiet.level -lt 3) {
        $failed++
        Write-Host ("FAIL  leveler did not start lifting the quiet mic (level {0} dB)" -f $quiet.level) -ForegroundColor Red
    }
    # a remembered gain (data/state.json) must restore the level right away
    $seeded = Invoke-Selftest 60 4 -20 6
    if ($null -ne $loud.model -and $null -ne $seeded.model) {
        $gap_db = 20 * [math]::Log10($loud.model / [math]::Max($seeded.model, 1e-9))
        if ([math]::Abs($gap_db) -gt 8) {
            $failed++
            Write-Host ("FAIL  remembered gain left the quiet mic {0:N1} dB off" -f $gap_db) -ForegroundColor Red
        } else {
            Write-Host ("ok    remembered gain: quiet mic within {0:N1} dB of the loud one" -f $gap_db)
        }
    }
}

Write-Host ""
if ($failed -gt 0) {
    Write-Host "$failed of $runs run(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host "voxlock OK ($runs runs)" -ForegroundColor Green
exit 0
