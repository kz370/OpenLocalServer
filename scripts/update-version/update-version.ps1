<#
.SYNOPSIS
Updates the OpenLocalServer version (the app, and the administrator helper when it changed).

.DESCRIPTION
The APP version is set in every file AGENTS.md section 5 lists:
  - Cargo.toml ([workspace.package] version: ols-core, ols-cli (olsc), ols-mcp and
    the desktop app all use it, so olsc --version and MCP serverInfo match the app)
  - src-tauri/tauri.conf.json, ui/package.json, ui/package-lock.json
  - Cargo.lock (ols-core, ols-cli, ols-mcp, openlocalserver)
  - README.md (version badge and installer name in the Download section)
  - CHANGELOG.md (adds the [version] section if missing)
  - release-notes/v<version>.md (creates a draft if missing)
  - ui/src/pages/Settings.tsx (release-notes import and the version fallbacks)
  - crates/ols-core/catalog/user_manual.md and docs/OLS_USER_MANUAL.md
    (Version header, Table of Contents line and Section 17 heading, plus an empty
    "What's New in v<version>" block when there is none yet)
  - index.html (download asset names, the version fact badge and the footer)

The ADMINISTRATOR HELPER has its own version: crates/ols-helper/Cargo.toml and
ADMIN_HELPER_VERSION in crates/ols-core/src/elevate.rs (kept equal). A new helper
version makes Settings offer a one-time elevated update, so it changes only when the
helper changed. With the default -Helper auto, the helper follows the app version when
its code (crates/ols-helper, hosts.rs, exec.rs, domain/store.rs) changed since its
version was last bumped (git history plus uncommitted changes), and keeps it otherwise.

Every change is prepared in memory first. If any expected text is not found, the
script stops and writes nothing. Unchanged files are reported as UNCHANGED.

Not changed on purpose (do these by hand): the release-notes bullets, the content
of manual Section 17, and the release highlights on index.html.

.PARAMETER Version
The target app version (e.g. 2.3.0, v2.3.0, 3.0.0-rc.1). If omitted, the script asks.
If omitted while -HelperVersion is given, only the helper changes (helper-only mode).

.PARAMETER Helper
What happens to the helper version during an app bump:
  auto (default)  follow the app version only if helper code changed since its last bump
  bump            follow the app version
  keep            do not change it

.PARAMETER HelperVersion
Set the helper to this exact version (x.y.z), whatever -Helper says.

.PARAMETER Unreleased
Write "Unreleased" instead of today's date in new CHANGELOG and release-notes entries.
On release day, run the script again for the same version with -Force and without
-Unreleased: it replaces "Unreleased" with today's date.

.PARAMETER Force
Do not ask questions: no "re-apply?" prompt, and -Helper auto applies without asking.

.PARAMETER DryRun
Preview changes without writing anything to disk.

.PARAMETER SkipLockCheck
Skip the "cargo metadata --locked" lockfile verification.

.PARAMETER NoChangelog
Do not add a new version entry to CHANGELOG.md.

.PARAMETER NoNotes
Do not generate a draft release-notes/v<version>.md.

.EXAMPLE
scripts\update-version.bat 2.3.0
scripts\update-version.bat 2.3.0 -Unreleased -DryRun
scripts\update-version.bat 2.3.0 -Helper bump
scripts\update-version.bat 2.3.0 -Helper keep
scripts\update-version.bat -HelperVersion 2.3.1
#>

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Version,

    [ValidateSet('auto', 'bump', 'keep')]
    [string]$Helper = 'auto',
    [string]$HelperVersion,

    [switch]$Unreleased,
    [switch]$Force,
    [switch]$DryRun,
    [switch]$SkipLockCheck,
    [switch]$NoChangelog,
    [switch]$NoNotes,

    [Alias('h', '?')]
    [switch]$Help
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Non-ASCII characters are built from code points so the script reads the same
# under Windows PowerShell 5.1 (which reads BOM-less files as ANSI) and pwsh 7.
$emDash = [string][char]0x2014
$midDot = [string][char]0x00B7

