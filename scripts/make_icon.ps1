# Desenha o icone do Local Utils (flat, squircle, gradient slate->indigo,
# prancheta de clipboard com linhas de conteudo e selo verde de "copiado")
# e gera icon.ico + icons/icon.png + icons/icon-1024.png.
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

function Squircle([float]$s, [float]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc(0, 0, $r, $r, 180, 90)
    $path.AddArc($s - $r, 0, $r, $r, 270, 90)
    $path.AddArc($s - $r, $s - $r, $r, $r, 0, 90)
    $path.AddArc(0, $s - $r, $r, $r, 90, 90)
    $path.CloseFigure()
    return $path
}

function RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc($x, $y, $r, $r, 180, 90)
    $path.AddArc($x + $w - $r, $y, $r, $r, 270, 90)
    $path.AddArc($x + $w - $r, $y + $h - $r, $r, $r, 0, 90)
    $path.AddArc($x, $y + $h - $r, $r, $r, 90, 90)
    $path.CloseFigure()
    return $path
}

function Capsule([float]$x, [float]$y, [float]$w, [float]$h) {
    return RoundRect $x $y $w $h ($h / 2)
}

$bmp, $g = New-Canvas $size

# fundo squircle com gradient linear (slate escuro -> indigo do tema)
$r = $size * 0.224
$sq = Squircle $size $r
$rect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, `
    [System.Drawing.Color]::FromArgb(255, 38, 34, 74), `
    [System.Drawing.Color]::FromArgb(255, 124, 92, 240), 60)
$g.FillPath($grad, $sq)

# sombra da prancheta (translucida, flat moderno) - prancheta grande,
# pouco fundo roxo em volta (pedido do Joao: "um pouquinho so a menos")
$bx = 248.0; $by = 240.0; $bw = 528.0; $bh = 584.0; $br = 44.0
$shadow = RoundRect ($bx + 6) ($by + 10) $bw $bh $br
$alphaBlack = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(70, 12, 10, 30))
$g.FillPath($alphaBlack, $shadow)
$shadow.Dispose()

# prancheta branca
$board = RoundRect $bx $by $bw $bh $br
$g.FillPath([System.Drawing.Brushes]::White, $board)

# presilha no topo (slate escuro por cima da prancheta)
$clip = RoundRect 430 200 164 106 36
$clipBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 58, 54, 92))
$g.FillPath($clipBrush, $clip)
# brilho interno da presilha (capsula clara menor)
$clipInner = Capsule 458 228 108 30
$clipInnerBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 104, 98, 158))
$g.FillPath($clipInnerBrush, $clipInner)

# linhas de conteudo (indigo do tema; a ultima em ambar, quente)
$l1 = Capsule 312 372 316 28
$lineBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 124, 92, 240))
$g.FillPath($lineBrush, $l1)
$l2 = Capsule 312 444 268 28
$g.FillPath($lineBrush, $l2)
$l3 = Capsule 312 516 180 28
$amberBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 185, 68))
$g.FillPath($amberBrush, $l3)

# assinatura da marca - easter egg pequeno, estilo dos irmaos da suite
# (Segoe Script, branco ~47%, discreta no pe do fundo roxo)
$sigFont = New-Object System.Drawing.Font('Segoe Script', [float]($size * 0.047), [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$sigBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(120, 255, 255, 255))
$sigFormat = New-Object System.Drawing.StringFormat
$sigFormat.Alignment = [System.Drawing.StringAlignment]::Center
$g.DrawString('Taylor', $sigFont, $sigBrush, [float]($size / 2), [float]($size * 0.885), $sigFormat)
$sigFont.Dispose(); $sigBrush.Dispose(); $sigFormat.Dispose()

# selo verde de "copiado" (circulo + check branco), no canto da prancheta
$ccx = 692.0; $ccy = 740.0; $ccr = 92.0
$badge = New-Object System.Drawing.Drawing2D.GraphicsPath
$badge.AddEllipse($ccx - $ccr, $ccy - $ccr, $ccr * 2, $ccr * 2)
# aro branco separando o selo da prancheta
$ring = New-Object System.Drawing.Drawing2D.GraphicsPath
$ring.AddEllipse($ccx - $ccr - 10, $ccy - $ccr - 10, ($ccr + 10) * 2, ($ccr + 10) * 2)
$g.FillPath([System.Drawing.Brushes]::White, $ring)
$badgeBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 40, 182, 120))
$g.FillPath($badgeBrush, $badge)
$checkPen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, 30)
$checkPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$checkPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$checkPen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
$g.DrawLines($checkPen, [System.Drawing.PointF[]]@(
    (New-Object System.Drawing.PointF(($ccx - 40), $ccy)),
    (New-Object System.Drawing.PointF(($ccx - 10), ($ccy + 32))),
    (New-Object System.Drawing.PointF(($ccx + 44), ($ccy - 34)))
))

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
