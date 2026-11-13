# Where Cargo actually keeps its build cache.
#
# Today that is `app/target`, but the location is not a constant: a local
# `.cargo/config.toml`, `CARGO_TARGET_DIR` or a differently moved workspace
# change it, and a script that joins `$app` and "target" by hand would then look
# for the exe and the NSIS installer in a directory nothing writes to.
# `cargo metadata` reports the path a build would really use.
#
# Run it from `app`: Cargo reads config from the current directory upward, not
# from `--manifest-path`, so the caller has to be in the workspace root for the
# path below to be the one a build would actually use.

function Get-CargoTargetDir {
    param([Parameter(Mandatory)][string]$App)

    Push-Location $App
    # `cargo metadata` prints UTF-8, but Windows PowerShell 5.1 decodes native
    # output with the OEM code page. This repository lives in a folder whose name
    # contains a typographic hyphen (U+2011); decoding it as CP866/CP1251 turned
    # it into three Cyrillic letters, and the returned path did not exist — so
    # regression.ps1 could not find nc.exe and packaging silently fell back to
    # app\target. Read the output as UTF-8 and restore the setting afterwards.
    $previous = [Console]::OutputEncoding
    try {
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
        $json = (& cargo metadata --format-version 1 --no-deps 2>$null) -join "`n"
    } finally {
        [Console]::OutputEncoding = $previous
    }

    try {
        if ($LASTEXITCODE -ne 0 -or -not $json) {
            throw "cargo metadata failed in $App"
        }
        $dir = ($json | ConvertFrom-Json).target_directory
    } finally {
        Pop-Location
    }

    if (-not $dir) { throw "cargo metadata reported no target_directory" }
    return $dir
}
