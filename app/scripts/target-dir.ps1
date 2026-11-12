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
    try {
        $json = (& cargo metadata --format-version 1 --no-deps 2>$null) -join "`n"
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