if ($Help) {
    Write-Host ""
    Write-Host "Usage: update-version.bat [version] [options]" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Arguments:"
    Write-Host "  version              New app version (e.g. 2.3.0, v2.3.0). olsc and ols-mcp"
    Write-Host "                       always share it; only the helper has its own."
    Write-Host ""
    Write-Host "Administrator helper version:"
    Write-Host "  -Helper auto         Default. Follow the app version only if helper code"
    Write-Host "                       changed since the helper's last bump."
    Write-Host "  -Helper bump         Follow the app version."
    Write-Host "  -Helper keep         Do not change it."
    Write-Host "  -HelperVersion <v>   Set it to exactly <v> (x.y.z). Without an app version,"
    Write-Host "                       only the helper changes."
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Unreleased          Date new CHANGELOG / release-notes entries 'Unreleased'."
    Write-Host "                       Re-run later with -Force (without -Unreleased) to set today's date."
    Write-Host "  -Force               Ask nothing (re-apply the current version, accept auto choices)"
    Write-Host "  -DryRun              Preview file updates without modifying files"
    Write-Host "  -SkipLockCheck       Skip the cargo metadata --locked verification"
    Write-Host "  -NoChangelog         Do not add an entry to CHANGELOG.md"
    Write-Host "  -NoNotes             Do not generate release-notes/v<version>.md draft"
    Write-Host "  -Help, -h, -?        Show this help message"
    Write-Host ""
    Write-Host "Nothing is written unless every expected piece of text was found."
    Write-Host "A new helper version makes Settings offer a one-time elevated update, so"
    Write-Host "bump it only when the helper (or hosts.rs, exec.rs, domain/store.rs) changed."
    Write-Host ""
    exit 0
}

# Resolve repository root
$repoRoot = (Resolve-Path "$PSScriptRoot\..\..").Path

function Write-Diagnostic {
    param(
        [Parameter(Mandatory = $true)][string]$Problem,
        [Parameter(Mandatory = $true)][string]$Cause,
        [Parameter(Mandatory = $true)][string]$Fix
    )
    Write-Host ""
    Write-Host "[x] Problem: $Problem" -ForegroundColor Red
    Write-Host "    Cause:   $Cause" -ForegroundColor Yellow
    Write-Host "    Fix:     $Fix" -ForegroundColor Cyan
    Write-Host ""
}

function Stop-Bump([string]$Problem, [string]$Cause, [string]$Fix) {
    Write-Diagnostic -Problem $Problem -Cause $Cause -Fix $Fix
    Write-Host "Nothing was written." -ForegroundColor Yellow
    exit 1
}

# ---------------------------------------------------------------------------
# In-memory edits: every change lands in $pending first and is written by
# Write-Pending at the end, so a failed step leaves the repository untouched.
# ---------------------------------------------------------------------------
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$pending = [ordered]@{}   # relative path -> new content
$original = @{}           # relative path -> content on disk ($null for new files)
$hadBom = @{}             # relative path -> file started with a UTF-8 BOM

function Test-RepoFile([string]$relativePath) {
    return Test-Path (Join-Path $repoRoot $relativePath)
}

function Get-Text([string]$relativePath) {
    if ($pending.Contains($relativePath)) { return [string]$pending[$relativePath] }
    $fullPath = Join-Path $repoRoot $relativePath
    if (-not (Test-Path $fullPath)) {
        Stop-Bump "Target file missing: $relativePath" `
            "The file does not exist at expected path '$fullPath'." `
            "Check out the missing file or verify repository integrity."
    }
    $bytes = [System.IO.File]::ReadAllBytes($fullPath)
    $hadBom[$relativePath] = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
    $text = [System.IO.File]::ReadAllText($fullPath, [System.Text.Encoding]::UTF8)
    $original[$relativePath] = $text
    $pending[$relativePath] = $text
    return $text
}

function Set-Text([string]$relativePath, [string]$content) {
    if (-not $original.ContainsKey($relativePath)) { $original[$relativePath] = $null; $hadBom[$relativePath] = $false }
    $pending[$relativePath] = $content
}

