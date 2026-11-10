# Regenerates the dependency inventory inside THIRD_PARTY_NOTICES.md.
#
#   powershell -File scripts\licenses.ps1          # rewrite the tables
#   powershell -File scripts\licenses.ps1 -Check   # exit 1 if they are stale
#
# The Rust side is what actually links into the Windows binary, the npm side is
# what ends up in the frontend bundle. Dev-only crates and packages are skipped:
# they never reach the user.
#
# Only the table rows are generated; the header row lives in the markdown file,
# so this script stays ASCII and works in Windows PowerShell 5.1 without a BOM.
param(
    [switch]$Check
)

$ErrorActionPreference = "Stop"
$app = Split-Path $PSScriptRoot -Parent
$notices = Join-Path (Split-Path $app -Parent) "THIRD_PARTY_NOTICES.md"

if (-not (Test-Path $notices)) {
    throw "THIRD_PARTY_NOTICES.md not found at $notices"
}

function New-Rows($rows) {
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($row in $rows) {
        $out.Add("| " + ($row -join " | ") + " |")
    }
    return ($out -join "`n")
}

# --- Rust: crates linked into the Windows build -------------------------------
$tree = & cargo tree --locked --target x86_64-pc-windows-msvc -e normal --prefix none -f "{p}|{l}" 2>$null
if ($LASTEXITCODE -ne 0) {
    throw "cargo tree failed; run this from the workspace (app/) with the registry available"
}

$rust = @{}
foreach ($line in $tree) {
    if ($line -notmatch '\|') { continue }
    $left, $right = $line -split '\|', 2
    $license = ($right -replace '\s*\(\*\)\s*$', '').Trim()
    # cargo tree appends " (proc-macro)" / " (path+file://...)" to some entries
    $left = ($left -replace '\s+\([^)]*\)$', '').Trim()
    if ($left -notmatch '^(\S+) v(\S+)$') { continue }
    $name = $Matches[1]
    $version = $Matches[2]
    if ($name -like 'noise-canceller*') { continue }
    $rust["$name|$version"] = [pscustomobject]@{ name = $name; version = $version; license = $license }
}

$rustRows = New-Rows ($rust.Values |
    Sort-Object name, version |
    ForEach-Object { , @($_.name, $_.version, $_.license) })

# --- npm: packages bundled into the frontend ----------------------------------
# The lockfile does not reliably mark dev-only packages, so walk the runtime
# dependency graph from the root instead: that is what Vite inlines into the
# bundle. Compiler and build tooling (TypeScript, Babel, PostCSS) drops out.
$lockPath = Join-Path $app "package-lock.json"
# Windows PowerShell refuses an empty property name, and lockfile v3 has one for
# the root package; rename it on the way in.
$raw = Get-Content $lockPath -Raw -Encoding UTF8
$raw = $raw -replace '(?m)^(\s*)"":(\s*)\{', '$1"__root__":$2{'
$lock = $raw | ConvertFrom-Json
$packages = $lock.packages

function Resolve-Package([string]$name) {
    $direct = "node_modules/$name"
    if ($packages.PSObject.Properties.Name -contains $direct) { return $packages.$direct }
    $nested = $packages.PSObject.Properties |
        Where-Object { $_.Name -like "*/node_modules/$name" } |
        Select-Object -First 1
    if ($nested) { return $nested.Value }
    return $null
}

$npm = @()
$seen = @{}
$queue = New-Object System.Collections.Queue
foreach ($name in $packages.__root__.dependencies.PSObject.Properties.Name) { $queue.Enqueue($name) }

while ($queue.Count -gt 0) {
    $name = $queue.Dequeue()
    if ($seen.ContainsKey($name)) { continue }
    $seen[$name] = $true

    $pkg = Resolve-Package $name
    if (-not $pkg) { continue }
    $license = $pkg.license
    if (-not $license) { $license = "see package" }
    $npm += [pscustomobject]@{ name = $name; version = $pkg.version; license = $license }

    foreach ($group in @($pkg.dependencies, $pkg.optionalDependencies)) {
        if (-not $group) { continue }
        foreach ($dep in $group.PSObject.Properties.Name) { $queue.Enqueue($dep) }
    }
}

$npmRows = New-Rows ($npm |
    Sort-Object name |
    ForEach-Object { , @($_.name, $_.version, $_.license) })

# --- rewrite the blocks -------------------------------------------------------
$text = [System.IO.File]::ReadAllText($notices, [System.Text.Encoding]::UTF8)
$before = $text
$text = [regex]::Replace(
    $text,
    '(?s)(<!-- inventory:rust:start -->\r?\n).*?(<!-- inventory:rust:end -->)',
    { param($m) $m.Groups[1].Value + $rustRows + "`n" + $m.Groups[2].Value }
)
$text = [regex]::Replace(
    $text,
    '(?s)(<!-- inventory:npm:start -->\r?\n).*?(<!-- inventory:npm:end -->)',
    { param($m) $m.Groups[1].Value + $npmRows + "`n" + $m.Groups[2].Value }
)

Write-Host ("rust crates: {0}, npm packages: {1}" -f $rust.Count, $npm.Count)

if ($Check) {
    if ($text -ne $before) {
        Write-Host "THIRD_PARTY_NOTICES.md is out of date; run scripts\licenses.ps1"
        exit 1
    }
    Write-Host "THIRD_PARTY_NOTICES.md is up to date"
    exit 0
}

if ($text -ne $before) {
    [System.IO.File]::WriteAllText($notices, $text, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "THIRD_PARTY_NOTICES.md updated"
} else {
    Write-Host "THIRD_PARTY_NOTICES.md already up to date"
}
