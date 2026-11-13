# Draws the Lampdesigner icon (glowing ribbed shade on a dark tile) and
# writes installer\lampdesigner.ico with 16-256 px images (PNG-compressed).

Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'

function New-IconBitmap([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $k = $s / 256.0

    # rounded dark tile
    $r = 52 * $k
    $tile = New-Object System.Drawing.Drawing2D.GraphicsPath
    $tile.AddArc(0, 0, 2 * $r, 2 * $r, 180, 90)
    $tile.AddArc($s - 2 * $r - 1, 0, 2 * $r, 2 * $r, 270, 90)
    $tile.AddArc($s - 2 * $r - 1, $s - 2 * $r - 1, 2 * $r, 2 * $r, 0, 90)
    $tile.AddArc(0, $s - 2 * $r - 1, 2 * $r, 2 * $r, 90, 90)
    $tile.CloseFigure()
    $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 30, 34, 44))), $tile)

    # warm glow behind the shade
    $glow = New-Object System.Drawing.Drawing2D.GraphicsPath
    $glow.AddEllipse(28 * $k, 20 * $k, 200 * $k, 190 * $k)
    $pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush $glow
    $pgb.CenterColor = [System.Drawing.Color]::FromArgb(150, 255, 170, 60)
    $pgb.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 255, 170, 60))
    $g.FillPath($pgb, $glow)

    # shade: bell silhouette
    $shade = New-Object System.Drawing.Drawing2D.GraphicsPath
    $shade.AddBezier(92 * $k, 52 * $k, 70 * $k, 90 * $k, 48 * $k, 150 * $k, 58 * $k, 176 * $k)
    $shade.AddLine(58 * $k, 176 * $k, 198 * $k, 176 * $k)
    $shade.AddBezier(198 * $k, 176 * $k, 208 * $k, 150 * $k, 186 * $k, 90 * $k, 164 * $k, 52 * $k)
    $shade.CloseFigure()
    $lg = New-Object System.Drawing.Drawing2D.LinearGradientBrush ((New-Object System.Drawing.PointF 0, (52 * $k)), (New-Object System.Drawing.PointF 0, (176 * $k)), ([System.Drawing.Color]::FromArgb(255, 255, 214, 120)), ([System.Drawing.Color]::FromArgb(255, 255, 150, 40)))
    $g.FillPath($lg, $shade)

    # twisted ribs (only where they stay readable)
    if ($s -ge 32) {
        $g.SetClip($shade)
        $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(110, 190, 90, 20)), ([math]::Max(1.0, 6 * $k))
        for ($i = -3; $i -le 7; $i++) {
            $x = (40 + $i * 24) * $k
            $g.DrawBezier($pen, $x, 40 * $k, $x + 14 * $k, 90 * $k, $x + 30 * $k, 140 * $k, $x + 50 * $k, 190 * $k)
        }
        $g.ResetClip()
    }

    # stem and foot
    $base = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 222, 224, 228))
    $g.FillRectangle($base, 120 * $k, 176 * $k, 16 * $k, 34 * $k)
    $g.FillEllipse($base, 78 * $k, 202 * $k, 100 * $k, 20 * $k)
    $g.Dispose()
    return $bmp
}

$sizes = 256, 64, 48, 32, 16
$pngs = foreach ($s in $sizes) {
    $bmp = New-IconBitmap $s
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

$out = Join-Path $PSScriptRoot 'lampdesigner.ico'
$fs = [System.IO.File]::Create($out)
$w = New-Object System.IO.BinaryWriter $fs
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $w.Write($p) }
$w.Close()

# 64 px raw RGBA for the window/taskbar icon (embedded in the app)
$bmp = New-IconBitmap 64
$raw = New-Object byte[] (64 * 64 * 4)
for ($y = 0; $y -lt 64; $y++) {
    for ($x = 0; $x -lt 64; $x++) {
        $c = $bmp.GetPixel($x, $y)
        $i = ($y * 64 + $x) * 4
        $raw[$i] = $c.R; $raw[$i + 1] = $c.G; $raw[$i + 2] = $c.B; $raw[$i + 3] = $c.A
    }
}
[System.IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'icon64.rgba'), $raw)
$bmp.Dispose()
Write-Host "wrote $out"