# Replaces $Pattern in a file. Stops the whole run when the text is not found,
# unless -Optional. -Expect N requires exactly N matches; -First replaces only the first.
function Edit-Text {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Pattern,
        [Parameter(Mandatory = $true)][string]$Replacement,
        [Parameter(Mandatory = $true)][string]$What,
        [int]$Expect = 0,
        [switch]$First,
        [switch]$Optional
    )
    $text = Get-Text $Path
    $rx = [regex]::new($Pattern)
    $count = $rx.Matches($text).Count
    if ($count -eq 0) {
        if ($Optional) { return 0 }
        Stop-Bump "Could not find $What in $Path." `
            "The text this script looks for (pattern: $Pattern) is not in the file; it was probably reworded or moved." `
            "Restore the expected text in $Path, or update the pattern in scripts/update-version/update-version.ps1."
    }
    if ($Expect -gt 0 -and $count -ne $Expect) {
        Stop-Bump "Found $count matches for $What in $Path, expected $Expect." `
            "The file has more or fewer copies of this text than the script was written for." `
            "Check $Path by hand, then adjust the expected count in scripts/update-version/update-version.ps1."
    }
    if ($First) { $new = $rx.Replace($text, $Replacement, 1) } else { $new = $rx.Replace($text, $Replacement) }
    Set-Text $Path $new
    return $count
}

function Write-Pending {
    foreach ($rel in @($pending.Keys)) {
        $new = [string]$pending[$rel]
        $old = $original[$rel]
        if ($null -ne $old -and $new -ceq $old) {
            Write-Host "  [UNCHANGED] $rel" -ForegroundColor Gray
            continue
        }
        $label = if ($null -eq $old) { 'CREATED' } else { 'UPDATED' }
        if ($DryRun) {
            Write-Host "  [PREVIEW]   $rel ($($label.ToLower()))" -ForegroundColor Magenta
            continue
        }
        $fullPath = Join-Path $repoRoot $rel
        $encoding = if ($hadBom[$rel]) { New-Object System.Text.UTF8Encoding($true) } else { $utf8NoBom }
        [System.IO.File]::WriteAllText($fullPath, $new, $encoding)
        Write-Host "  [$label]   $rel" -ForegroundColor Green
    }
}

function Test-SemVer([string]$value) {
    return $value -match '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'
}

function ConvertTo-PlainVersion([string]$value) {
    $value = $value.Trim()
    if ($value -match '^[vV](\d+.*)$') { $value = $Matches[1] }
    return $value
}

# -1 / 0 / 1 on major.minor.patch (pre-release suffixes are ignored for this check)
function Compare-CoreVersion([string]$a, [string]$b) {
    $pa = @(($a -split '[-+]')[0].Split('.') | ForEach-Object { [int]$_ })
    $pb = @(($b -split '[-+]')[0].Split('.') | ForEach-Object { [int]$_ })
    for ($i = 0; $i -lt 3; $i++) {
        if ($pa[$i] -lt $pb[$i]) { return -1 }
        if ($pa[$i] -gt $pb[$i]) { return 1 }
    }
    return 0
}

# ---------------------------------------------------------------------------
# Administrator helper: the only part with its own version
# ---------------------------------------------------------------------------
$elevatePath = "crates/ols-core/src/elevate.rs"
$helperCargo = "crates/ols-helper/Cargo.toml"
$helperConstPattern = '(?m)(^pub const ADMIN_HELPER_VERSION: &str = )"([^"]+)"'
$packageVersionPattern = '(?ms)(^\[package\]\r?\n(?:(?!^\[)[^\r\n]*\r?\n)*?^version\s*=\s*)"([^"]+)"'

# Files the resident helper service runs. Changing any of them without bumping
# the helper version leaves users on the old administrator copy with no prompt.
$helperWatchPaths = @(
    "crates/ols-helper/src",
    "crates/ols-helper/Cargo.toml",
    "crates/ols-core/src/hosts.rs",
    "crates/ols-core/src/exec.rs",
    "crates/ols-core/src/domain/store.rs"
)

