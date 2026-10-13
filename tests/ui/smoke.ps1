# Focus-free UI smoke test for RENotepad: the main window stays hidden (or a
# click-through 1/255-opaque ghost) and everything is driven by window
# messages, so it never steals the keyboard or covers the screen.
# Usage: pwsh tests/ui/smoke.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
Add-Type -Name Dpi -Namespace Z -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);'

$File = Join-Path $env:TEMP 'renotepad-smoke.txt'
[IO.File]::WriteAllText($File, "alpha beta`r`ngamma beta`r`ndelta`r`n")
$Key = 'HKCU:\Software\RENotepad'
# Run from default settings, and put back whatever was there afterwards.
$Backup = Join-Path $env:TEMP 'renotepad-smoke-settings.reg'
$hadKey = Test-Path $Key
if ($hadKey) { reg export 'HKCU\Software\RENotepad' $Backup /y | Out-Null; Remove-Item $Key -Recurse }
$script:failed = $false
try {

$p = Start-Process $Exe -ArgumentList "`"$File`"" -WindowStyle Hidden -PassThru
$main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 500
$scale = [Z.Dpi]::GetDpiForWindow($main) / 96.0
$edit = [Q]::VisibleEdit($main)
function Px($v) { [int][Math]::Round($v * $scale) }
function Click($x, $y, $down = 0x201, $up = 0x202) {
  $l = [IntPtr](((Px $y) -shl 16) -bor (Px $x))
  # Posted, not sent: a click can open a modal prompt, and SendMessage would wait on it.
  [Q]::PostMessageW($main, $down, [IntPtr]1, $l) | Out-Null; [Q]::PostMessageW($main, $up, [IntPtr]0, $l) | Out-Null
  Start-Sleep -Milliseconds 300
}

Check 'title names the file' (Title $main) 'renotepad-smoke.txt - RENotepad'

# Find: Alt+R on the "Wrap around" label must tick the box; third Find Next must wrap.
Cmd $main 21
$find = Wait-Top $p.Id '#32770' 'Find'
[Q]::SendMessageW([Q]::GetDlgItem($find, 1152), 0x0C, [IntPtr]::Zero, 'beta') | Out-Null
[Q]::PostMessageW([Q]::GetDlgItem($find, 1152), 0x0106, [IntPtr][int][char]'r', [IntPtr]0x20000000) | Out-Null; Start-Sleep -Milliseconds 300
Check 'Alt+R ticks Wrap around' $([Q]::SendMessageW([Q]::GetDlgItem($find, 1042), 0xF0, [IntPtr]::Zero, [IntPtr]::Zero)) 1
Cmd $find 1; Check 'find 1' (Sel $edit) '6-10'
Cmd $find 1; Check 'find 2' (Sel $edit) '18-22'
Cmd $find 1; Check 'find 3 wraps' (Sel $edit) '6-10'
# Closing Find while its "Cannot find" box is up must end that box too.
[Q]::SendMessageW([Q]::GetDlgItem($find, 1152), 0x0C, [IntPtr]::Zero, 'zzz') | Out-Null
Cmd $find 1
Check 'not found opens a box over Find' ([Q]::Owned($p.Id, 'RENotepadDialog', $find) -ne [IntPtr]::Zero) $true
Cmd $find 2
Check 'closing Find ends the box' ("$([Q]::Top($p.Id, 'RENotepadDialog', [NullString]::Value) -eq [IntPtr]::Zero) $([Q]::IsWindowEnabled($main))") 'True True'

# Replace All.
Cmd $main 24
$rep = Wait-Top $p.Id '#32770' 'Replace'
[Q]::SendMessageW([Q]::GetDlgItem($rep, 1152), 0x0C, [IntPtr]::Zero, 'beta') | Out-Null
[Q]::SendMessageW([Q]::GetDlgItem($rep, 1153), 0x0C, [IntPtr]::Zero, 'BETA') | Out-Null
Cmd $rep 1025; Check 'replace all' ((Text $edit) -replace "`r`n", '|') 'alpha BETA|gamma BETA|delta|'
Cmd $rep 2

# From here on the window is shown as a click-through ghost: layout needs a
# rendered window, and dialogs over a ghost are ghosts too (never on screen).
Show-Ghost $main

# Go To: a line past the end is refused in a second dialog, then line 3 works.
Cmd $main 25
$go = Wait-Top $p.Id 'RENotepadDialog' 'Go to line'
Check 'go to opens a Windows 11 dialog' ($go -ne [IntPtr]::Zero) $true
Check 'the dialog disables the main window' ([Q]::IsWindowEnabled($main)) $false
[Q]::SendMessageW([Q]::GetDlgItem($go, 258), 0x0C, [IntPtr]::Zero, '99') | Out-Null
Cmd $go 1
$err = [Q]::Owned($p.Id, 'RENotepadDialog', $go)
Check 'line past the end is refused' ($err -ne [IntPtr]::Zero) $true
Cmd $err 1
Check 'refusal closes, go to stays' ("$([Q]::Owned($p.Id, 'RENotepadDialog', $go) -eq [IntPtr]::Zero) $([Q]::IsWindowVisible($go))") 'True True'
[Q]::SendMessageW([Q]::GetDlgItem($go, 258), 0x0C, [IntPtr]::Zero, '3') | Out-Null
Cmd $go 1; Check 'go to line 3' (Sel $edit) '24-24'
Check 'main window enabled again' ([Q]::IsWindowEnabled($main)) $true

$r = New-Object int[] 4
[Q]::SendMessageW($edit, 0xB2, [IntPtr]0, $r) | Out-Null # EM_GETRECT
Check 'text inset 16 px' "$($r[0]),$($r[1])" "$(Px 16),$(Px 16)"
Cmd $main 40 # zoom in: the control resets its rect, the app must re-inset it
[Q]::SendMessageW($edit, 0xB2, [IntPtr]0, $r) | Out-Null
Check 'inset survives zoom' "$($r[0]),$($r[1])" "$(Px 16),$(Px 16)"
Cmd $main 42

# Scrollbars only when there is something to scroll.
function EditWiderThanClient {
  $er = New-Object int[] 4; $cr = New-Object int[] 4
  [Q]::GetWindowRect([Q]::VisibleEdit($main), $er) | Out-Null; [Q]::GetClientRect($main, $cr) | Out-Null
  ($er[2] - $er[0]) -gt $cr[2]
}
Check 'short text: vertical bar tucked away' (EditWiderThanClient) $true
[Q]::SendMessageW($edit, 0xB1, [IntPtr]0, [IntPtr]0) | Out-Null
Type-Text $edit ((1..200 | ForEach-Object { "row $_" }) -join "`r`n")
Check 'long text: vertical bar shown' (EditWiderThanClient) $false

# Tabs.
Cmd $main 1
Check 'Ctrl+N opens a tab' ([Q]::CountEdits($main)) 2
Check 'new tab is Untitled' (Title $main) 'Untitled - RENotepad'
Type-Text ([Q]::VisibleEdit($main)) 'scratch'
Check 'typing marks the tab' (Title $main) '*Untitled - RENotepad'
Cmd $main 12
Check 'Ctrl+Tab goes back' (Title $main) '*renotepad-smoke.txt - RENotepad'
Click 548 26 # the + button: right of two 240 px tabs starting at x=48
Check '+ button opens a tab' ([Q]::CountEdits($main)) 3
Click 140 26 0x207 0x208 # middle-click the first tab: modified, so it asks
$ask = Wait-Top $p.Id 'RENotepadDialog' 'RENotepad'
Check 'closing a modified tab asks' ($ask -ne [IntPtr]::Zero) $true
Check 'the owner is dimmed behind it' ([Q]::Top($p.Id, 'RENotepadSmoke', [NullString]::Value) -ne [IntPtr]::Zero) $true
[Q]::PostMessageW($ask, $WM_COMMAND, [IntPtr]7, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 500 # Don't save
Check "Don't save closes it" ([Q]::CountEdits($main)) 2

# Close everything: the scratch tab asks once, then the window goes and settings are saved.
[Q]::PostMessageW($main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$ask = Wait-Top $p.Id 'RENotepadDialog' 'RENotepad'
[Q]::PostMessageW($ask, $WM_COMMAND, [IntPtr]7, [IntPtr]::Zero) | Out-Null
$p.WaitForExit(5000) | Out-Null
Check 'window closes after the prompt' $p.HasExited $true
Check 'settings saved on close' (Test-Path $Key) $true

} finally {
  if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  Remove-Item $Key -Recurse -ErrorAction SilentlyContinue
  if ($hadKey) { reg import $Backup 2>&1 | Out-Null; Remove-Item $Backup }
  Remove-Item $File -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
