# Screenshot RENotepad without stealing focus: the window is shown nearly
# transparent and click-through, never activated, and captured with PrintWindow.
# -Dialog save|goto|about opens that dialog and composes it over the dimmed
# window, the way it looks on screen.
# -File opens that file in the first tab (extra tabs are modified Untitled ones).
# The picture is cropped to the visible window.
# Usage: pwsh tests/ui/shot.ps1 -Out shot.png [-Exe ...] [-Light] [-Tabs 3] [-Lines 60] [-Dialog save] [-File notes.txt]
param(
  [Parameter(Mandatory)][string]$Out,
  [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'),
  [switch]$Light,
  [int]$Tabs = 2,
  [int]$Lines = 1,
  [ValidateSet('', 'save', 'goto', 'about')][string]$Dialog = '',
  [string]$File
)
. "$PSScriptRoot\lib.ps1"

$args_ = if ($File) { @("`"$((Resolve-Path $File).Path)`"") } else { @() }
$p = Start-Process $Exe -ArgumentList $args_ -WindowStyle Hidden -PassThru
$main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 300
if ($Light) { Cmd $main 44 }
if ($File) {
  for ($i = 1; $i -lt $Tabs; $i++) { Cmd $main 1; Type-Text ([Q]::VisibleEdit($main)) 'This is RE:Notepad' }
  if ($Tabs -gt 1 -and $Dialog -ne 'save') { Cmd $main 12 } # Ctrl+Tab wraps back to the file (the save prompt needs the modified tab)
  Show-Ghost $main
} else {
  if ($Tabs -gt 1) { Type-Text ([Q]::VisibleEdit($main)) 'This is RE:Notepad' } # a modified tab behind
  for ($i = 1; $i -lt $Tabs; $i++) { Cmd $main 1 }
  Show-Ghost $main
  # The active tab gets $Lines lines (enough of them brings up the scrollbar).
  $text = (1..$Lines | ForEach-Object { if ($_ -eq 1) { 'This is RE:Notepad' } else { "line $_" } }) -join "`r`n"
  Type-Text ([Q]::VisibleEdit($main)) $text
}
Start-Sleep -Milliseconds 300
if (-not $Dialog) {
  Snap $main $Out
} else {
  Cmd $main @{ save = 10; goto = 25; about = 48 }[$Dialog]
  $dlg = Wait-Top $p.Id 'RENotepadDialog' ([NullString]::Value); Start-Sleep -Milliseconds 300
  $back = "$Out.main.png"; $front = "$Out.dialog.png"
  Snap $main $back; Snap $dlg $front
  $mr = New-Object int[] 4; $dr = New-Object int[] 4
  [Q]::GetWindowRect($main, $mr) | Out-Null; [Q]::GetWindowRect($dlg, $dr) | Out-Null
  $bmp = [Drawing.Bitmap]::FromFile($back); $fg = [Drawing.Bitmap]::FromFile($front)
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.FillRectangle((New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(77, 0, 0, 0))), 0, 0, $bmp.Width, $bmp.Height)
  # DWM rounds the corners (8 px) and draws the border; PrintWindow sees neither.
  $g.SmoothingMode = 'AntiAlias'
  $x = $dr[0] - $mr[0]; $y = $dr[1] - $mr[1]; $w = $fg.Width; $h = $fg.Height; $d = 16 * $fg.HorizontalResolution / 96
  $path = New-Object Drawing.Drawing2D.GraphicsPath
  $path.AddArc($x, $y, $d, $d, 180, 90); $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90); $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90); $path.CloseFigure()
  $g.SetClip($path); $g.DrawImage($fg, $x, $y, $w, $h); $g.ResetClip()
  $border = if ($Light) { [Drawing.Color]::FromArgb(202, 202, 202) } else { [Drawing.Color]::FromArgb(61, 61, 61) }
  $g.DrawPath((New-Object Drawing.Pen $border), $path)
  $g.Dispose(); $fg.Dispose(); $bmp.Save("$Out.tmp.png"); $bmp.Dispose()
  Move-Item -Force "$Out.tmp.png" $Out; Remove-Item $back, $front
}
Crop-ToFrame $main $Out
Stop-Process -Id $p.Id -Force
"wrote $Out"
