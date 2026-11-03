# Desenha o icone PLACEHOLDER do Local Utils (flat, squircle, gradient navy->#5C8CEB,
# moldura de foto polaroid com sol e montanhas) e gera icon.ico + icons/icon.png.
# O motivo é herdado do molde da suíte: ícone próprio vem antes da v0.1.0.
Add-Type -AssemblyName System.Drawing

$root = "C:\Dev\underworld-apps\local-utils"
$size = 1024

function New-Canvas([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    return @($bmp, $g)
}

function Squircle([System.Drawing.Graphics]$g, [float]$s, [float]$r) {
    # squircle aproximado: rounded rect com raio ~22.4% do lado
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc(0, 0, $r, $r, 180, 90)
    $path.AddArc($s - $r, 0, $r, $r, 270, 90)
    $path.AddArc($s - $r, $s - $r, $r, $r, 0, 90)
    $path.AddArc(0, $s - $r, $r, $r, 90, 90)
    $path.CloseFigure()
    return $path
}

$bmp, $g = New-Canvas $size

# fundo squircle com gradient linear (navy -> azul do tema)
$r = $size * 0.224
$sq = Squircle $g $size $r
$rect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, `
    [System.Drawing.Color]::FromArgb(255, 38, 56, 92), `
    [System.Drawing.Color]::FromArgb(255, 92, 140, 235), 60)
$g.FillPath($grad, $sq)

# sombra da moldura (translucida, navy - flat moderno)
$shadow = New-Object System.Drawing.Drawing2D.GraphicsPath
$fx = $size * 0.235; $fy = $size * 0.225; $fw = $size * 0.53; $fh = $size * 0.55
$fr = $size * 0.045
$shadow.AddArc($fx + 6, $fy + 8, $fr, $fr, 180, 90)
$shadow.AddArc($fx + $fw - $fr + 6, $fy + 8, $fr, $fr, 270, 90)
$shadow.AddArc($fx + $fw - $fr + 6, $fy + $fh - $fr + 8, $fr, $fr, 0, 90)
$shadow.AddArc($fx + 6, $fy + $fh - $fr + 8, $fr, $fr, 90, 90)
$shadow.CloseFigure()
$alphaBlack = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(70, 12, 20, 40))
$g.FillPath($alphaBlack, $shadow)
$shadow.Dispose()

# moldura polaroid branca
$frame = New-Object System.Drawing.Drawing2D.GraphicsPath
$frame.AddArc($fx, $fy, $fr, $fr, 180, 90)
$frame.AddArc($fx + $fw - $fr, $fy, $fr, $fr, 270, 90)
$frame.AddArc($fx + $fw - $fr, $fy + $fh - $fr, $fr, $fr, 0, 90)
$frame.AddArc($fx, $fy + $fh - $fr, $fr, $fr, 90, 90)
$frame.CloseFigure()
$g.FillPath([System.Drawing.Brushes]::White, $frame)

# area da foto (dentro da moldura): ciano claro -> azul
$px = $fx + $fw * 0.09; $py = $fy + $fh * 0.09
$pw = $fw * 0.82; $ph = $fh * 0.70
$photo = New-Object System.Drawing.Rectangle([int]$px, [int]$py, [int]$pw, [int]$ph)
$photoGrad = New-Object System.Drawing.Drawing2D.LinearGradientBrush($photo, `
    [System.Drawing.Color]::FromArgb(255, 176, 216, 255), `
    [System.Drawing.Color]::FromArgb(255, 80, 130, 220), 90)
$g.FillRectangle($photoGrad, $photo)

# sol (circulo amarelo claro)
$sunR = $size * 0.055
$sunX = $px + $pw * 0.68; $sunY = $py + $ph * 0.28
$sun = New-Object System.Drawing.Drawing2D.GraphicsPath
$sun.AddEllipse($sunX - $sunR, $sunY - $sunR, $sunR * 2, $sunR * 2)
$sunBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 221, 60))
$g.FillPath($sunBrush, $sun)

# montanhas (2 triangulos sobrepostos, azul-escuro e medio)
$base = $py + $ph
$mnt1 = New-Object System.Drawing.Drawing2D.GraphicsPath
$mnt1.AddPolygon([System.Drawing.PointF[]]@(
    (New-Object System.Drawing.PointF(($px + $pw * 0.02), $base)),
    (New-Object System.Drawing.PointF(($px + $pw * 0.34), ($base - $ph * 0.52))),
    (New-Object System.Drawing.PointF(($px + $pw * 0.62), $base))
))
$g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(235, 30, 52, 96))), $mnt1)
$mnt2 = New-Object System.Drawing.Drawing2D.GraphicsPath
$mnt2.AddPolygon([System.Drawing.PointF[]]@(
    (New-Object System.Drawing.PointF(($px + $pw * 0.38), $base)),
    (New-Object System.Drawing.PointF(($px + $pw * 0.66), ($base - $ph * 0.38))),
    (New-Object System.Drawing.PointF(($px + $pw * 0.98), $base))
))
$g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 58, 92, 165))), $mnt2)

# assinatura da marca (estilo da suite: Segoe Script, branco ~55%, no pe do tile)
$sigFont = New-Object System.Drawing.Font('Segoe Script', [float]($size * 0.082), [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$sigBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(140, 255, 255, 255))
$sigFormat = New-Object System.Drawing.StringFormat
$sigFormat.Alignment = [System.Drawing.StringAlignment]::Center
$g.DrawString('Taylor', $sigFont, $sigBrush, [float]($size / 2), [float]($size * 0.845), $sigFormat)
$sigFont.Dispose(); $sigBrush.Dispose(); $sigFormat.Dispose()

$g.Dispose()

# salva master 1024 (fonte pra futuros redraws)
$bmp.Save("$root\icons\icon-1024.png", [System.Drawing.Imaging.ImageFormat]::Png)

# icons/icon.png 256 (AppImage/Arch)
$icon256 = New-Object System.Drawing.Bitmap($bmp, 256, 256)
$icon256.Save("$root\icons\icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
$icon256.Dispose()

# icon.ico multi-tamanho (256/64/48/32/16)
$sizes = @(256, 64, 48, 32, 16)
$streams = @()
foreach ($s in $sizes) {
    $b = New-Object System.Drawing.Bitmap($bmp, $s, $s)
    $ms = New-Object System.IO.MemoryStream
    $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $b.Dispose()
    $streams += ,([byte[]]$ms.ToArray())
    $ms.Dispose()
}
$ico = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ico)
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $d = $streams[$i]
    $bw.Write([byte]$(if ($s -ge 256) { 0 } else { $s })); $bw.Write([byte]$(if ($s -ge 256) { 0 } else { $s }))
    $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$d.Length); $bw.Write([uint32]$offset)
    $offset += $d.Length
}
foreach ($d in $streams) { $bw.Write($d) }
$bw.Flush()
[System.IO.File]::WriteAllBytes("$root\icon.ico", $ico.ToArray())
$bw.Dispose(); $ico.Dispose()

$bmp.Dispose()
"icones gerados: icon.ico ($((Get-Item $root\icon.ico).Length) bytes), icons/icon.png ($((Get-Item $root\icons\icon.png).Length) bytes), icons/icon-1024.png"
