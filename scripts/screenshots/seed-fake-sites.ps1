<#
  Writes (or with -Remove deletes) the fake sites from fake-sites.json.

  Each fake site is a static-HTML folder in <repo>\data\fake_sites with its own favicon.svg
  (OLS shows a discovered favicon as the project and site icon). The script only writes files:
  it never touches the app. Add the folders in OLS yourself (Sites -> Add a folder).

  Screenshot workflow
    1. Run this script, then add the folders in the app.
    2. Hide/disable your real sites and projects so only the fake ones are visible.
    3. Take the screenshots, then bring your real sites back and remove the fake ones.
    4. Run this script with -Remove to delete the fake folders.

  Only folders named in fake-sites.json and inside data\fake_sites are ever created or deleted.
#>
param(
  [string]$FakeDir = '',
  [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $FakeDir) { $FakeDir = Join-Path $Repo 'data\fake_sites' }
New-Item -ItemType Directory -Force $FakeDir | Out-Null
$FakeDir = (Resolve-Path $FakeDir).Path
$fixtures = Get-Content (Join-Path $PSScriptRoot 'fake-sites.json') -Raw | ConvertFrom-Json

# 24x24 stroke glyphs drawn for this script
$glyphs = @{
  cart      = '<path d="M3 4h2l2.4 10.2a1 1 0 0 0 1 .8h8.2a1 1 0 0 0 1-.8L19 8H6"/><circle cx="9.5" cy="19" r="1.4"/><circle cx="17" cy="19" r="1.4"/>'
  book      = '<path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5z"/><path d="M5 19.5A1.5 1.5 0 0 0 6.5 21H19v-3"/>'
  briefcase = '<rect x="3" y="7" width="18" height="13" rx="2"/><path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2M3 13h18"/>'
  rocket    = '<path d="M12 3c3 2 5 5 5 9l-2 3H9l-2-3c0-4 2-7 5-9z"/><circle cx="12" cy="9.5" r="1.6"/><path d="M9 15l-2 4 3-1.5M15 15l2 4-3-1.5"/>'
  pen       = '<path d="M4 20l1-4L16.5 4.5a2 2 0 0 1 3 3L8 19z"/><path d="M14 7l3 3"/>'
}

foreach ($s in $fixtures.sites) {
  $dir = Join-Path $FakeDir $s.name
  if ($Remove) {
    if ((Test-Path $dir) -and ((Resolve-Path $dir).Path.StartsWith($FakeDir + '\'))) {
      Remove-Item $dir -Recurse -Force
      Write-Host "removed $dir"
    }
    continue
  }
  $glyph = $glyphs[$s.glyph]
  if (-not $glyph) { throw "Unknown glyph '$($s.glyph)' for $($s.name)." }
  New-Item -ItemType Directory -Force $dir | Out-Null
  "<!doctype html><meta charset=utf-8><title>$($s.title)</title><link rel=icon href=favicon.svg><h1>$($s.title)</h1>" |
    Set-Content (Join-Path $dir 'index.html') -Encoding utf8
  @"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="7" fill="$($s.color)"/><g transform="translate(4 4)" fill="none" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">$glyph</g></svg>
"@ | Set-Content (Join-Path $dir 'favicon.svg') -Encoding utf8
  Write-Host "added   $dir"
}
