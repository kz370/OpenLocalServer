<#
.SYNOPSIS
Updates the OpenLocalServer project version across all files in the repository.

.DESCRIPTION
Synchronizes the version number across all repository manifests, configuration files,
lockfiles, documentation, changelog, and release notes:
  - Cargo.toml ([workspace.package] version)
  - src-tauri/tauri.conf.json (version)
  - ui/package.json (version)
  - ui/package-lock.json (version in root and packages[""])
  - Cargo.lock (ols-core, ols-helper, ols-cli, openlocalserver workspace packages)
  - README.md (version badge)
  - CHANGELOG.md (adds new version section if missing)
  - release-notes/v<version>.md (creates scaffold template if missing)

Validates the new version against Semantic Versioning rules, checks lockfile
integrity using cargo metadata, and reports all file updates cleanly.

.PARAMETER Version
The target version string (e.g. 2.3.0, v2.3.0, 3.0.0-rc.1).
If omitted, the script prompts interactively while showing the current version.

.PARAMETER DryRun
Preview changes without writing anything to disk.

.PARAMETER SkipLockCheck
Skip the cargo metadata lockfile verification check.

.PARAMETER NoChangelog
Do not append a new version entry to CHANGELOG.md.

.PARAMETER NoNotes
Do not generate a draft release-notes/v<version>.md template.

.EXAMPLE
scripts\update-version.bat 2.3.0
.\scripts\update-version.ps1 -Version 2.3.0
.\scripts\update-version.ps1 -Version 2.3.0 -DryRun
#>

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Version,

    [switch]$DryRun,
    [switch]$SkipLockCheck,
    [switch]$NoChangelog,
    [switch]$NoNotes,

    [Alias('h', '?')]
    [switch]$Help
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Help) {
    Write-Host ""
    Write-Host "Usage: update-version.bat [version] [options]" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Arguments:"
    Write-Host "  version              New target version (e.g. 2.3.0, v2.3.0)"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -DryRun              Preview file updates without modifying files"
    Write-Host "  -SkipLockCheck       Skip cargo metadata lockfile verification"
    Write-Host "  -NoChangelog         Do not add an entry to CHANGELOG.md"
    Write-Host "  -NoNotes             Do not generate release-notes/v<version>.md draft"
    Write-Host "  -Help, -h, -?        Show this help message"
    Write-Host ""
    exit 0
}

# Resolve repository root
$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path

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

# UTF-8 without BOM writer (reliable across PS 5.1 and PS 7+)
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Read-Utf8File([string]$relativePath) {
    $fullPath = Join-Path $repoRoot $relativePath
    if (-not (Test-Path $fullPath)) {
        Write-Diagnostic -Problem "Target file missing: $relativePath" `
            -Cause "The file does not exist at expected path '$fullPath'." `
            -Fix "Check out the missing file or verify repository integrity."
        exit 1
    }
    return [System.IO.File]::ReadAllText($fullPath, [System.Text.Encoding]::UTF8)
}

function Save-Utf8File([string]$relativePath, [string]$content) {
    $fullPath = Join-Path $repoRoot $relativePath
    if ($DryRun) {
        Write-Host "  [PREVIEW] $relativePath" -ForegroundColor Magenta
        return
    }
    [System.IO.File]::WriteAllText($fullPath, $content, $utf8NoBom)
    Write-Host "  [UPDATED] $relativePath" -ForegroundColor Green
}

# 1. Read current version from Cargo.toml
$cargoTomlContent = Read-Utf8File "Cargo.toml"
$cargoTomlMatch = [regex]::Match($cargoTomlContent, '(?m)^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"')
if (-not $cargoTomlMatch.Success) {
    Write-Diagnostic -Problem "Could not find [workspace.package] version in Cargo.toml." `
        -Cause "Cargo.toml does not contain a standard '[workspace.package]' block with 'version = ...'." `
        -Fix "Restore or format the [workspace.package] section in Cargo.toml."
    exit 1
}
$currentVersion = $cargoTomlMatch.Groups[1].Value

