# Markdown preview, focus-free: View > Markdown preview, the menu-bar button,
# Esc, per-tab state across tab switches, a relative link that opens a tab,
# and theme or zoom changes while the preview shows. It saves preview-dark.png
# and preview-light.png beside the test files for a look.
# Usage: pwsh tests/ui/preview.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
Add-Type -Name Dpi -Namespace Z -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);'
Add-Type -Name Kids -Namespace Z -MemberDefinition @'
[DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, Cb cb, IntPtr l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
delegate bool Cb(IntPtr h, IntPtr l);
/// The visible preview (RichEdit) child of the main window, or zero.
public static IntPtr Preview(IntPtr main) {
  IntPtr f = IntPtr.Zero;
  EnumChildWindows(main, (h, l) => { var c = new System.Text.StringBuilder(64); GetClassNameW(h, c, 64);
    if (c.ToString() == "RICHEDIT50W" && (GetWindowLongPtrW(h, -16).ToInt64() & 0x10000000) != 0) { f = h; return false; } return true; }, IntPtr.Zero);
  return f;
}
'@

$Dir = Join-Path $env:TEMP 'renotepad-preview'
New-Item -ItemType Directory -Force $Dir | Out-Null
$Readme = Join-Path $Dir 'README.md'
[IO.File]::WriteAllText($Readme, "[other file](other.md)`r`n`r`n# Title`r`n`r`nSome **bold** and ``code``.`r`n`r`n- item`r`n- [x] done`r`n`r`n> quoted`r`n`r`n| A | B |`r`n|---|--:|`r`n| 1 | 2 |`r`n`r`n---`r`n`r`n``````r`nfn main() {}`r`n``````r`n")
[IO.File]::WriteAllText((Join-Path $Dir 'other.md'), "---`r`ntitle: Front`r`n---`r`n# Other`r`n")
$Bullet = [string][char]0x2022
$Checked = [string][char]0x2611
$script:failed = $false
function PreviewText($h) { $sb = New-Object Text.StringBuilder 4096; [Q]::SendMessageW($h, 0x0D, [IntPtr]4096, $sb) | Out-Null; $sb.ToString() }
try {

$p = Start-Process $Exe -ArgumentList "`"$Readme`"" -WindowStyle Hidden -PassThru
$main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 500
$edit = [Q]::VisibleEdit($main)
Check 'a Markdown file opens in the editor' ([Z.Kids]::Preview($main) -eq [IntPtr]::Zero) $true

Cmd $main 54 # View > Markdown preview
$pv = [Z.Kids]::Preview($main)
Check 'the menu item shows the preview' ($pv -ne [IntPtr]::Zero) $true
Check 'the editor hides behind it' ([Q]::VisibleEdit($main)) 0
$t = PreviewText $pv
Check 'the heading renders as text' ($t.Contains('Title') -and -not $t.Contains('# Title')) $true
Check 'a bullet item gets a bullet' ($t.Contains("$Bullet`titem")) $true
Check 'a task item gets a checkbox' ($t.Contains("$Checked`tdone")) $true
Check 'a table cell renders' ($t.Contains('1') -and -not $t.Contains('|---')) $true

$before = Text $edit
Cmd $main 27 # Time/date (F5)
Check 'editing commands do nothing in the preview' ((Text $edit) -eq $before) $true
Check 'and the tab stays unmodified' (Title $main) 'README.md - RENotepad'

[Q]::PostMessageW($pv, 0x100, [IntPtr]0x1B, [IntPtr]0x00010001) | Out-Null; Start-Sleep -Milliseconds 400 # Esc
Check 'Esc goes back to the editor' ("$([Z.Kids]::Preview($main) -eq [IntPtr]::Zero) $([Q]::VisibleEdit($main) -eq $edit)") 'True True'

Show-Ghost $main
$scale = [Z.Dpi]::GetDpiForWindow($main) / 96.0
function Px($v) { [int][Math]::Round($v * $scale) }
$c = New-Object int[] 4; [Q]::GetClientRect($main, $c) | Out-Null
# The button sits left of the gear (44 px from the right edge) in the 32 px menu bar under the 42 px strip.
$l = [IntPtr](((Px 58) -shl 16) -bor ($c[2] - (Px 72)))
[Q]::PostMessageW($main, 0x201, [IntPtr]1, $l) | Out-Null; [Q]::PostMessageW($main, 0x202, [IntPtr]0, $l) | Out-Null; Start-Sleep -Milliseconds 500
$pv = [Z.Kids]::Preview($main)
Check 'the menu-bar button shows the preview' ($pv -ne [IntPtr]::Zero) $true
Snap $main (Join-Path $Dir 'preview-dark.png'); Crop-ToFrame $main (Join-Path $Dir 'preview-dark.png')

Cmd $main 1 # New tab
Check 'a new tab shows its editor' ("$([Z.Kids]::Preview($main) -eq [IntPtr]::Zero) $([Q]::VisibleEdit($main) -ne $edit)") 'True True'
Cmd $main 13 # Previous tab
Check 'the Markdown tab keeps its preview' ([Z.Kids]::Preview($main)) $pv

Cmd $main 44 # Dark mode off
Check 'the preview survives a theme change' ((PreviewText ([Z.Kids]::Preview($main))).Contains("$Bullet`titem")) $true
Snap $main (Join-Path $Dir 'preview-light.png'); Crop-ToFrame $main (Join-Path $Dir 'preview-light.png')
Cmd $main 44
Cmd $main 40 # Zoom in
Check 'the preview survives a zoom' ([Z.Kids]::Preview($main)) $pv

# The link is the first line, at the 16 px text inset.
$l = [IntPtr](((Px 26) -shl 16) -bor (Px 30))
[Q]::PostMessageW($pv, 0x201, [IntPtr]1, $l) | Out-Null; [Q]::PostMessageW($pv, 0x202, [IntPtr]0, $l) | Out-Null; Start-Sleep -Milliseconds 700
Check 'a relative Markdown link opens in a tab' (Title $main) 'other.md - RENotepad'
Check 'the linked file opens in the editor' ([Z.Kids]::Preview($main) -eq [IntPtr]::Zero) $true
Cmd $main 54
$t = PreviewText ([Z.Kids]::Preview($main))
Check 'front matter shows as its text above the heading' ($t -match '^\s*title: Front\s+Other\s*$') $true

[Q]::PostMessageW($main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Check 'the window closes without a prompt' ($p.WaitForExit(3000)) $true

} finally {
  if ($p -and -not $p.HasExited) { $p.Kill() }
  Remove-Item $TestKey -Recurse -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