function Get-CurrentHelperVersion {
    $c = [regex]::Match((Get-Text $elevatePath), $helperConstPattern)
    if (-not $c.Success) {
        Stop-Bump "Could not find ADMIN_HELPER_VERSION in $elevatePath." `
            "The constant was renamed, moved, or no longer has the form 'pub const ADMIN_HELPER_VERSION: &str = `"x.y.z`";'." `
            "Restore the constant in $elevatePath, or update `$helperConstPattern in this script."
    }
    $m = [regex]::Match((Get-Text $helperCargo), $packageVersionPattern)
    if (-not $m.Success) {
        Stop-Bump "Could not find the [package] version in $helperCargo." `
            "The helper needs its own 'version = `"x.y.z`"' line (not 'version.workspace = true') in [package]." `
            "Put 'version = `"$($c.Groups[2].Value)`"' (the ADMIN_HELPER_VERSION value) in the [package] section of $helperCargo."
    }
    if ($m.Groups[2].Value -ne $c.Groups[2].Value) {
        Write-Host "[!] $helperCargo says $($m.Groups[2].Value) but ADMIN_HELPER_VERSION is $($c.Groups[2].Value); the constant wins and both are set together on the next helper bump." -ForegroundColor Yellow
    }
    return $c.Groups[2].Value
}

function Set-HelperVersion([string]$newVersion) {
    $replacement = '${1}"' + $newVersion + '"'
    $null = Edit-Text -Path $elevatePath -Pattern $helperConstPattern -Replacement $replacement `
        -What "ADMIN_HELPER_VERSION" -Expect 1
    $null = Edit-Text -Path $helperCargo -Pattern $packageVersionPattern -Replacement $replacement `
        -What "the [package] version" -First
    $null = Edit-Text -Path "Cargo.lock" -What "the ols-helper package entry" -Expect 1 `
        -Pattern '(?ms)(^\[\[package\]\]\r?\nname\s*=\s*"ols-helper"\r?\nversion\s*=\s*)"[^"]+"' `
        -Replacement $replacement
}

# Helper files changed since ADMIN_HELPER_VERSION was last edited, including
# uncommitted changes. $null when git or the history is unavailable.
function Get-HelperChanges {
    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) { return $null }
    Push-Location $repoRoot
    try {
        $bump = (& git log -1 --format=%H -G 'ADMIN_HELPER_VERSION: &str' -- $elevatePath 2>$null)
        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($bump)) { return $null }
        $bump = $bump.Trim()
        $committed = @(& git diff --name-only "$bump..HEAD" -- @helperWatchPaths 2>$null)
        $uncommitted = @(& git diff --name-only HEAD -- @helperWatchPaths 2>$null)
        # Leading comma keeps an empty or single-item result an array instead of $null/a string.
        return , @(($committed + $uncommitted) | Where-Object { $_ } | Sort-Object -Unique)
    }
    finally {
        Pop-Location
    }
}

# Validate -HelperVersion up front
if (-not [string]::IsNullOrWhiteSpace($HelperVersion)) {
    $HelperVersion = ConvertTo-PlainVersion $HelperVersion
    if ($HelperVersion -notmatch '^\d+\.\d+\.\d+$') {
        Stop-Bump "Invalid helper version '$HelperVersion'." `
            "ADMIN_HELPER_VERSION is compared as major.minor.patch; pre-release or build suffixes are not supported." `
            "Pass a plain version such as -HelperVersion 2.10.0."
    }
}
else {
    $HelperVersion = $null
}

# Decides and stages the helper version. $appVersion is $null in helper-only mode.
function Update-Helper([string]$appVersion, [bool]$interactive) {
    $current = Get-CurrentHelperVersion
    $target = $null
    $reason = ''
    if ($HelperVersion) {
        $target = $HelperVersion; $reason = 'set with -HelperVersion'
    }
    elseif ($Helper -eq 'keep') {
        $reason = '-Helper keep'
    }
    elseif ($Helper -eq 'bump') {
        $target = $appVersion; $reason = '-Helper bump'
    }
    else {
        $changes = Get-HelperChanges
        if ($null -eq $changes) {
            $reason = 'no git history; kept (use -Helper bump to change it)'
        }
        elseif ($changes.Count -eq 0) {
            $reason = 'no helper code changed since its last bump'
        }
        else {
            Write-Host "  Helper code changed since $current" -ForegroundColor Yellow
            foreach ($f in $changes) { Write-Host "      $f" -ForegroundColor Yellow }
            $take = $true
            if ($interactive) {
                $answer = Read-Host "  Bump the helper to $appVersion? (Y/n)"
                $take = $answer -notmatch '^[nN]'
            }
            if ($take) { $target = $appVersion; $reason = "$($changes.Count) changed file(s)" }
            else { $reason = 'changed, but you chose to keep it' }
        }
    }

    if ($target -and $target -notmatch '^\d+\.\d+\.\d+$') {
        Stop-Bump "The helper cannot take version '$target'." `
            "The helper version is compared as major.minor.patch, so pre-release versions are not allowed." `
            "Pass a plain version, for example -HelperVersion $(($target -split '[-+]')[0]), or -Helper keep."
    }
    if ($target -and $target -eq $current) {
        $target = $null; $reason = 'already this version'
    }
    # Only an explicit -HelperVersion may lower it.
    if ($target -and -not $HelperVersion -and (Compare-CoreVersion $target $current) -lt 0) {
        $target = $null; $reason = "would go down from $current; kept (use -HelperVersion to force)"
    }

    if ($target) {
        Set-HelperVersion $target
        Write-Host "  Administrator helper  $current -> $target  ($reason)" -ForegroundColor Green
        Write-Host "  Note: a new helper version makes Settings offer a one-time elevated update. Mention it in the release notes." -ForegroundColor Yellow
    }
    else {
        Write-Host "  Administrator helper  $current  kept ($reason)" -ForegroundColor Gray
    }
}

function Invoke-LockCheck {
    if ($SkipLockCheck -or $DryRun) { return }
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoCmd) { return }
    Write-Host ""
    Write-Host "Verifying Cargo.lock matches the workspace (cargo metadata --locked)..." -ForegroundColor Gray
    Push-Location $repoRoot
    # Cargo prints progress ("Downloading crates") on stderr; under 'Stop',
    # Windows PowerShell 5.1 would turn that into a terminating error.
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $metaErrors = @(& cargo metadata --format-version 1 --locked 2>&1 | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] })
        $metaExit = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
        Pop-Location
    }
    if ($metaExit -ne 0) {
        Write-Diagnostic -Problem "Cargo.lock does not match the workspace after the update." `
            -Cause (($metaErrors | ForEach-Object { $_.ToString() }) -join "`n") `
            -Fix "The files above were written. Run 'cargo check --workspace' to inspect, or undo with 'git checkout -- .' and report the problem."
        exit 1
    }
    Write-Host "  [OK] Cargo.lock matches the workspace." -ForegroundColor Green
}

# 1. Read the current app version from Cargo.toml
$cargoTomlContent = Get-Text "Cargo.toml"
$cargoTomlMatch = [regex]::Match($cargoTomlContent, '(?m)^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"')
if (-not $cargoTomlMatch.Success) {
    Stop-Bump "Could not find [workspace.package] version in Cargo.toml." `
        "Cargo.toml does not contain a standard '[workspace.package]' block with 'version = ...'." `
        "Restore or format the [workspace.package] section in Cargo.toml."
}
$currentVersion = $cargoTomlMatch.Groups[1].Value

# Helper-only mode: -HelperVersion without an app version
if ([string]::IsNullOrWhiteSpace($Version) -and $HelperVersion) {
    Write-Host ""
    Write-Host "=== Administrator helper version only (app stays $currentVersion) ===" -ForegroundColor Cyan
    if ($DryRun) { Write-Host "(DRY RUN: no files will be written to disk)" -ForegroundColor Yellow }
    Write-Host ""
    Update-Helper $null $false
    Write-Host ""
    Write-Host "Files:" -ForegroundColor Cyan
    Write-Pending
    Invoke-LockCheck
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor Cyan
    Write-Host "  1. Run checks:          scripts\run-checks.bat"
    Write-Host "  2. Update specs:        specs/runtime.md log line"
    Write-Host "  3. Release notes:       mention that Settings will offer a one-time helper update"
    Write-Host ""
    exit 0
}

# 2. Acquire and validate the target app version
$versionPrompted = $false
if ([string]::IsNullOrWhiteSpace($Version)) {
    $versionPrompted = $true
    Write-Host ""
    Write-Host "=== OpenLocalServer Version Update ===" -ForegroundColor Cyan
    Write-Host "Current version: " -NoNewline
    Write-Host $currentVersion -ForegroundColor Yellow
    Write-Host ""
    $prompted = Read-Host "Enter new version (e.g. 2.3.0)"
    if ([string]::IsNullOrWhiteSpace($prompted)) {
        Write-Host "No version entered. Aborting." -ForegroundColor Gray
        exit 0
    }
    $Version = $prompted
}
$Version = ConvertTo-PlainVersion $Version

if (-not (Test-SemVer $Version)) {
    Stop-Bump "Invalid version format '$Version'." `
        "OLS follows Semantic Versioning (SemVer 2.0.0), expecting format like '2.3.0' or '2.3.0-rc.1'." `
        "Specify a valid semantic version (e.g. 2.3.0)."
}

if ($Version -eq $currentVersion -and -not $Force) {
    Write-Host ""
    Write-Host "Target version '$Version' is already the current version." -ForegroundColor Yellow
    $confirm = Read-Host "Re-apply update anyway? (y/N)"
    if ($confirm -notmatch '^[yY]') {
        Write-Host "Aborted." -ForegroundColor Gray
        exit 0
    }
}

$today = (Get-Date).ToString("yyyy-MM-dd")
$releaseDate = if ($Unreleased) { 'Unreleased' } else { $today }
$escapedVersion = [regex]::Escape($Version)
# A version as it appears inside other text (file names, footers): x.y.z[-pre]
$anyVersion = '\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?'

Write-Host ""
Write-Host "=== Bumping version: $currentVersion -> $Version ===" -ForegroundColor Cyan
if ($DryRun) {
    Write-Host "(DRY RUN: no files will be written to disk)" -ForegroundColor Yellow
}
Write-Host ""

# 3. Cargo.toml (workspace version: ols-core, olsc, ols-mcp and the desktop app)
$null = Edit-Text -Path "Cargo.toml" -What "[workspace.package] version" -Expect 1 `
    -Pattern '(?m)(^\[workspace\.package\][\s\S]*?^version\s*=\s*)"[^"]+"' `
    -Replacement ('${1}"' + $Version + '"')

# 4. src-tauri/tauri.conf.json (only the top-level "version", which comes first)
$null = Edit-Text -Path "src-tauri/tauri.conf.json" -What '"version"' -First `
    -Pattern '(?m)(^\s*"version"\s*:\s*)"[^"]+"' `
    -Replacement ('${1}"' + $Version + '"')

# 5. ui/package.json
$null = Edit-Text -Path "ui/package.json" -What '"version"' -First `
    -Pattern '(?m)(^\s*"version"\s*:\s*)"[^"]+"' `
    -Replacement ('${1}"' + $Version + '"')

# 6. ui/package-lock.json (root and packages[""])
if (Test-RepoFile "ui/package-lock.json") {
    $null = Edit-Text -Path "ui/package-lock.json" -What "the root version" -Expect 1 `
        -Pattern '(?m)(^{\s*"name":\s*"openlocalserver-ui",\r?\n\s*"version":\s*)"[^"]+"' `
        -Replacement ('${1}"' + $Version + '"')
    $null = Edit-Text -Path "ui/package-lock.json" -What 'the packages[""] version' -Expect 1 `
        -Pattern '(?m)("":\s*\{\s*"name":\s*"openlocalserver-ui",\r?\n\s*"version":\s*)"[^"]+"' `
        -Replacement ('${1}"' + $Version + '"')
}

# 7. Cargo.lock: the four packages on the workspace version (the helper is step 14)
$null = Edit-Text -Path "Cargo.lock" -What "the ols-cli, ols-core, ols-mcp and openlocalserver package entries" -Expect 4 `
    -Pattern '(?ms)(^\[\[package\]\]\r?\nname\s*=\s*"(?:ols-cli|ols-core|ols-mcp|openlocalserver)"\r?\nversion\s*=\s*)"[^"]+"' `
    -Replacement ('${1}"' + $Version + '"')

# 8. README.md: version badge and the installer name in the Download section
$null = Edit-Text -Path "README.md" -What "the version badge" -Expect 1 `
    -Pattern '\[!\[Version\]\(https:\/\/img\.shields\.io\/badge\/version-[^\)]+?-blue\)\]\(\.\/CHANGELOG\.md\)' `
    -Replacement "[![Version](https://img.shields.io/badge/version-$Version-blue)](./CHANGELOG.md)"
$null = Edit-Text -Path "README.md" -What "the installer name (OLS-x.y.z-setup.exe)" `
    -Pattern ('OLS-' + $anyVersion + '-setup\.exe') `
    -Replacement "OLS-$Version-setup.exe"

# 9. CHANGELOG.md
if (-not $NoChangelog) {
    $changelog = Get-Text "CHANGELOG.md"
    if ($changelog -notmatch "(?m)^##\s*\[$escapedVersion\]") {
        $entry = "## [Unreleased]`n`n## [$Version] $emDash $releaseDate`n`nFeature release. Full notes: [``release-notes/v$Version.md``](./release-notes/v$Version.md).`n"
        $null = Edit-Text -Path "CHANGELOG.md" -What "the '## [Unreleased]' heading" -First `
            -Pattern '(?m)^##\s*\[Unreleased\]\r?\n' -Replacement $entry.Replace('$', '$$')
    }
    elseif (-not $Unreleased) {
        # Release day: turn "## [x.y.z] — Unreleased" into today's date.
        $null = Edit-Text -Path "CHANGELOG.md" -What "the Unreleased date" -Optional `
            -Pattern ("(?m)^(##\s*\[$escapedVersion\]\s*$emDash\s*)Unreleased") `
            -Replacement ('${1}' + $today)
    }
}

# 10. release-notes/v<version>.md
if (-not $NoNotes) {
    $notesFile = "release-notes/v$Version.md"
    if (-not (Test-RepoFile $notesFile)) {
        $notes = @(
            "# OLS (Open Local Server) v$Version",
            "",
            "**Release date:** $releaseDate $midDot **Platform:** Windows 10/11 (64-bit) $midDot **License:** GPL-3.0-only",
            "",
            "Feature release.",
            "",
            "## Added",
            "- ",
            "",
            "## Changed & Improved",
            "- ",
            "",
            "## Fixed",
            "- ",
            "",
            "## Upgrading",
            "Download ``OLS-$Version-setup.exe`` and run the installer. In-place upgrade from earlier versions preserves existing configuration, sites, databases, secrets, services, and certificates.",
            ""
        ) -join "`n"
        Set-Text $notesFile $notes
    }
    else {
        # Older drafts named the installer OLS-v<version>-setup.exe; the real file has no "v".
        $null = Edit-Text -Path $notesFile -What "an OLS-v<version> installer name" -Optional `
            -Pattern ('OLS-v(' + $anyVersion + ')-setup\.exe') -Replacement 'OLS-${1}-setup.exe'
        if (-not $Unreleased) {
            $null = Edit-Text -Path $notesFile -What "the Unreleased date" -Optional `
                -Pattern '(\*\*Release date:\*\*\s*)Unreleased' -Replacement ('${1}' + $today)
        }
    }
}

