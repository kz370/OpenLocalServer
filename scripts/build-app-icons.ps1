# Rebuilds every place the app shows its mark from the three sources in assets/.
# Four variants: light/dark theme x running/stopped.
#
#   scripts\build-app-icons.bat
#   pwsh -File scripts/build-app-icons.ps1 [-Tool auto|magick|ffmpeg]
#
#   Sources: assets/icon.webp (running, light), assets/stop-icon.webp (stopped,
# light), assets/icon-dark.webp (running, dark). The stopped-dark twin is derived
# from the dark running master by rotating its plate hue onto the light red's
# measured hue, so the two pairs stay one artwork in two colourways.
#
# Everything downstream is generated, never hand-edited: the tauri bundle set, the
# tray/window sizes, the dark .ico the Start Menu shortcut and the taskbar button
# read, and the self-contained SVGs the UI and the welcome page read.
#
# Raster work goes through whichever of ImageMagick or ffmpeg is installed;
# `-Tool` pins one. Both backends read the plate the same way (a circular mean
# over the saturated pixels of a 96x96 downscale) and were within 0.1 degrees of
# each other on the same master, so a machine without ImageMagick produces the
# same artwork as one with it.
[CmdletBinding()]
param(
  [ValidateSet('auto', 'magick', 'ffmpeg')]
  [string] $Tool = 'auto'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root 'src-tauri/icons/source'
$icons = Join-Path $root 'src-tauri/icons'
$public = Join-Path $root 'ui/public'

function Fail([string] $Problem, [string] $Cause, [string] $Fix) {
  [Console]::Error.WriteLine("[x] Problem: $Problem")
  [Console]::Error.WriteLine("    Cause:  $Cause")
  [Console]::Error.WriteLine("    Fix:    $Fix")
  exit 1
}

function Test-Tool([string] $Name) {
  $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

if ($Tool -eq 'auto') {
  # ImageMagick first: it is the better-documented of the two for still-image
  # work, and the repo has used it since the first icon build.
  if (Test-Tool 'magick') { $Tool = 'magick' }
  elseif (Test-Tool 'ffmpeg') { $Tool = 'ffmpeg' }
  else {
    Fail 'No image tool found.' 'Neither magick nor ffmpeg is on PATH.' 'Install ImageMagick (https://imagemagick.org) or ffmpeg (https://ffmpeg.org) and run this again.'
  }
}
elseif (-not (Test-Tool $Tool)) {
  Fail "$Tool not found." 'It is not on PATH, but -Tool pinned it.' 'Install it, or drop -Tool to auto-detect.'
}
Write-Output "raster  $Tool"

# Scratch space for the raw pixel dumps Get-PlateHue reads.
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) "ols-icon-$PID"
New-Item -ItemType Directory -Force -Path $scratch | Out-Null

function Invoke-Raster([string[]] $Argv) {
  & $Argv[0] $Argv[1..($Argv.Length - 1)]
  if ($LASTEXITCODE -ne 0) { throw "$($Argv[0]) failed: $($Argv -join ' ')" }
}

# Scale the shorter side to 1024 and centre the remainder on a transparent
# canvas, so a non-square source still yields an exactly 1024x1024 master.
function Convert-ToMaster([string] $Webp, [string] $Master) {
  if ($Tool -eq 'magick') {
    magick $Webp -background none -resize '1024x1024' -gravity center -extent 1024x1024 -strip "PNG32:$Master"
    if ($LASTEXITCODE -ne 0) { throw "magick failed for $Webp" }
  }
  else {
    Invoke-Raster @('ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', $Webp,
      '-vf', 'scale=1024:1024:force_original_aspect_ratio=decrease,pad=1024:1024:(ow-iw)/2:(oh-ih)/2:color=0x00000000,format=rgba',
      '-frames:v', '1', '-y', $Master)
  }
  $size = (Get-Item -LiteralPath $Master).Length
  Write-Output "master  $([System.IO.Path]::GetFileName($Master)) ($Webp -> 1024x1024, $size bytes)"
}

function Write-Sizes([string] $Master, [string] $Dir, [string] $Label) {
  New-Item -ItemType Directory -Force -Path $Dir | Out-Null
  foreach ($size in 32, 64, 128) {
    $out = Join-Path $Dir "${size}x${size}.png"
    if ($Tool -eq 'magick') {
      magick $Master -resize "${size}x${size}" -strip "PNG32:$out"
      if ($LASTEXITCODE -ne 0) { throw "magick failed for $Label ${size}x${size}" }
    }
    else {
      Invoke-Raster @('ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', $Master,
        '-vf', "scale=${size}:${size}:flags=lanczos,format=rgba", '-frames:v', '1', '-y', $out)
    }
  }
  Write-Output "sizes   $Dir ($Label)"
}

function Write-SvgMark([string] $Png, [string] $Svg, [string[]] $Comment) {
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

# The dominant hue of the coloured plate, in degrees 0-360.
#
# A modal colour is the wrong tool here: the plate is a gradient, so no single
# colour is dominant, and the white "OLS" glyph is one large flat region that
# wins any histogram. Instead this averages the hue *angle* over every pixel
# that is opaque and has real saturation. An arithmetic mean of degrees would
# average 350 and 10 into 180 and turn red into cyan, which is why it is a
# circular mean. Hue is angle-invariant, so the plate's shading washes out and
# only its colour survives.
function Get-PlateHue([string] $Png) {
  $raw = Join-Path $scratch 'plate.rgba'
  if ($Tool -eq 'magick') {
    magick $Png -resize '96x96!' -depth 8 "rgba:$raw"
    if ($LASTEXITCODE -ne 0) { throw "magick failed to dump pixels for $Png" }
  }
  else {
    Invoke-Raster @('ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', $Png,
      '-vf', 'scale=96:96:flags=area,format=rgba', '-f', 'rawvideo', '-y', $raw)
  }
  $px = [System.IO.File]::ReadAllBytes($raw)
  if ($px.Length -lt 96 * 96 * 4) { throw "short pixel dump for $Png ($($px.Length) bytes)" }

  $cos = 0.0
  $sin = 0.0
  $n = 0
  for ($i = 0; $i -lt 96 * 96; $i++) {
    if ($px[$i * 4 + 3] -lt 200) { continue }
    $r = [double]$px[$i * 4]
    $g = [double]$px[$i * 4 + 1]
    $b = [double]$px[$i * 4 + 2]
    $max = [Math]::Max($r, [Math]::Max($g, $b))
    $min = [Math]::Min($r, [Math]::Min($g, $b))
    if (($max - $min) -lt 40) { continue }
    $d = $max - $min
    if ($max -eq $g) { $h = ($b - $r) / $d + 2.0 }
    elseif ($max -eq $r) { $h = ($g - $b) / $d } # negative; the wrap below handles it
    else { $h = ($r - $g) / $d + 4.0 }
    $h = (($h % 6.0) + 6.0) % 6.0
    $a = [Math]::PI / 3.0 * $h # sixths of a turn -> radians
    $cos += [Math]::Cos($a)
    $sin += [Math]::Sin($a)
    $n++
  }
  if ($n -eq 0) { throw "no saturated pixels in $Png - the plate is transparent or greyscale" }
  return ([Math]::Atan2($sin, $cos) * 180.0 / [Math]::PI + 360.0) % 360.0
}

Convert-ToMaster (Join-Path $root 'assets/icon.webp') (Join-Path $src 'icon-light-running.png')
Convert-ToMaster (Join-Path $root 'assets/stop-icon.webp') (Join-Path $src 'icon-light-stopped.png')
Convert-ToMaster (Join-Path $root 'assets/icon-dark.webp') (Join-Path $src 'icon-dark-running.png')

# Rotate the dark running master onto the light red's hue, measured rather than
# hardcoded, so a repaint of either pair keeps both in step. The two backends
# spell the rotation differently: ImageMagick's -modulate takes a percentage of
# a turn (100 = no change, 200 = 360 degrees, so 1 unit = 1.8 degrees) while
# ffmpeg's hue filter takes degrees outright.
$targetHue = Get-PlateHue (Join-Path $src 'icon-light-stopped.png')
$sourceHue = Get-PlateHue (Join-Path $src 'icon-dark-running.png')
$delta = (($targetHue - $sourceHue) % 360.0 + 360.0) % 360.0
$stoppedMaster = Join-Path $src 'icon-dark-stopped.png'
if ($Tool -eq 'magick') {
  Invoke-Raster @('magick', (Join-Path $src 'icon-dark-running.png'),
    '-modulate', ('100,100,{0:N4}' -f (100.0 + $delta / 1.8)),
    '-colorspace', 'sRGB', '-strip', "PNG32:$stoppedMaster")
}
else {
  Invoke-Raster @('ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', (Join-Path $src 'icon-dark-running.png'),
    '-vf', ('hue=h={0:N4}' -f $delta), '-frames:v', '1', '-y', $stoppedMaster)
}
Write-Output ("hue     dark running {0:N1} deg -> light stopped {1:N1} deg (rotate {2:N1} deg)" -f $sourceHue, $targetHue, $delta)
Write-Output "master  icon-dark-stopped.png (hue-shifted from the dark running master onto the light red)"

Push-Location (Join-Path $root 'src-tauri')
try {
  cargo tauri icon icons/source/icon-light-running.png | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'cargo tauri icon failed' }
} finally {
  Pop-Location
}
Write-Output 'bundle  src-tauri/icons (light running)'

Write-Sizes (Join-Path $src 'icon-light-stopped.png') (Join-Path $icons 'red') 'light stopped (services stopped)'
Write-Sizes (Join-Path $src 'icon-dark-running.png') (Join-Path $icons 'dark') 'dark running'
Write-Sizes (Join-Path $src 'icon-dark-stopped.png') (Join-Path $icons 'dark/red') 'dark stopped (services stopped)'

# The dark colourway also needs a real .ico, and not as a bundle: the Start Menu shortcut
# points its own IconLocation at it, and the taskbar button takes the taskbar mark from
# that shortcut rather than from the window (see the shortcut section of src-tauri/src/
# notify.rs). `IShellLink::SetIconLocation` wants an icon *resource*, so a .png there
# leaves the button blank — this has to be a genuine multi-size .ico.
#
# 128 down to 16 rather than the 256 the bundle set carries: a shortcut is never drawn
# larger than a Start Menu tile, and the entry sizes dominate the file.
$darkIco = Join-Path $icons 'dark/icon.ico'
if ($Tool -eq 'magick') {
  magick (Join-Path $src 'icon-dark-running.png') -define icon:auto-resize=128,64,48,32,24,16 -strip $darkIco
  if ($LASTEXITCODE -ne 0) { throw "magick failed for the dark .ico" }
}
else {
  # ffmpeg writes no .ico, so the sizes are emitted one by one and packed by magick if it
  # happens to be present; without it the dark .ico is left for a machine that has it,
  # and the shortcut falls back to the executable's own (light) icon rather than breaking.
  if (Test-Tool 'magick') {
    magick (Join-Path $src 'icon-dark-running.png') -define icon:auto-resize=128,64,48,32,24,16 -strip $darkIco
    if ($LASTEXITCODE -ne 0) { throw "magick failed for the dark .ico" }
  }
  else {
    Write-Warning 'no ImageMagick: icons/dark/icon.ico not rebuilt, so the taskbar keeps the executable icon'
  }
}
if (Test-Path $darkIco) {
  Write-Output "ico     $darkIco ($((Get-Item -LiteralPath $darkIco).Length) bytes, 16-128px, the shortcut/taskbar mark)"
}

Write-SvgMark (Join-Path $icons '128x128.png') (Join-Path $public 'favicon.svg') @(
  'Official mark, light theme: the green server-and-globe app icon from',
  'src-tauri/icons/128x128.png, embedded so this file stays self-contained',
  'wherever it is copied. Regenerate with scripts/build-app-icons.bat.'
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

Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