# 2. Acquire and validate target version
if ([string]::IsNullOrWhiteSpace($Version)) {
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
    $Version = $prompted.Trim()
} else {
    $Version = $Version.Trim()
}

# Normalize: strip leading 'v' or 'V'
if ($Version -match '^[vV](\d+.*)$') {
    $Version = $Matches[1]
}

# SemVer validation (major.minor.patch[-prerelease][+build])
$semverRegex = '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'
if ($Version -notmatch $semverRegex) {
    Write-Diagnostic -Problem "Invalid version format '$Version'." `
        -Cause "OLS follows Semantic Versioning (SemVer 2.0.0), expecting format like '2.3.0' or '2.3.0-rc.1'." `
        -Fix "Specify a valid semantic version (e.g. 2.3.0)."
    exit 1
}

if ($Version -eq $currentVersion) {
    Write-Host ""
    Write-Host "Target version '$Version' is already the current version." -ForegroundColor Yellow
    $confirm = Read-Host "Re-apply update anyway? (y/N)"
    if ($confirm -notmatch '^[yY]') {
        Write-Host "Aborted." -ForegroundColor Gray
        exit 0
    }
}

Write-Host ""
Write-Host "=== Bumping version: $currentVersion -> $Version ===" -ForegroundColor Cyan
if ($DryRun) {
    Write-Host "(DRY RUN: no files will be written to disk)" -ForegroundColor Yellow
}
Write-Host ""

# 3. Update Cargo.toml
$newCargoToml = [regex]::Replace(
    $cargoTomlContent,
    '(?m)(^\[workspace\.package\][\s\S]*?^version\s*=\s*)"[^"]+"',
    "`${1}`"$Version`""
)
Save-Utf8File "Cargo.toml" $newCargoToml

# 4. Update src-tauri/tauri.conf.json
$tauriConfContent = Read-Utf8File "src-tauri/tauri.conf.json"
$newTauriConf = [regex]::Replace(
    $tauriConfContent,
    '(?m)(^\s*"version"\s*:\s*)"[^"]+"',
    "`${1}`"$Version`""
)
Save-Utf8File "src-tauri/tauri.conf.json" $newTauriConf

# 5. Update ui/package.json
$packageJsonContent = Read-Utf8File "ui/package.json"
$newPackageJson = [regex]::Replace(
    $packageJsonContent,
    '(?m)(^\s*"version"\s*:\s*)"[^"]+"',
    "`${1}`"$Version`""
)
Save-Utf8File "ui/package.json" $newPackageJson

# 6. Update ui/package-lock.json (if present)
$packageLockPath = Join-Path $repoRoot "ui/package-lock.json"
if (Test-Path $packageLockPath) {
    $lockContent = [System.IO.File]::ReadAllText($packageLockPath, [System.Text.Encoding]::UTF8)
    $newLock = [regex]::Replace(
        $lockContent,
        '(?m)(^{\s*"name":\s*"openlocalserver-ui",\r?\n\s*"version":\s*)"[^"]+"',
        "`${1}`"$Version`""
    )
    $newLock = [regex]::Replace(
        $newLock,
        '(?m)("":\s*\{\s*"name":\s*"openlocalserver-ui",\r?\n\s*"version":\s*)"[^"]+"',
        "`${1}`"$Version`""
    )
    Save-Utf8File "ui/package-lock.json" $newLock
}

# 7. Update Cargo.lock (workspace members only: ols-cli, ols-core, ols-helper, openlocalserver)
$cargoLockContent = Read-Utf8File "Cargo.lock"
$targetPackages = "ols-cli|ols-core|ols-helper|openlocalserver"
$lockPattern = '(?ms)(^\[\[package\]\]\r?\nname\s*=\s*"(?:' + $targetPackages + ')"\r?\nversion\s*=\s*)"[^"]+"'
$newCargoLock = [regex]::Replace($cargoLockContent, $lockPattern, "`${1}`"$Version`"")
Save-Utf8File "Cargo.lock" $newCargoLock

