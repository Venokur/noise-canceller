# Fills portable\ with everything the app needs to run, and with the licenses
# that have to travel with it.
#
#   powershell -File scripts\package-portable.ps1
#   powershell -File scripts\package-portable.ps1 -Zip
#
# What it copies: the exe, the models, the native DLLs, licenses\ and
# THIRD_PARTY_NOTICES.txt. What it leaves alone: portable\data (the user's own
# settings, profiles and recordings) and the reference wavs, which are only
# needed by `nc selftest`.
#
# The archive is noise-canceller-<version>-portable.zip. Its entries sit at the
# archive root, so Explorer's "Extract All" creates exactly one folder, named
# after the archive, with noise-canceller.exe directly inside it. Do not wrap
# the entries in a folder of their own: the default extraction folder is already
# named after the archive, and the user then gets a folder inside a folder.
# The same layout is what the NSIS installer puts into the install directory:
# see `bundle.resources` in src-tauri\tauri.conf.json.
param(
    [string]$Exe = "",
    [string]$Out = "",
    [string]$Destination = "",
    [string]$Version = "",
    [switch]$SkipExe,
    [switch]$Zip
)

$ErrorActionPreference = "Stop"

$app = Split-Path $PSScriptRoot -Parent
$repo = Split-Path $app -Parent
. (Join-Path $PSScriptRoot "target-dir.ps1")
$runtime = Get-Content (Join-Path $PSScriptRoot "runtime-files.json") -Raw | ConvertFrom-Json
$runtimeFiles = @($runtime.files)
if (-not $Out) {
    $Out = Join-Path $repo "portable"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (-not (Test-Path $Out)) {
    New-Item -ItemType Directory -Force $Out | Out-Null
}

# Version comes from the same file Tauri bundles with: the archive name has to
# match the installer, otherwise a release holds two files from different builds.
if (-not $Version) {
    $config = Get-Content (Join-Path $app "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
    $Version = $config.version
}

if (-not $SkipExe) {
    if (-not $Exe) {
        $Exe = Join-Path (Get-CargoTargetDir -App $app) "release\noise-canceller.exe"
    }
    if (-not (Test-Path $Exe)) {
        # `cargo metadata` can return the path with a different Unicode
        # normalization of the folder name; the default app\target is still
        # the place the Tauri CLI just wrote to.
        $fallback = Join-Path $app "target\release\noise-canceller.exe"
        if (Test-Path $fallback) { $Exe = $fallback }
    }
    if (-not (Test-Path $Exe)) {
        throw "exe not found: $Exe (build it with: npm run tauri build -- --no-bundle)"
    }

    # A running instance holds the exe open; fail before copying anything, not
    # halfway through.
    $targetExe = Join-Path $Out "noise-canceller.exe"
    if (Test-Path $targetExe) {
        try {
            $handle = [System.IO.File]::Open($targetExe, "Open", "ReadWrite", "None")
            $handle.Close()
        } catch {
            throw "cannot replace $targetExe - the app is running, close it first"
        }
    }
}

function Copy-Runtime {
    foreach ($file in $runtimeFiles) {
        $from = Join-Path $app $file.source
        $target = Join-Path $Out $file.dest
        $required = -not ($file.PSObject.Properties.Name -contains "required") -or $file.required
        if (-not (Test-Path $from)) {
            if ($required) {
                throw "runtime file missing: $($file.source) (run scripts\fetch-models.ps1)"
            }
            Write-Host "  skip optional $($file.source)"
            continue
        }
        $dir = Split-Path $target -Parent
        New-Item -ItemType Directory -Force $dir | Out-Null
        Copy-Item $from $target -Force
    }

    # Leftovers from removed experiments and the reference wavs must not travel
    # with the build. The allow-list is the manifest, not a second list here.
    $allowed = @{}
    foreach ($file in $runtimeFiles) { $allowed[($file.dest -replace '/', '\')] = $true }
    foreach ($dir in @("models\voxlock", "models\denoise", "models\bwe", "models\speaker", "native")) {
        $full = Join-Path $Out $dir
        if (-not (Test-Path $full)) { continue }
        Get-ChildItem $full -File | ForEach-Object {
            $rel = "$dir\$($_.Name)"
            if (-not $allowed.ContainsKey($rel)) {
                Write-Host "  drop $rel"
                Remove-Item $_.FullName -Force
            }
        }
    }
    $stale = Join-Path $Out "models\voxlock\test_wavs"
    if (Test-Path $stale) { Remove-Item $stale -Recurse -Force }
}

function Copy-Licenses {
    $dst = Join-Path $Out "licenses"
    New-Item -ItemType Directory -Force $dst | Out-Null
    Get-ChildItem (Join-Path $repo "licenses") -File | ForEach-Object {
        Copy-Item $_.FullName $dst -Force
    }
    Copy-Item (Join-Path $repo "LICENSE") (Join-Path $Out "LICENSE.txt") -Force
    Copy-Item (Join-Path $repo "THIRD_PARTY_NOTICES.md") (Join-Path $Out "THIRD_PARTY_NOTICES.txt") -Force
    Copy-Item (Join-Path $repo "LICENSING.md") (Join-Path $Out "LICENSING.txt") -Force
    Copy-Item (Join-Path $repo "MODEL_LICENSES.json") (Join-Path $Out "MODEL_LICENSES.json") -Force
}

if (-not $SkipExe) {
    Copy-Item $Exe $targetExe -Force
}
Copy-Runtime
Copy-Licenses

Write-Host "package: $Out"
foreach ($f in (Get-ChildItem $Out -File)) {
    Write-Host ("  {0,-34} {1,8:0.0} MB" -f $f.Name, ($f.Length / 1MB))
}
foreach ($d in (Get-ChildItem $Out -Directory | Where-Object { $_.Name -ne "data" })) {
    $size = (Get-ChildItem $d.FullName -Recurse -File | Measure-Object Length -Sum).Sum
    Write-Host ("  {0,-34} {1,8:0.0} MB" -f ($d.Name + "\"), ($size / 1MB))
}

if ($Zip) {
    if (-not $Destination) {
        $Destination = Split-Path $Out -Parent
    }
    $Destination = [System.IO.Path]::GetFullPath($Destination)
    if (-not (Test-Path $Destination)) {
        New-Item -ItemType Directory -Force $Destination | Out-Null
    }

    # The archive is named after the folder the user ends up with, so the
    # entries themselves live at the archive root. Wrapping them in a folder of
    # their own gives "extract all" users a folder inside a folder.
    $archive = Join-Path $Destination "noise-canceller-$Version-portable.zip"
    if (Test-Path $archive) { Remove-Item $archive -Force }

    $stage = Join-Path $env:TEMP "noise-canceller-package"
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Force $stage | Out-Null

    # data\ holds the author's own voice profiles and recordings; it must never
    # leave the machine.
    Get-ChildItem $Out -Force | Where-Object { $_.Name -ne "data" } | ForEach-Object {
        Copy-Item $_.FullName $stage -Recurse -Force
    }

    # Zipping half a package is worse than not zipping at all. The list comes
    # from the same manifest the copy used; only the optional air model may be
    # absent.
    $requiredPaths = @("noise-canceller.exe")
    foreach ($file in $runtimeFiles) {
        $optional = ($file.PSObject.Properties.Name -contains "required") -and -not $file.required
        if (-not $optional) { $requiredPaths += $file.dest }
    }
    foreach ($requiredPath in $requiredPaths) {
        if (-not (Test-Path (Join-Path $stage $requiredPath))) {
            throw "incomplete package: $requiredPath is missing from $Out"
        }
    }

    # Collect every file together with its name inside the archive by walking the
    # staging folder. The name must not be cut out of `FullName` by its length:
    # $env:TEMP can be an 8.3 path (the GitHub runner uses
    # `C:\Users\RUNNER~1\...`) while Get-ChildItem reports the expanded one
    # (`C:\Users\runneradmin\...`), and the offset is then three characters short.
    # That is how the v0.1.0 archive grew a stray ".../age/" level: the tail of
    # "noise-canceller-package" survived the cut.
    $files = New-Object System.Collections.Generic.List[object]
    $queue = New-Object System.Collections.Generic.Queue[object]
    $queue.Enqueue(@{ Dir = $stage; Prefix = "" })
    while ($queue.Count -gt 0) {
        $node = $queue.Dequeue()
        foreach ($file in (Get-ChildItem -LiteralPath $node.Dir -File)) {
            $files.Add(@{ Path = $file.FullName; Name = ($node.Prefix + $file.Name) })
        }
        foreach ($dir in (Get-ChildItem -LiteralPath $node.Dir -Directory)) {
            $queue.Enqueue(@{ Dir = $dir.FullName; Prefix = ($node.Prefix + $dir.Name + "/") })
        }
    }
    if ($files.Count -eq 0) {
        throw "nothing to archive: $stage is empty"
    }

    # Build the archive entry by entry: Windows PowerShell zips names with
    # backslashes, which the zip spec does not allow and some tools reject.
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zipStream = [System.IO.Compression.ZipFile]::Open(
        $archive,
        [System.IO.Compression.ZipArchiveMode]::Create
    )
    try {
        foreach ($file in $files) {
            [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zipStream,
                $file.Path,
                $file.Name,
                [System.IO.Compression.CompressionLevel]::Optimal
            )
        }
    } finally {
        $zipStream.Dispose()
    }

    # Check what actually landed in the archive, not what we meant to write: a
    # release already went out with everything pushed one level down, and
    # nothing in the build noticed.
    $expected = @(
        "noise-canceller.exe",
        "LICENSE.txt",
        "LICENSING.txt",
        "MODEL_LICENSES.json",
        "README.txt",
        "THIRD_PARTY_NOTICES.txt",
        "models",
        "native",
        "licenses"
    )
    $check = [System.IO.Compression.ZipFile]::OpenRead($archive)
    try {
        foreach ($entry in $check.Entries) {
            $parts = $entry.FullName -split '/'
            if ($expected -notcontains $parts[0]) {
                throw ("archive layout: '$($entry.FullName)' is not one of " +
                    ($expected -join ", "))
            }
            if ($parts.Count -gt 1 -and ($parts -contains "")) {
                throw "archive layout: empty path segment in '$($entry.FullName)'"
            }
        }
        if ($check.Entries.Count -ne $files.Count) {
            throw "archive layout: $($check.Entries.Count) entries in $archive, expected $($files.Count)"
        }
    } finally {
        $check.Dispose()
    }

    Remove-Item $stage -Recurse -Force
    Write-Host ("archive: {0} ({1:0.0} MB)" -f $archive, ((Get-Item $archive).Length / 1MB))
}
