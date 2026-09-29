# Rebuilds every place the app shows its mark from the 1024 masters in
# src-tauri/icons/source. Four variants: light/dark theme × running/stopped.
#
#   pwsh -File scripts/build-app-icons.ps1
#
# Sources: assets/icon.webp (running, light), assets/stop-icon.webp (stopped,
# light), assets/icon-dark.webp (running, dark). The stopped-dark twin is derived
# from the dark running master by rotating its plate hue onto the light red's
# measured hue, so the two pairs stay one artwork in two colourways.
#
# Everything downstream is generated, never hand-edited: the tauri bundle set, the
# tray/window sizes, and the self-contained SVGs the UI and the welcome page read.
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root 'src-tauri/icons/source'
$icons = Join-Path $root 'src-tauri/icons'
$public = Join-Path $root 'ui/public'

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

function Get-PlateHue([string]$Png) {
  # The plate is the single dominant colour of the mark, so the modal
  # quantised swatch is the plate and nothing else. Sampling a fixed corner
  # instead would break the moment a future repaint moves the artwork.
  $line = magick $Png -colors 4 -format %c histogram:info:- |
    Sort-Object { [int](($_ -split ':')[0]) } -Descending |
    Select-Object -First 1
  if ($line -notmatch 'srgba\(([\d.]+)%,([\d.]+)%,([\d.]+)%') { throw "could not read plate colour from $Png" }
  $r = [double]$Matches[1] * 255 / 100
  $g = [double]$Matches[2] * 255 / 100
  $b = [double]$Matches[3] * 255 / 100
  $max = [Math]::Max($r, [Math]::Max($g, $b))
  $min = [Math]::Min($r, [Math]::Min($g, $b))
  if ($max -eq $min) { return 0.0 }
  $d = $max - $min
  if ($max -eq $g) { $h = ($b - $r) / $d + 2.0 }
  elseif ($max -eq $r) { $h = ($g - $b) / $d } # can be negative; the modulo below wraps it
  else { $h = ($r - $g) / $d + 4.0 }
  $h = (($h % 6.0) + 6.0) % 6.0
  return $h * 60.0
}

Convert-ToMaster (Join-Path $root 'assets/icon.webp') (Join-Path $src 'icon-light-running.png')
Convert-ToMaster (Join-Path $root 'assets/stop-icon.webp') (Join-Path $src 'icon-light-stopped.png')
Convert-ToMaster (Join-Path $root 'assets/icon-dark.webp') (Join-Path $src 'icon-dark-running.png')

# The dark stopped twin is the dark running master rotated onto the light red's
# hue, measured rather than hardcoded, so a repaint of either pair keeps both in
# step. `magick -modulate` takes hue as a percentage where 100 is no change and
# 200 is a full turn, so one unit is 360/200 = 1.8 degrees.
$targetHue = Get-PlateHue (Join-Path $src 'icon-light-stopped.png')
$sourceHue = Get-PlateHue (Join-Path $src 'icon-dark-running.png')
$delta = (($targetHue - $sourceHue) % 360.0 + 360.0) % 360.0
$stoppedHueShift = 100.0 + $delta / 1.8
Write-Output ("hue     dark running {0:N1} deg -> light stopped {1:N1} deg (modulate {2:N2})" -f $sourceHue, $targetHue, $stoppedHueShift)
magick (Join-Path $src 'icon-dark-running.png') -modulate "100,100,$stoppedHueShift" -colorspace sRGB -strip "PNG32:$(Join-Path $src 'icon-dark-stopped.png')"
if ($LASTEXITCODE -ne 0) { throw 'magick failed for the dark stopped master' }
Write-Output "master  $(Join-Path $src 'icon-dark-stopped.png') (hue-shifted from the dark running master onto the light red)"

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
