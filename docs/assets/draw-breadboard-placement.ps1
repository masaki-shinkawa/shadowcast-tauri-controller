$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$bitmap = [System.Drawing.Bitmap]::new(1920,1320)
$g = [System.Drawing.Graphics]::FromImage($bitmap)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$g.Clear([System.Drawing.ColorTranslator]::FromHtml('#F1F5F9'))
function Brush([string]$color) { [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($color)) }
function Rect([single]$x,[single]$y,[single]$w,[single]$h,[string]$color) {
 $b=Brush $color; $g.FillRectangle($b,$x,$y,$w,$h); $b.Dispose()
}
function Text([string]$text,[single]$x,[single]$y,[single]$size=28,[string]$color='#172B46') {
 $font=[System.Drawing.Font]::new('Yu Gothic',$size,[System.Drawing.FontStyle]::Bold,[System.Drawing.GraphicsUnit]::Pixel)
 $b=Brush $color; $g.DrawString($text,$font,$b,$x,$y); $font.Dispose(); $b.Dispose()
}
function Line([single]$x1,[single]$y1,[single]$x2,[single]$y2,[string]$color='#708398',[single]$width=3) {
 $p=[System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml($color),$width)
 $g.DrawLine($p,$x1,$y1,$x2,$y2); $p.Dispose()
}
function Arrow([single]$x1,[single]$y1,[single]$x2,[single]$y2,[string]$color='#1565C0') {
 $p=[System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml($color),5)
 $p.CustomEndCap=[System.Drawing.Drawing2D.AdjustableArrowCap]::new(5,6)
 $g.DrawLine($p,$x1,$y1,$x2,$y2); $p.Dispose()
}
function Hole([single]$x,[single]$y,[string]$color='#A7AFAF') {
 $b=Brush $color; $g.FillEllipse($b,$x-6,$y-6,12,12); $b.Dispose()
}

Text 'ブレッドボード：ピンを挿す位置' 66 35 52
Text '写真と同じ向き：左に赤・黒のレール／上が1番。まず1枚ずつ作業します。' 70 109 30 '#52677E'
Rect 40 185 875 955 '#FFFFFF'
Rect 945 185 935 955 '#FFFFFF'
Text '① 黄色の2列に、9本ピンを挿す' 65 210 34
Text '左ブロックの i 列 ／ 右ブロックの e 列' 66 266 27 '#52677E'

# This user's board has six holes per half, labelled l..g / f..a from left to right.
# Index positions include a 3-pitch gap between the inner columns g and f.
# i and e are 6 pitches apart, matching a 15.24 mm RP2040-Zero row separation.
$columnX=@(260,300,340,380,420,460,580,620,660,700,740,780)
$columnNames=@('l','k','j','i','h','g','f','e','d','c','b','a')
$startY=362
$pitch=40
Rect 100 317 754 725 '#F4F0E6'
Rect 488 339 65 675 '#DED9CB'
Line 116 340 116 1018 '#D85A59' 4
Line 186 340 186 1018 '#495764' 4
for($c=0;$c -lt $columnNames.Count;$c++) {
 $color=if($columnNames[$c] -in @('i','e')){'#125CB0'}else{'#647C8B'}
 Text $columnNames[$c] ($columnX[$c]-8) 319 24 $color
}
for($row=1;$row -le 16;$row++) {
 $y=$startY+($row-1)*$pitch
 $color=if($row -in @(3,11)){'#125CB0'}else{'#647C8B'}
 Text ([string]$row) 209 ($y-15) 24 $color
 foreach($x in $columnX){Hole $x $y}
 Hole 139 $y; Hole 168 $y
}

# Selected holes are shown by the actual pin centres, not an entire shorted row.
foreach($x in @(380,620)) {
 for($row=3;$row -le 11;$row++) {
  $y=$startY+($row-1)*$pitch
  Rect ($x-14) ($y-18) 28 36 '#F5CC39'
  Rect ($x-5) ($y-5) 10 10 '#6B7888'
 }
}
$outline=[System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml('#168BA4'),4)
$outline.DashStyle=[System.Drawing.Drawing2D.DashStyle]::Dash
# The outline spans the centre trench. Pins i3/e3 form the USB-end row.
$g.DrawRectangle($outline,358,417,284,369)
$g.DrawRectangle($outline,447,397,105,83)
$outline.Dispose()
Text 'USB側 ↑' 443 488 25 '#13778C'
Text '基板は' 445 586 24 '#13778C'
Text 'この上に' 445 623 24 '#13778C'
Text '載せる' 451 660 24 '#13778C'
Text '短い横列は付けない' 352 809 24 '#A52B27'
Text 'i3〜i11' 270 1061 33 '#125CB0'
Text 'e3〜e11' 581 1061 33 '#125CB0'
Text '上側1〜16番を拡大。17〜30番は省略。' 165 1106 21 '#52677E'

Text '② 短い脚へ基板を載せる' 974 210 35
Text '横から見た図（部品を離して表示）' 979 267 26 '#52677E'
Text 'USB端子・ボタンのある面が上' 1039 330 30 '#1565C0'

# Exploded side elevation of TWO side headers, with no rear header.
Rect 1070 445 610 25 '#176583'
Rect 1300 386 143 59 '#B7C4D2'
Text 'USB' 1340 397 25
Rect 1488 419 48 26 '#D8DFE6'
foreach($x in @(1135,1605)) {
 Rect ($x-11) 440 22 35 '#F1F5F9'
 Rect ($x-5) 528 10 181 '#788797'
 Rect ($x-26) 572 52 45 '#EDC13F'
}
Text '基板' 1715 437 27 '#176583'
Arrow 1711 486 1640 525
Text '短い脚' 1681 539 25
Text '黄色い樹脂' 1285 574 29 '#95670A'
Text '長い脚' 1304 657 28 '#52677E'
Arrow 1135 722 1135 784
Arrow 1605 722 1605 784
Rect 1020 799 730 140 '#E6E0D2'
Rect 1117 799 36 10 '#8A8D8E'
Rect 1587 799 36 10 '#8A8D8E'
Rect 1371 799 55 48 '#C6C0B4'
Text 'ブレッドボード' 1265 860 32 '#5F6F7C'
Text '長い脚を穴へ挿し、樹脂を下側にする。' 1030 982 28
Text '基板を載せ、上に出た短い脚をはんだ付け。' 1005 1031 27
Text '※ 固定台として使っても、はんだ付けは必要です。' 1002 1086 24 '#52677E'

Text 'USBは両方とも外す。電源レールは使わない。' 370 1165 38 '#A52B27'
Text '穴が合わなければ押し込まず、基板の左右の穴が無理なく重なる位置を確認してください。' 220 1226 27 '#52677E'

$path=Join-Path $PSScriptRoot 'rp2040-zero-breadboard-placement.png'
$bitmap.Save($path,[System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bitmap.Dispose()
Write-Output $path
