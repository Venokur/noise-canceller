# Golden check for the Rust audio core.
# Runs the pipeline on the bundled clip and fails if the numbers drift.
#
#   .\scripts\golden.ps1
#
# Expected (reference build, 80 ms blocks):
#   mic 0.0534 · model 0.0417 · model peak 0.437 · dsp ~0.028 · RTF < 0.9

$ErrorActionPreference = "Stop"

Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $output = & cargo run -q --release -p noise-canceller-cli -- selftest 80 2>&1
    $text = $output -join "`n"
    Write-Host $text

    function Assert-Range([string]$name, [double]$actual, [double]$min, [double]$max) {
        if ($actual -lt $min -or $actual -gt $max) {
            throw "$name out of range: $actual (expected $min..$max)"
        }
        Write-Host ("  ok  {0,-12} {1}" -f $name, $actual)
    }

    $mic = [regex]::Match($text, 'mic\s+rms=([0-9.]+)')
    $model = [regex]::Match($text, 'model rms=([0-9.]+) peak=([0-9.]+)')
    $dsp = [regex]::Match($text, 'dsp\s+rms=([0-9.]+) peak=([0-9.]+)')
    $rtf = [regex]::Match($text, 'rtf=([0-9.]+)')

    if (-not $mic.Success -or -not $model.Success -or -not $dsp.Success -or -not $rtf.Success) {
        throw "could not parse selftest output"
    }

    $micRms = [double]::Parse($mic.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
    $modelRms = [double]::Parse($model.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
    $modelPeak = [double]::Parse($model.Groups[2].Value, [Globalization.CultureInfo]::InvariantCulture)
    $dspRms = [double]::Parse($dsp.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
    $dspPeak = [double]::Parse($dsp.Groups[2].Value, [Globalization.CultureInfo]::InvariantCulture)
    $rtfValue = [double]::Parse($rtf.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)

    Assert-Range "mic rms" $micRms 0.040 0.070
    Assert-Range "model rms" $modelRms 0.025 0.060
    Assert-Range "model peak" $modelPeak 0.30 0.60
    Assert-Range "dsp rms" $dspRms 0.020 0.120
    Assert-Range "dsp peak" $dspPeak 0.25 0.99
    Assert-Range "rtf" $rtfValue 0.05 0.90

    # the chain must not kill the signal
    if ($dspRms -lt $modelRms * 0.5) {
        throw "dsp output is far quieter than the model output - check the DSP chain"
    }

    Write-Host ""
    Write-Host "GOLDEN OK"
}
finally {
    Pop-Location
}
