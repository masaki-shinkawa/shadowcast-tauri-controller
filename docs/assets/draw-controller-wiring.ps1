$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$canvas = [System.Drawing.Bitmap]::new(1800, 1180)
$g = [System.Drawing.Graphics]::FromImage($canvas)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$g.Clear([System.Drawing.ColorTranslator]::FromHtml('#F8FAFC'))

function Brush([string]$color) { [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($color)) }
function Pen([string]$color, [single]$width) {
    $p = [System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml($color), $width)
    $p.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $p.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    return $p
}
function Label([string]$text, [single]$x, [single]$y, [single]$size = 30, [string]$color = '#172033') {
    $font = [System.Drawing.Font]::new('Yu Gothic', $size, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $b = Brush $color
    $g.DrawString($text, $font, $b, $x, $y)
    $font.Dispose(); $b.Dispose()
}
function Rect([single]$x,[single]$y,[single]$w,[single]$h,[string]$color) {
    $b = Brush $color; $g.FillRectangle($b,$x,$y,$w,$h); $b.Dispose()
}
function Dot([single]$x,[single]$y,[string]$color) {
    $b = Brush '#FFFFFF'; $g.FillEllipse($b,$x-20,$y-20,40,40); $b.Dispose()
    $b = Brush $color; $g.FillEllipse($b,$x-13,$y-13,26,26); $b.Dispose()
}
function Stroke($path,[string]$color) {
    $outline = Pen '#F8FAFC' 20
    $line = Pen $color 10
    $g.DrawPath($outline,$path); $g.DrawPath($line,$path)
    $outline.Dispose(); $line.Dispose()
}
function Board([single]$x,[string]$role,[string]$firmware) {
    Rect $x 380 320 430 '#164A6B'
    Rect ($x+90) 340 140 115 '#B9C4CF'
    Rect ($x+108) 348 104 16 '#38495B'
    Label 'USB-C' ($x+102) 390 25 '#172033'
    Label $role ($x+77) 294 38
    Label 'RP2040-Zero' ($x+56) 567 25 '#FFFFFF'
    Label $firmware ($x+45) 609 21 '#DDEAF3'
    Rect ($x+76) 674 66 60 '#DEE4E9'
    Rect ($x+183) 674 66 60 '#DEE4E9'
    Label 'BOOT' ($x+77) 744 18 '#DDEAF3'
    Label 'RESET' ($x+177) 744 18 '#DDEAF3'
    $leftLabels = @('5V','GND','3V3','29','28','27','26','15','14')
    for ($i=0; $i -lt 9; $i++) {
        $y = 425 + 44*$i
        $b = Brush '#E3C56D'
        $g.FillEllipse($b,$x-9,$y-9,18,18)
        $g.FillEllipse($b,$x+311,$y-9,18,18)
        $b.Dispose()
        Label $leftLabels[$i] ($x+16) ($y-13) 20 '#E6EEF4'
        Label ([string]$i) ($x+282) ($y-13) 20 '#E6EEF4'
    }
}

Label 'RP2040-Zero：3本だけ接続' 70 35 52
Label '写真と同じ向き：左がPC側／右がSwitch側／USB端子が上' 73 111 29 '#526377'
Board 180 'PC側' '2wiCC_Comms'
Board 1110 'Switch側' '2wiCC'

# Ground leaves each board from its left-edge SECOND pad (GND).
$ground = [System.Drawing.Drawing2D.GraphicsPath]::new()
$ground.AddBezier(180,469,80,469,80,890,180,900)
$ground.AddLine(180,900,930,900)
$ground.AddBezier(930,900,1020,900,1020,469,1110,469)
Stroke $ground '#25313D'

# Blue: left GP0 (top right) -> right GP1 (second on right).
$blue = [System.Drawing.Drawing2D.GraphicsPath]::new()
$blue.AddBezier(500,425,720,207,1090,215,1480,215)
$blue.AddBezier(1480,215,1590,215,1600,469,1430,469)
Stroke $blue '#176BE8'

# Orange: left GP1 -> right GP0. White halo means crossings are not junctions.
$orange = [System.Drawing.Drawing2D.GraphicsPath]::new()
$orange.AddBezier(500,469,760,490,820,179,1090,179)
$orange.AddLine(1090,179,1500,179)
$orange.AddBezier(1500,179,1700,179,1700,425,1430,425)
Stroke $orange '#CF6413'

Dot 500 425 '#176BE8'
Dot 1430 469 '#176BE8'
Dot 500 469 '#CF6413'
Dot 1430 425 '#CF6413'
Dot 180 469 '#25313D'
Dot 1110 469 '#25313D'

# Local endpoint labels: never use a wire's color to imply an unrelated pad.
Rect 537 492 337 108 '#F8FAFC'
Label '青：0（TX）' 550 498 28 '#176BE8'
Label '橙：1（RX）' 550 542 28 '#CF6413'
Rect 1480 487 300 108 '#F8FAFC'
Label '橙：0（TX）' 1490 493 28 '#CF6413'
Label '青：1（RX）' 1490 537 28 '#176BE8'
Label 'GND' 56 421 27 '#25313D'
Label 'GND' 987 421 27 '#25313D'
Label '※ 線の交差部分は接続しない' 560 814 24 '#526377'

Rect 620 635 350 126 '#FFFFFF'
Label '0 → 1' 648 642 30 '#176BE8'
Label '1 ← 0' 648 681 30 '#CF6413'
Label 'GND ↔ GND' 648 720 27 '#25313D'

Label '配線するときは、両方のUSBを抜く' 360 952 43 '#A52B27'
Label '5V・3V3は接続しない' 606 1012 37 '#A52B27'
Label 'ピンヘッダーをはんだ付け → メス–メスのジャンパー線を3本接続' 235 1080 29
Label '端子配置を示す模式図です。接続前に基板の「0」「1」「GND」の印字を確認してください。' 174 1125 24 '#526377'

$path = Join-Path $PSScriptRoot 'rp2040-zero-wiring.png'
$canvas.Save($path,[System.Drawing.Imaging.ImageFormat]::Png)
$ground.Dispose(); $blue.Dispose(); $orange.Dispose(); $g.Dispose(); $canvas.Dispose()
Write-Output $path