# 11. ui/src/pages/Settings.tsx: release-notes import and the version fallbacks
$settingsPath = "ui/src/pages/Settings.tsx"
$null = Edit-Text -Path $settingsPath -What "the release-notes import" -Expect 1 `
    -Pattern "(release-notes/v)[^/'""]+?(\.md\?raw)" `
    -Replacement ('${1}' + $Version + '${2}')
$null = Edit-Text -Path $settingsPath -What "the version fallbacks (version || 'x.y.z')" `
    -Pattern "(version\s*\|\|\s*')[^']*(')" `
    -Replacement ('${1}' + $Version + '${2}')

# 12. User manuals (both copies): header, Table of Contents, Section 17 heading
$manuals = @("crates/ols-core/catalog/user_manual.md", "docs/OLS_USER_MANUAL.md")
# GitHub-style anchor of the version: lowercase, only letters, digits and '-'
$anchor = ($Version.ToLower() -replace '[^a-z0-9-]', '')
foreach ($manual in $manuals) {
    $null = Edit-Text -Path $manual -What "the '> **Version:**' header" -Expect 1 `
        -Pattern '(?m)^(>\s*\*\*Version:\*\*\s*)\S+' `
        -Replacement ('${1}' + $Version)
    $null = Edit-Text -Path $manual -What "the Section 17 Table of Contents line" -Expect 1 `
        -Pattern "(?m)^(17\.\s*\[Version )\S+( Specific Notes & What's New\]\(#17-version-)[0-9a-z-]*?(-specific-notes--whats-new\))" `
        -Replacement ('${1}' + $Version + '${2}' + $anchor + '${3}')
    $null = Edit-Text -Path $manual -What "the Section 17 heading" -Expect 1 `
        -Pattern "(?m)^(##\s*17\.\s*Version )\S+( Specific Notes & What's New)" `
        -Replacement ('${1}' + $Version + '${2}')
    if ((Get-Text $manual) -notmatch "(?m)^###\s*What's New in v$escapedVersion\b") {
        $devMark = if ($Unreleased) { ' (in development)' } else { '' }
        $null = Edit-Text -Path $manual -What "the Section 17 heading line" -Expect 1 `
            -Pattern "(?m)^(##\s*17\.\s*Version \S+ Specific Notes & What's New\r?\n)" `
            -Replacement ('${1}' + "`n### What's New in v$Version$devMark`n- `n")
    }
    # Older versions are released once a newer one is bumped; the current one is
    # released when the bump runs without -Unreleased.
    $null = Edit-Text -Path $manual -What "older '(in development)' marks" -Optional `
        -Pattern "(?m)^(###\s*What's New in v(?!$escapedVersion\b)\S+) \(in development\)" `
        -Replacement '${1}'
    if (-not $Unreleased) {
        $null = Edit-Text -Path $manual -What "the '(in development)' mark" -Optional `
            -Pattern "(?m)^(###\s*What's New in v$escapedVersion) \(in development\)" `
            -Replacement '${1}'
    }
}
if ((Get-Text $manuals[0]) -cne (Get-Text $manuals[1])) {
    Write-Host "[!] The two manuals are not identical after the update (AGENTS.md section 5 requires it)." -ForegroundColor Yellow
    Write-Host "    Compare them: git diff --no-index $($manuals[0]) $($manuals[1])" -ForegroundColor Yellow
    Write-Host ""
}

