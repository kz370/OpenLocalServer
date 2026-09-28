# Normalize an AI-generated 1254px app-icon source into a clean 1024px master:
# crop the black matte, resize, and knock the rounded-square corners out to alpha 0
# so the taskbar, tray and window icons have no black frame.
#
#   pwsh -File scripts/prepare-icon-source.ps1 -SourcePath assets/new-icon.png -TargetPath src-tauri/icons/source/icon-green.png
param(
  [Parameter(Mandatory = $true)][string]$SourcePath,
  [Parameter(Mandatory = $true)][string]$TargetPath,
  [int]$Size = 1024,
  [double]$CornerRadius = 0.225
)

Add-Type -AssemblyName System.Drawing
# PowerShell 7 ships System.Drawing.Common as a separate assembly, so the
# C# helper has to be compiled against it (and the GDI+ backend it forwards to).
$runtimeDir = [System.Runtime.InteropServices.RuntimeEnvironment]::GetRuntimeDirectory()
$drawingRefs = @(
  'System.Drawing.Common.dll'
  'System.Drawing.Primitives.dll'
  'System.Private.Windows.Core.dll'
  'System.Private.Windows.GdiPlus.dll'
) | ForEach-Object { Join-Path $runtimeDir $_ } | Where-Object { Test-Path -LiteralPath $_ }
Add-Type -ReferencedAssemblies $drawingRefs -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;

public static class IconPrep {
  static bool OpaqueLight(Color c) { return c.A > 8 && (c.R + c.G + c.B) > 40; }

  public static void Run(string input, string output, int size, double radius) {
    using (var src = new Bitmap(input)) {
      int minX = src.Width, minY = src.Height, maxX = -1, maxY = -1;
      for (int y = 0; y < src.Height; y++) {
        for (int x = 0; x < src.Width; x++) {
          if (!OpaqueLight(src.GetPixel(x, y))) continue;
          if (x < minX) minX = x;
          if (x > maxX) maxX = x;
          if (y < minY) minY = y;
          if (y > maxY) maxY = y;
        }
      }
      if (maxX < minX || maxY < minY) throw new Exception("no visible art found in " + input);
      int side = Math.Max(maxX - minX + 1, maxY - minY + 1);
      int offX = minX + ((maxX - minX + 1) - side) / 2;
      int offY = minY + ((maxY - minY + 1) - side) / 2;

      using (var dst = new Bitmap(size, size, PixelFormat.Format32bppArgb)) {
        using (Graphics g = Graphics.FromImage(dst)) {
          g.InterpolationMode = InterpolationMode.HighQualityBicubic;
          g.PixelOffsetMode = PixelOffsetMode.HighQuality;
          g.SmoothingMode = SmoothingMode.HighQuality;
          g.DrawImage(src, new Rectangle(0, 0, size, size), new Rectangle(offX, offY, side, side), GraphicsUnit.Pixel);
        }

        // Rounded-square mask, 4x4 supersampled for a clean antialiased edge.
        double r = size * radius;
        var data = dst.LockBits(new Rectangle(0, 0, size, size), ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
        try {
          int stride = data.Stride;
          var row = new byte[stride];
          for (int y = 0; y < size; y++) {
            System.Runtime.InteropServices.Marshal.Copy(data.Scan0 + y * stride, row, 0, stride);
            for (int x = 0; x < size; x++) {
              int hits = 0;
              for (int sy = 0; sy < 4; sy++) {
                for (int sx = 0; sx < 4; sx++) {
                  if (Inside(x + (sx + 0.5) / 4.0, y + (sy + 0.5) / 4.0, size, r)) hits++;
                }
              }
              if (hits == 16) continue;
              int o = x * 4;
              if (hits == 0) { row[o + 3] = 0; continue; }
              int a = row[o + 3];
              row[o + 3] = (byte)((a * hits + 8) / 16);
            }
            System.Runtime.InteropServices.Marshal.Copy(row, 0, data.Scan0 + y * stride, stride);
          }
        } finally {
          dst.UnlockBits(data);
        }
        dst.Save(output, ImageFormat.Png);
      }
    }
  }

  static bool Inside(double px, double py, double size, double r) {
    if (px >= r && px <= size - r) return true;
    if (py >= r && py <= size - r) return true;
    double cx = px < r ? r : size - r;
    double cy = py < r ? r : size - r;
    double dx = px - cx, dy = py - cy;
    return dx * dx + dy * dy <= r * r;
  }
}
'@

$SourcePathFull = (Resolve-Path $SourcePath).Path
$TargetPathFull = [System.IO.Path]::GetFullPath($TargetPath)
$TargetPathDir = [System.IO.Path]::GetDirectoryName($TargetPathFull)
if (-not (Test-Path -LiteralPath $TargetPathDir)) {
  New-Item -ItemType Directory -Force -Path $TargetPathDir | Out-Null
}
[IconPrep]::Run($SourcePathFull, $TargetPathFull, $Size, $CornerRadius)
Write-Output "wrote $TargetPathFull"