# 8. Update README.md version badge
$readmeContent = Read-Utf8File "README.md"
$newReadme = [regex]::Replace(
    $readmeContent,
    '\[!\[Version\]\(https:\/\/img\.shields\.io\/badge\/version-[^-\)]+-blue\)\]\(\.\/CHANGELOG\.md\)',
    "[![Version](https://img.shields.io/badge/version-$Version-blue)](./CHANGELOG.md)"
)
Save-Utf8File "README.md" $newReadme

# 9. Update CHANGELOG.md
if (-not $NoChangelog) {
    $changelogContent = Read-Utf8File "CHANGELOG.md"
    if ($changelogContent -notmatch "(?m)^##\s*\[$([regex]::Escape($Version))\]") {
        $today = (Get-Date).ToString("yyyy-MM-dd")
        $changelogEntry = @"
## [Unreleased]

## [$Version] — $today

Feature release. Full notes: [`release-notes/v$Version.md`](./release-notes/v$Version.md).

"@
        $newChangelog = [regex]::Replace($changelogContent, '(?m)^##\s*\[Unreleased\]\r?\n', $changelogEntry, 1)
        Save-Utf8File "CHANGELOG.md" $newChangelog
    } else {
        Write-Host "  [UNCHANGED] CHANGELOG.md (entry for [$Version] already exists)" -ForegroundColor Gray
    }
}

# 10. Generate release-notes/v<version>.md draft
if (-not $NoNotes) {
    $notesFile = "release-notes/v$Version.md"
    $fullNotesPath = Join-Path $repoRoot $notesFile
    if (-not (Test-Path $fullNotesPath)) {
        $today = (Get-Date).ToString("yyyy-MM-dd")
        $notesContent = @"
# OLS (Open Local Server) v$Version

**Release date:** $today · **Platform:** Windows 10/11 (64-bit) · **License:** GPL-3.0-only

Feature release.

## Added
- 

## Changed & Improved
- 

## Fixed
- 

## Upgrading
Download ``OLS-v$Version-setup.exe`` and run the installer. In-place upgrade from earlier versions preserves existing configuration, sites, databases, secrets, services, and certificates.
"@
        Save-Utf8File $notesFile $notesContent
    } else {
        Write-Host "  [UNCHANGED] $notesFile (already exists)" -ForegroundColor Gray
    }
}

# 11. Lockfile integrity verification via cargo metadata
if (-not $SkipLockCheck -and -not $DryRun) {
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargoCmd) {
        Write-Host ""
        Write-Host "Verifying Cargo.lock consistency with workspace metadata..." -ForegroundColor Gray
        $metaOutput = & cargo metadata --format-version 1 --no-deps 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Diagnostic -Problem "Cargo metadata check failed after updating Cargo.lock." `
                -Cause ($metaOutput -join "`n") `
                -Fix "Run 'cargo check --workspace' manually to inspect the error."
            exit 1
        }
        Write-Host "  [OK] Cargo workspace metadata and lockfile verified." -ForegroundColor Green
    }
}

Write-Host ""
Write-Host "=== Version bump to $Version complete! ===" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Review changes:        git diff"
Write-Host "  2. Run quality gates:     cargo fmt --all"
Write-Host "                            cargo clippy -p ols-core -p ols-helper --all-targets"
Write-Host "                            cargo test -p ols-core -p ols-helper"
Write-Host "                            cd ui && npm run lint && npm run build"
Write-Host "  3. Update specs:          specs/runtime.md (log line + file counts) and specs/full_documentation.md"
Write-Host "  4. Commit:                git commit -am `"chore(release): bump version to $Version`""
Write-Host "  5. Tag and publish:       git tag v$Version"
Write-Host "                            scripts\build-installer.bat full"
Write-Host "                            scripts\upload-release.bat v$Version full"
Write-Host ""