# 13. Landing page: download asset names, the version fact badge and the footer.
#     The release highlights are content and stay as they are.
$null = Edit-Text -Path "index.html" -What "the download asset names (OLS-x.y.z-setup.exe / portable zip / SHA256SUMS)" `
    -Pattern ('OLS-' + $anyVersion + '-(setup\.exe|portable-win-x64\.zip|SHA256SUMS\.txt)') `
    -Replacement ("OLS-$Version-" + '${1}')
$null = Edit-Text -Path "index.html" -What "the version fact badge (a line holding only vX.Y.Z)" -Expect 1 `
    -Pattern ('(?m)^(\s*)v' + $anyVersion + '(\r?)$') `
    -Replacement ('${1}v' + $Version + '${2}')
$null = Edit-Text -Path "index.html" -What "the footer version (<span>OLS x.y.z $midDot ...)" -Expect 1 `
    -Pattern ('(<span>OLS )' + $anyVersion + "( $midDot )") `
    -Replacement ('${1}' + $Version + '${2}')
if (Test-RepoFile "website/index.html") {
    $null = Edit-Text -Path "website/index.html" -What "download asset names" -Optional `
        -Pattern ('OLS-' + $anyVersion + '-(setup\.exe|portable-win-x64\.zip|SHA256SUMS\.txt)') `
        -Replacement ("OLS-$Version-" + '${1}')
}

