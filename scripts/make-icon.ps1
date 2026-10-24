# Renders the Soundesti app icon (music note inside a glowing crystal) at all Windows
# icon sizes and writes assets/icons/soundesti-<size>.png and assets/icons/soundesti.ico.
# Original artwork: a square and a diamond frame overlap into an eight-point crystal
# (inspired by floating "end crystal" style gems), with a glowing core and a note.
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$outDir = Join-Path $root "assets\icons"
New-Item -ItemType Directory -Force $outDir | Out-Null

function C($a, $r, $g, $b) { [System.Drawing.Color]::FromArgb($a, $r, $g, $b) }
function P($x, $y) { New-Object System.Drawing.PointF([single]$x, [single]$y) }

function Draw-Icon([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = "AntiAlias"; $g.PixelOffsetMode = "HighQuality"; $g.CompositingQuality = "HighQuality"
    $g.Clear([System.Drawing.Color]::Transparent)
    $c = $s / 2.0
    $small = $s -le 24

    # Rounded-square background.
    $r = $s * 0.22; $m = $s * 0.03; $w = $s - 2 * $m
    $bg = New-Object System.Drawing.Drawing2D.GraphicsPath
    $bg.AddArc($m, $m, 2 * $r, 2 * $r, 180, 90); $bg.AddArc($m + $w - 2 * $r, $m, 2 * $r, 2 * $r, 270, 90)
    $bg.AddArc($m + $w - 2 * $r, $m + $w - 2 * $r, 2 * $r, 2 * $r, 0, 90); $bg.AddArc($m, $m + $w - 2 * $r, 2 * $r, 2 * $r, 90, 90)
    $bg.CloseFigure()
    $bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush((P 0 0), (P 0 $s), (C 255 0x22 0x1a 0x33), (C 255 0x0d 0x12 0x18))
    $g.FillPath($bgBrush, $bg)

    # Core glow (magenta/violet, like a crystal's inner light).
    $glowR = $s * 0.34
    $glow = New-Object System.Drawing.Drawing2D.GraphicsPath
    $glow.AddEllipse($c - $glowR, $c - $glowR, 2 * $glowR, 2 * $glowR)
    $pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush($glow)
    $pgb.CenterColor = C 170 0xc0 0x5c 0xff
    $pgb.SurroundColors = @((C 0 0xc0 0x5c 0xff))
    $g.FillPath($pgb, $glow)

    # Crystal: axis-aligned square (violet) + diamond (teal) => eight-point gem.
    $sq = $s * 0.235
    $sqPts = @((P ($c - $sq) ($c - $sq)), (P ($c + $sq) ($c - $sq)), (P ($c + $sq) ($c + $sq)), (P ($c - $sq) ($c + $sq)))
    $dm = $s * 0.335
    $dmPts = @((P $c ($c - $dm)), (P ($c + $dm) $c), (P $c ($c + $dm)), (P ($c - $dm) $c))
    $g.FillPolygon((New-Object System.Drawing.SolidBrush (C 55 0xb4 0x6c 0xff)), $sqPts)
    $g.FillPolygon((New-Object System.Drawing.SolidBrush (C 45 0x2e 0xd3 0xb7)), $dmPts)
    $penSq = New-Object System.Drawing.Pen((C 235 0xc8 0x8c 0xff), [single]([math]::Max(1.0, $s * 0.026)))
    $penDm = New-Object System.Drawing.Pen((C 255 0x2e 0xd3 0xb7), [single]([math]::Max(1.2, $s * 0.034)))
    $penSq.LineJoin = "Round"; $penDm.LineJoin = "Round"
    $g.DrawPolygon($penSq, $sqPts)
    $g.DrawPolygon($penDm, $dmPts)
    if (-not $small) {
        # Short glints at the gem's tips (kept clear of the note).
        $glint = New-Object System.Drawing.Pen((C 150 0xff 0xff 0xff), [single]($s * 0.010))
        foreach ($p in $dmPts) {
            $dx = ($c - $p.X) * 0.22; $dy = ($c - $p.Y) * 0.22
            $g.DrawLine($glint, $p, (P ($p.X + $dx) ($p.Y + $dy)))
        }
    }

    # Eighth note, white, centred on the core.
    $white = New-Object System.Drawing.SolidBrush (C 255 0xf4 0xf7 0xfa)
    $hw = $s * 0.150; $hh = $s * 0.112
    $hx = $c - $s * 0.075; $hy = $c + $s * 0.085
    $state = $g.Save()
    $g.TranslateTransform([single]($hx), [single]($hy)); $g.RotateTransform(-22)
    $g.FillEllipse($white, [single](-$hw / 2), [single](-$hh / 2), [single]$hw, [single]$hh)
    $g.Restore($state)
    $stemW = [math]::Max(1.0, $s * 0.034)
    $stemX = $hx + $hw * 0.42
    $stemTop = $c - $s * 0.175
    $g.FillRectangle($white, [single]($stemX - $stemW / 2), [single]$stemTop, [single]$stemW, [single]($hy - $stemTop))
    $flag = New-Object System.Drawing.Drawing2D.GraphicsPath
    $fx = $stemX + $stemW / 2
    $flag.AddBezier((P ($fx - $stemW) $stemTop), (P ($fx + $s * 0.05) ($stemTop + $s * 0.03)), (P ($fx + $s * 0.11) ($stemTop + $s * 0.07)), (P ($fx + $s * 0.06) ($stemTop + $s * 0.17)))
    $flag.AddBezier((P ($fx + $s * 0.06) ($stemTop + $s * 0.17)), (P ($fx + $s * 0.075) ($stemTop + $s * 0.10)), (P ($fx + $s * 0.03) ($stemTop + $s * 0.075)), (P ($fx - $stemW) ($stemTop + $s * 0.075)))
    $flag.CloseFigure()
    $g.FillPath($white, $flag)

    $g.Dispose()
    return $bmp
}

$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = @()
foreach ($sz in $sizes) {
    $bmp = Draw-Icon $sz
    $path = Join-Path $outDir "soundesti-$sz.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $pngs += , @($sz, [IO.File]::ReadAllBytes($path))
}

# ICO container with PNG-compressed entries (supported since Windows Vista).
$ms = New-Object IO.MemoryStream
$bw = New-Object IO.BinaryWriter($ms)
$bw.Write([UInt16]0); $bw.Write([UInt16]1); $bw.Write([UInt16]$pngs.Count)
$offset = 6 + 16 * $pngs.Count
foreach ($e in $pngs) {
    $sz = $e[0]; $data = $e[1]
    $dim = if ($sz -ge 256) { 0 } else { $sz }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([UInt16]1); $bw.Write([UInt16]32); $bw.Write([UInt32]$data.Length); $bw.Write([UInt32]$offset)
    $offset += $data.Length
}
foreach ($e in $pngs) { $bw.Write($e[1]) }
$bw.Flush()
[IO.File]::WriteAllBytes((Join-Path $outDir "soundesti.ico"), $ms.ToArray())
"wrote $($sizes.Count) PNGs and soundesti.ico to assets/icons"
