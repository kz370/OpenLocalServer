# Rebuilds every place the app shows its mark from the two 1024 masters in
# src-tauri/icons/source (green = running, red = services stopped).
#
#   pwsh -File scripts/build-app-icons.ps1
#
# The masters come from assets/icon.webp (running) and assets/stop-icon.webp
# (stopped). Everything downstream is generated, never hand-edited: the tauri
# bundle set, the tray/window sizes, and the self-contained SVGs the UI and the
# welcome page read.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

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

function Write-SvgMark([string]$Png, [string]$Svg, [string]$Label, [string[]]$Comment) {
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

Convert-ToMaster (Join-Path $root 'assets/icon.webp') (Join-Path $src 'icon-green.png')
Convert-ToMaster (Join-Path $root 'assets/stop-icon.webp') (Join-Path $src 'icon-red.png')

Push-Location (Join-Path $root 'src-tauri')
cargo tauri icon icons/source/icon-green.png | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'cargo tauri icon failed' }
Pop-Location
Write-Output 'bundle  src-tauri/icons (green)'

foreach ($size in 32, 64, 128) {
  magick (Join-Path $src 'icon-red.png') -resize "${size}x${size}" -strip "PNG32:$(Join-Path $icons "red/${size}x${size}.png")"
  if ($LASTEXITCODE -ne 0) { throw "magick failed for red ${size}x${size}" }
}
Write-Output 'stopped src-tauri/icons/red'

Write-SvgMark (Join-Path $icons '128x128.png') (Join-Path $public 'favicon.svg') 'running' @(
  'Official mark: the green server-and-globe app icon from src-tauri/icons/128x128.png,',
  'embedded so this file stays self-contained wherever it is copied.',
  'Regenerate with scripts/build-app-icons.ps1.'
)
Write-SvgMark (Join-Path $icons 'red/128x128.png') (Join-Path $public 'favicon-stopped.svg') 'stopped' @(
  'Red twin of favicon.svg: the same mark with nothing running. Kept in sync with',
  'src-tauri/icons/red/128x128.png, which the tray and taskbar use.'
)