# 14. Administrator helper (its own version)
Write-Host "Administrator helper version:" -ForegroundColor Cyan
$interactive = $versionPrompted -and -not $Force -and -not $DryRun
Update-Helper $Version $interactive

# 15. Write everything (every step above succeeded)
Write-Host ""
Write-Host "Files:" -ForegroundColor Cyan
Write-Pending

# 16. Lockfile integrity: --locked fails if Cargo.lock no longer matches the manifests.
Invoke-LockCheck

Write-Host ""
Write-Host "=== Version bump to $Version complete! ===" -ForegroundColor Green
Write-Host ""
Write-Host "Still to do by hand:" -ForegroundColor Cyan
Write-Host "  - release-notes/v$Version.md and CHANGELOG.md: the actual bullets"
Write-Host "  - Both manuals: the content of Section 17 ('What's New in v$Version')"
Write-Host "  - index.html: the release highlights section (it still describes the previous release)"
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Review changes:        git diff"
Write-Host "  2. Run quality gates:     scripts\run-checks.bat"
Write-Host "  3. Update specs:          specs/runtime.md log line (AGENTS.md section 1)"
Write-Host "  4. Commit:                git commit -am `"chore: bump version to $Version`""
Write-Host "  5. Tag and publish:       git tag v$Version"
Write-Host "                            scripts\build-installer.bat full"
Write-Host "                            scripts\upload-release.bat v$Version full"
Write-Host ""
