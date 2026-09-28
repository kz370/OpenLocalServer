<#
.SYNOPSIS
Builds the Tauri frontend (ui/dist) with a content-hash cache.

.DESCRIPTION
Hashes every file the UI build depends on (ui/src, ui/public, index.html,
vite.config.ts, package.json, package-lock.json, tsconfig*.json, .oxlintrc.json)
plus a CACHE_VERSION constant. The digest is stored in ui/.build-cache/ui.stamp.

  digest matches + ui/dist/index.html exists -> skip npm install and npm run
  build entirely and reuse the cached ui/dist.
  anything else (first run, an edited source, a changed dependency, a missing
  or emptied dist) -> npm install (only when node_modules is missing) then
  npm run build, then write the new stamp.

A failure to hash, install, or build is never silent: the script writes a
Diagnostic (problem / cause / fix) to stderr and exits non-zero.

.PARAMETER Force
Ignore the cached stamp and rebuild even when nothing changed.

.PARAMETER UiDir
Path to the ui/ directory. Defaults to <repo>/ui, resolved from this script.

.PARAMETER Quiet
Suppress the "reusing cached build" line.
#>
[CmdletBinding()]
param(
  [switch] $Force,
  [string] $UiDir,
  [switch] $Quiet
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Bump when the caching rules themselves change, so every developer rebuilds
# once instead of reusing a dist built under different rules.
$CACHE_VERSION = '1'

$script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
if (-not $UiDir) { $UiDir = Join-Path $script:RepoRoot 'ui' }

$DistDir = Join-Path $UiDir 'dist'
$CacheDir = Join-Path $UiDir '.build-cache'
$StampFile = Join-Path $CacheDir 'ui.stamp'

function Fail([string] $Problem, [string] $Cause, [string] $Fix) {
  [Console]::Error.WriteLine("[x] Problem: $Problem")
  [Console]::Error.WriteLine("    Cause:  $Cause")
  [Console]::Error.WriteLine("    Fix:    $Fix")
  exit 1
}

# Build inputs, relative to ui/. Any change here must be mirrored in
# $CACHE_VERSION so stale dists are never reused.
$InputDirs = @('src', 'public')
$InputFiles = @(
  'index.html',
  'vite.config.ts',
  'package.json',
  'package-lock.json',
  'tsconfig.json',
  'tsconfig.app.json',
  'tsconfig.node.json',
  '.oxlintrc.json'
)

function Get-UiDigest {
  $sha = [System.Security.Cryptography.SHA256]::Create()
  $builder = New-Object System.Text.StringBuilder
  [void] $builder.Append("version=$CACHE_VERSION`n")

  $files = New-Object System.Collections.Generic.List[System.IO.FileInfo]

  foreach ($dir in $InputDirs) {
    $dirPath = Join-Path $UiDir $dir
    if (-not (Test-Path -LiteralPath $dirPath)) {
      Fail "ui\$dir is missing." 'Incomplete ui checkout.' 'Run from a full repo checkout.'
    }
    Get-ChildItem -LiteralPath $dirPath -Recurse -File -Force |
      ForEach-Object { $files.Add($_) }
  }

  foreach ($name in $InputFiles) {
    $path = Join-Path $UiDir $name
    if (Test-Path -LiteralPath $path) { $files.Add((Get-Item -LiteralPath $path)) }
  }

  if ($files.Count -eq 0) {
    Fail 'No UI build inputs found.' 'ui/src and ui/public are both empty or missing.' 'Run from a full repo checkout.'
  }

  # Sort by repo-relative path so the digest is stable across machines.
  $ordered = $files | Sort-Object { $_.FullName.Substring($UiDir.Length).Replace('\', '/').ToLowerInvariant() }

  foreach ($file in $ordered) {
    $rel = $file.FullName.Substring($UiDir.Length).Replace('\', '/')
    [void] $builder.Append($rel).Append("`n")
    $stream = [System.IO.File]::OpenRead($file.FullName)
    try {
      [void] $builder.Append(([System.BitConverter]::ToString($sha.ComputeHash($stream)))).Append("`n")
    } finally {
      $stream.Dispose()
    }
  }

  $digestBytes = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($builder.ToString()))
  $sha.Dispose()
  return [System.BitConverter]::ToString($digestBytes)
}

$digest = Get-UiDigest

$cached = $null
if (Test-Path -LiteralPath $StampFile) {
  $cached = (Get-Content -LiteralPath $StampFile -Raw).Trim()
}

$distEntry = Join-Path $DistDir 'index.html'
$cacheValid = ($cached -eq $digest) -and (Test-Path -LiteralPath $distEntry)

if ($cacheValid -and -not $Force) {
  if (-not $Quiet) { Write-Host '       frontend: reusing cached ui\dist (sources unchanged).' }
  exit 0
}

if ($cacheValid -and $Force) {
  Write-Host '       frontend: cache hit but -Force given - rebuilding.'
}

# node_modules only matters when we are about to build; npm install itself is
# left to package-lock.json so a cached dist never pays for it.
$nodeModules = Join-Path $UiDir 'node_modules'
if (-not (Test-Path -LiteralPath $nodeModules)) {
  Write-Host '       frontend: installing UI dependencies...'
  Push-Location $UiDir
  try {
    & npm.cmd install
    if ($LASTEXITCODE -ne 0) { Fail 'npm install failed.' 'See the npm output above.' 'Delete ui\node_modules and run this again.' }
  } finally {
    Pop-Location
  }
}

Push-Location $UiDir
try {
  & npm.cmd run build
  if ($LASTEXITCODE -ne 0) { Fail 'Frontend build failed.' 'See the tsc/vite output above.' 'Fix the reported error and run this again.' }
} finally {
  Pop-Location
}

if (-not (Test-Path -LiteralPath $distEntry)) {
  Fail 'ui\dist\index.html missing after build.' 'The UI build produced no entry point.' 'Delete ui\dist and run this again.'
}

if (-not (Test-Path -LiteralPath $CacheDir)) { New-Item -ItemType Directory -Path $CacheDir -Force | Out-Null }
# UTF8 without a BOM: the stamp is compared as raw text, and a BOM would make
# every later run miss the cache.
[System.IO.File]::WriteAllText($StampFile, "$digest`n", (New-Object System.Text.UTF8Encoding($false)))

exit 0
