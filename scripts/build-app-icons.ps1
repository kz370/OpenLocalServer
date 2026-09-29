# Rebuilds every place the app shows its mark from the 1024 masters in
# src-tauri/icons/source. Four variants: light/dark theme × running/stopped.
#
#   pwsh -File scripts/build-app-icons.ps1
#
# Sources: assets/icon.webp (running, light), assets/stop-icon.webp (stopped,
# light), assets/icon-dark.webp (running, dark). The stopped-dark twin is derived
# from the dark running master by rotating its hue onto the light red's hue, so
# the two pairs stay one artwork in two colourways.
#
# Everything downstream is generated, never hand-edited: the tauri bundle set, the
# tray/window sizes, and the self-contained SVGs the UI and the welcome page read.
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root 'src-tauri/icons/source'
$icons = Join-Path $root 'src-tauri/icons'
$public = Join-Path $root 'ui/public'

# Hue of the light "stopped" artwork. The dark twin is rotated onto it rather than
# picking a red by eye, so a future repaint of the light pair keeps both pairs in
# step: re-run this and the dark red follows.
$STOPPED_HUE_SHIFT = 18

function Convert-ToMaster([string]$Webp, [string]$Master) {
  $size = magick identify -format '%wx%h' $Webp
  magick $Webp -background none -resize '1024x1024' -gravity center -extent 1024x1024 -strip "PNG32:$Master"
  if ($LASTEXITCODE -ne 0) { throw "magick failed for $Webp" }
  Write-Output "master  $Master (from $size webp)"
}

function Write-Sizes([string]$Master, [string]$Dir, [string]$Label) {
  New-Item -ItemType Directory -Force -Path $Dir | Out-Null
  foreach ($size in 32, 64, 128) {
    $out = Join-Path $Dir "${size}x${size}.png"
    magick $Master -resize "${size}x${size}" -strip "PNG32:$out"
    if ($LASTEXITCODE -ne 0) { throw "magick failed for $Label ${size}x${size}" }
  }
  Write-Output "sizes   $Dir ($Label)"
}

function Write-SvgMark([string]$Png, [string]$Svg, [string[]]$Comment) {
  $b64 = [Convert]::ToBase64String([System.IO.File]::ReadAllBytes($Png))
  $lines = @(
    '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 128 128" width="128" height="128" role="img" aria-label="Open Local Server">'
    ($Comment | ForEach-Object { "  <!-- $_ -->" })
    "  <image x=`"0`" y=`"0`" width=`"128`" height=`"128`" preserveAspectRatio=`"xMidYMid meet`" xlink:href=`"data:image/png;base64,$b64`" />"
    '</svg>'
  )
  [System.IO.File]::WriteAllText($Svg, ($lines -join "`n") + "`n")
  Write-Output "svg     $Svg"
}

Convert-ToMaster (Join-Path $root 'assets/icon.webp') (Join-Path $src 'icon-light-running.png')
Convert-ToMaster (Join-Path $root 'assets/stop-icon.webp') (Join-Path $src 'icon-light-stopped.png')
Convert-ToMaster (Join-Path $root 'assets/icon-dark.webp') (Join-Path $src 'icon-dark-running.png')
magick (Join-Path $src 'icon-dark-running.png') -modulate "100,100,$STOPPED_HUE_SHIFT" -colorspace sRGB -strip "PNG32:$(Join-Path $src 'icon-dark-stopped.png')"
if ($LASTEXITCODE -ne 0) { throw 'magick failed for the dark stopped master' }
Write-Output "master  $(Join-Path $src 'icon-dark-stopped.png') (hue-shifted from the dark running master)"

Push-Location (Join-Path $root 'src-tauri')
cargo tauri icon icons/source/icon-light-running.png | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'cargo tauri icon failed' }
Pop-Location
Write-Output 'bundle  src-tauri/icons (light running)'

Write-Sizes (Join-Path $src 'icon-light-stopped.png') (Join-Path $icons 'red') 'light stopped (services stopped)'
Write-Sizes (Join-Path $src 'icon-dark-running.png') (Join-Path $icons 'dark') 'dark running'
Write-Sizes (Join-Path $src 'icon-dark-stopped.png') (Join-Path $icons 'dark/red') 'dark stopped (services stopped)'

Write-SvgMark (Join-Path $icons '128x128.png') (Join-Path $public 'favicon.svg') @(
  'Official mark, light theme: the green server-and-globe app icon from',
  'src-tauri/icons/128x128.png, embedded so this file stays self-contained',
  'wherever it is copied. Regenerate with scripts/build-app-icons.ps1.'
)
Write-SvgMark (Join-Path $icons 'red/128x128.png') (Join-Path $public 'favicon-stopped.svg') @(
  'Stopped twin of favicon.svg: the same mark with nothing running.'
)
Write-SvgMark (Join-Path $icons 'dark/128x128.png') (Join-Path $public 'favicon-dark.svg') @(
  'Dark-theme twin of favicon.svg: the same mark recoloured for a dark UI.'
)
Write-SvgMark (Join-Path $icons 'dark/red/128x128.png') (Join-Path $public 'favicon-stopped-dark.svg') @(
  'Dark-theme twin of favicon-stopped.svg.'
)
