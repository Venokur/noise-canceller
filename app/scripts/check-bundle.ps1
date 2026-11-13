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
# without them it starts but cannot load a model or a DLL. The list is not
# written here: it is `scripts/runtime-files.json`, the same file
# `package-portable.ps1` copies from. The two used to be separate lists and had
# already drifted.
$runtime = Get-Content (Join-Path $PSScriptRoot "runtime-files.json") -Raw | ConvertFrom-Json
$manifest = @{}
foreach ($file in @($runtime.files)) {
    $manifest[$file.dest] = $file.source
    $optional = ($file.PSObject.Properties.Name -contains "required") -and -not $file.required
    if (-not $optional -and -not $destinations.ContainsKey($file.dest)) {
        $problems += "bundle.resources does not install $($file.dest)"
    }
    if ($destinations.ContainsKey($file.dest)) {
        $source = $destinations[$file.dest]
        if (-not $source.EndsWith($file.source)) {
            $problems += "bundle.resources maps $($file.dest) from $source, expected .../$($file.source)"
        }
    }
}

# A resource under models/ or native/ that is not in the manifest would travel
# in the installer but not in the portable zip (or the other way round).
foreach ($destination in $destinations.Keys) {
    if (($destination -like "models/*" -or $destination -like "native/*") -and -not $manifest.ContainsKey($destination)) {
        $problems += "bundle.resources installs $destination, but runtime-files.json does not list it"
    }
}

if ($problems.Count -gt 0) {
    Write-Host "bundle config:"
    $problems | ForEach-Object { Write-Host "  $_" }
    exit 1
}

Write-Host "bundle config ok: $($destinations.Count) files"
