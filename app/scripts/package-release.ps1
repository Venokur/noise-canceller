# Builds both artifacts that go to a GitHub release:
#
#   release\noise-canceller-<version>-setup.exe       NSIS installer
#   release\noise-canceller-<version>-portable.zip    portable folder in a zip
#   release\SHA256SUMS.txt
#
#   powershell -File scripts\package-release.ps1
#   powershell -File scripts\package-release.ps1 -SkipBuild
#
# The installer and the portable copy must contain exactly the same files:
# the installer takes them from `bundle.resources` in src-tauri\tauri.conf.json,
# the portable copy from package-portable.ps1. This script fails when the two
# lists drift, because a release where the installer lacks a model cannot be
# caught by looking at the file sizes.
param(
    [string]$Version = "",
    [string]$Out = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$app = Split-Path $PSScriptRoot -Parent
$repo = Split-Path $app -Parent
$configPath = Join-Path $app "src-tauri\tauri.conf.json"
$config = Get-Content $configPath -Raw | ConvertFrom-Json

if (-not $Version) {
    $Version = $config.version
}
if (-not $Out) {
    $Out = Join-Path $repo "release"
}
$Out = [System.IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $Out | Out-Null

# --- what the installer promises to ship -----------------------------------

$resources = $config.bundle.resources
if (-not $resources) {
    throw "bundle.resources is empty in $configPath - the installer would ship without models"
}

$srcRoot = Join-Path $app "src-tauri"
$missing = @()
foreach ($source in $resources.PSObject.Properties.Name) {
    $path = Join-Path $srcRoot $source
    if ($source -match '[*?]') {
        if (-not (Get-ChildItem $path -ErrorAction SilentlyContinue)) { $missing += $source }
    } elseif (-not (Test-Path $path)) {
        $missing += $source
    }
}
if ($missing.Count -gt 0) {
    throw ("bundle.resources points at files that do not exist (run scripts\fetch-models.ps1):`n  " +
        ($missing -join "`n  "))
}

# --- build -----------------------------------------------------------------

$exe = Join-Path $app "target\release\noise-canceller.exe"
$installerDir = Join-Path $app "target\release\bundle\nsis"

if (-not $SkipBuild) {
    Write-Host "build: npm run tauri build"
    Push-Location $app
    try {
        # Only the Tauri CLI: cargo build --release would leave devUrl in the
        # binary and the window would show ERR_CONNECTION_REFUSED.
        & npm.cmd run tauri build
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path $exe)) {
    throw "exe not found: $exe (build it with: npm run tauri build)"
}

$installer = Get-ChildItem $installerDir -Filter "*-setup.exe" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $installer) {
    throw "installer not found in $installerDir (tauri build needs --bundles nsis, see tauri.conf.json)"
}
# A stale bundle from a previous version would be published under the new name.
if ($installer.Name -notlike "*$Version*") {
    throw "installer $($installer.Name) is not version $Version - rebuild without -SkipBuild"
}

$setup = Join-Path $Out "noise-canceller-$Version-setup.exe"
Copy-Item $installer.FullName $setup -Force

# --- portable --------------------------------------------------------------

$portable = Join-Path $repo "portable"
& (Join-Path $PSScriptRoot "package-portable.ps1") -Zip -Version $Version -Destination $Out

$archive = Join-Path $Out "noise-canceller-$Version-portable.zip"
if (-not (Test-Path $archive)) {
    throw "archive not found: $archive"
}

# The portable folder is the reference: every file the installer installs has to
# be there too, under the same name.
$notPorted = @()
foreach ($destination in $resources.PSObject.Properties.Value) {
    if (-not (Test-Path (Join-Path $portable $destination))) { $notPorted += $destination }
}
if ($notPorted.Count -gt 0) {
    throw ("the installer ships files the portable copy does not:`n  " +
        ($notPorted -join "`n  ") +
        "`nadd them to scripts\package-portable.ps1 or drop them from bundle.resources")
}

# --- checksums -------------------------------------------------------------

$sums = Join-Path $Out "SHA256SUMS.txt"
$lines = Get-ChildItem $Out -File |
    Where-Object { $_.Name -ne "SHA256SUMS.txt" } |
    Sort-Object Name |
    ForEach-Object {
        "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
    }
Set-Content $sums $lines -Encoding ASCII

Write-Host ""
Write-Host "release: $Out"
foreach ($file in (Get-ChildItem $Out -File | Sort-Object Name)) {
    Write-Host ("  {0,-40} {1,8:0.0} MB" -f $file.Name, ($file.Length / 1MB))
}
