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
    [switch]$Check,
    [switch]$Policy
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

# --- licence policy -----------------------------------------------------------
#
# The inventory above only records what is linked. This list decides what is
# allowed to be linked at all: the app is distributed free under MIT, so every
# dependency and every model must stay redistributable without a commercial or
# copyleft obligation.
$allowedLicenses = @(
    'MIT', 'MIT-0', 'APACHE-2.0', 'BSD-2-CLAUSE', 'BSD-3-CLAUSE', 'ISC',
    'ZLIB', '0BSD', 'UNLICENSE', 'CC0-1.0', 'UNICODE-3.0', 'UNICODE-DFS-2016'
)
# MPL-2.0 is file-level copyleft. The project already ships these five
# unmodified crates; everything else needs a review before it is accepted.
$mplAllowed = @('cssparser', 'cssparser-macros', 'selectors', 'dtoa-short', 'option-ext')

function Test-LicenseTerm([string]$term, [string]$name) {
    $term = $term.Trim()
    if ([string]::IsNullOrWhiteSpace($term)) { return $false }
    if ($term -eq 'MPL-2.0') { return ($mplAllowed -contains $name) }
    return ($allowedLicenses -contains $term)
}

function Test-LicenseExpression([string]$expression, [string]$name) {
    if ([string]::IsNullOrWhiteSpace($expression)) { return $false }
    $value = $expression.ToUpperInvariant()
    if ($value -match 'UNKNOWN|SEE PACKAGE|^OTHER$') { return $false }
    # "Apache-2.0 WITH LLVM-exception" is still Apache-2.0 for our purpose
    $value = $value -replace '\s+WITH\s+.*$', ''
    # Parentheses only group OR/AND; flattening them is enough for the
    # permissive expressions this project accepts.
    $value = $value -replace '[()]', ''
    # AND binds tighter: "(MIT OR Apache-2.0) AND Unicode-3.0" needs both
    # groups, while every OR alternative inside a group is a choice.
    $andParts = $value -split '\s+AND\s+'
    if ($andParts.Count -gt 1) {
        foreach ($andPart in $andParts) {
            $any = $false
            foreach ($alternative in ($andPart -split '\s+OR\s+|\s*/\s*')) {
                if (Test-LicenseTerm $alternative $name) { $any = $true }
            }
            if (-not $any) { return $false }
        }
        return $true
    }
    foreach ($alternative in ($value -split '\s+OR\s+|\s*/\s*')) {
        if (Test-LicenseTerm $alternative $name) { return $true }
    }
    return $false
}

function Test-ModelManifest([string]$repo, [string]$app) {
    $problems = New-Object System.Collections.Generic.List[string]
    $manifestPath = Join-Path $repo "MODEL_LICENSES.json"
    if (-not (Test-Path $manifestPath)) {
        $problems.Add("MODEL_LICENSES.json is missing")
        return $problems
    }
    $manifest = Get-Content $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $listed = @{}
    foreach ($model in $manifest.models) {
        if (-not (Test-LicenseExpression $model.license $model.name)) {
            $problems.Add("model '$($model.name)' has a disallowed licence: $($model.license)")
        }
        foreach ($file in @($model.files)) {
            if ([string]::IsNullOrWhiteSpace($file)) { continue }
            $listed[$file] = $true
            if (-not (Test-Path (Join-Path $repo $file))) {
                $problems.Add("model file is missing: $file")
            }
        }
        if ($model.notice) {
            if (-not (Test-Path (Join-Path $repo $model.notice))) {
                $problems.Add("notice is missing: $($model.notice)")
            }
        }
    }

    # Every shipped .onnx and native DLL must be named in the manifest: a new
    # model cannot arrive without its licence being reviewed.
    $shipped = @()
    foreach ($dir in @((Join-Path $app "models"), (Join-Path $app "native"))) {
        if (Test-Path $dir) {
            $shipped += Get-ChildItem $dir -Recurse -File |
                Where-Object { $_.Extension -eq '.onnx' -or $_.Extension -eq '.dll' } |
                ForEach-Object { $_.FullName.Substring($repo.Length + 1).Replace('\', '/') }
        }
    }
    foreach ($file in $shipped) {
        if (-not $listed.ContainsKey($file)) {
            $problems.Add("shipped model/DLL is not in MODEL_LICENSES.json: $file")
        }
    }
    return $problems
}


# --- Rust: crates linked into the Windows build -------------------------------
$tree = & cargo tree --locked --target x86_64-pc-windows-msvc -e normal --prefix none -f "{p}|{l}" 2>$null
if ($LASTEXITCODE -ne 0) {
    throw "cargo tree failed; run this from the workspace (app/) with the registry available"
}

$rust = @{}
# CI sets CARGO_TERM_COLOR=always, so the " (*)" marker of a deduplicated
# dependency arrives wrapped in ANSI escapes and must be stripped first.
$ansi = [regex]"\x1b\[[0-9;]*m"
foreach ($line in $tree) {
    $line = $ansi.Replace($line, '')
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

# --- policy gate --------------------------------------------------------------
$repo = Split-Path $app -Parent
if ($Policy) {
    $problems = New-Object System.Collections.Generic.List[string]
    foreach ($crate in $rust.Values) {
        if (-not (Test-LicenseExpression $crate.license $crate.name)) {
            $problems.Add("crate '$($crate.name) $($crate.version)' has a disallowed licence: $($crate.license)")
        }
    }
    foreach ($package in $npm) {
        if (-not (Test-LicenseExpression $package.license $package.name)) {
            $problems.Add("npm package '$($package.name)' has a disallowed licence: $($package.license)")
        }
    }
    foreach ($problem in (Test-ModelManifest $repo $app)) {
        $problems.Add($problem)
    }
    if ($problems.Count -gt 0) {
        foreach ($problem in $problems) {
            Write-Host ("LICENSE: " + $problem)
        }
        Write-Host ("licence policy failed: {0} problem(s)" -f $problems.Count)
        exit 1
    }
    Write-Host ("licence policy OK: {0} crates, {1} npm packages, models checked" -f $rust.Count, $npm.Count)
    if (-not $Check) { exit 0 }
}

# --- rewrite the blocks -------------------------------------------------------
$text = [System.IO.File]::ReadAllText($notices, [System.Text.Encoding]::UTF8)
# A git checkout on Windows gets CRLF, a local tree may keep LF. Compare and
# write one form, otherwise "out of date" depends on the machine's git settings
# while the generated rows themselves are always joined with LF.
$text = $text -replace "`r`n", "`n"
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
        $old = $before -split "`n"
        $new = $text -split "`n"
        $i = 0
        $limit = [Math]::Min($old.Count, $new.Count)
        while ($i -lt $limit -and $old[$i] -eq $new[$i]) { $i++ }
        Write-Host "THIRD_PARTY_NOTICES.md is out of date; run scripts\licenses.ps1"
        Write-Host ("first difference at line {0}" -f ($i + 1))
        if ($i -lt $old.Count) { Write-Host ("  file: " + $old[$i]) }
        if ($i -lt $new.Count) { Write-Host ("  now : " + $new[$i]) }
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
