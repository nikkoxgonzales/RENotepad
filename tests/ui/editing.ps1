# Editing keys: multi-level undo and redo, Ctrl+D, Tab on lines, and undo kept
# across a word wrap toggle. The exe starts hidden and gets window messages only.
# Shift+Tab is not here: the app reads Shift from the keyboard state, which a
# message cannot set. The unit tests in src/lines.rs cover the outdent.
# Usage: pwsh tests/ui/editing.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
$script:failed = $false
$Dir = Join-Path $env:TEMP 'renotepad-editing'
New-Item -ItemType Directory -Force $Dir | Out-Null
$File = Join-Path $Dir 'cfg.json'
$Orig = "{`r`n  `"a`": 1,`r`n  `"b`": [2, 3]`r`n}"
[IO.File]::WriteAllText($File, $Orig)

function Text { $sb = New-Object Text.StringBuilder 4096; [void][Q]::SendMessageW($e, 0x0D, [IntPtr]4096, $sb); $sb.ToString() } # WM_GETTEXT
function Cmd($id) { [void][Q]::SendMessageW($main, $WM_COMMAND, [IntPtr]$id, [IntPtr]::Zero) }
function Char($c) { [void][Q]::SendMessageW($e, 0x102, [IntPtr][int]$c, [IntPtr]1) } # WM_CHAR
function Sel($s, $t) { [void][Q]::SendMessageW($e, 0xB1, [IntPtr]$s, [IntPtr]$t) } # EM_SETSEL
function Modified { [int][Q]::SendMessageW($e, 0xB8, [IntPtr]::Zero, [IntPtr]::Zero) } # EM_GETMODIFY
$UNDO = 16; $REDO = 28; $DUP = 29; $WRAP = 32

$p = Start-Process $Exe -ArgumentList "`"$File`"" -WindowStyle Hidden -PassThru
try {
  $main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 300
  $e = [Q]::VisibleEdit($main)

  # Two words typed after "1": two undo steps, then back at the loaded text.
  Sel 11 11
  foreach ($c in 'x y'.ToCharArray()) { Char $c }
  Check 'typed text is in' (Text).Contains('1x y,') $true
  Cmd $UNDO
  Check 'first undo takes back the second word' (Text).Contains('1x ,') $true
  Cmd $UNDO
  Check 'second undo gives the loaded text' (Text) $Orig
  Check 'undo to the loaded text clears the modified flag' (Modified) 0
  Cmd $REDO; Cmd $REDO
  Check 'redo twice puts both words back' (Text).Contains('1x y,') $true
  Check 'redo sets the modified flag' (Modified) 1
  Cmd $UNDO; Cmd $UNDO

  # Ctrl+D on line 2 copies it below.
  Sel 5 5
  Cmd $DUP
  Check 'Ctrl+D copies the caret line below' (Text) "{`r`n  `"a`": 1,`r`n  `"a`": 1,`r`n  `"b`": [2, 3]`r`n}"
  Cmd $UNDO
  Check 'undo takes back Ctrl+D' (Text) $Orig

  # Tab on a selection over lines 2 and 3 indents both by the file's unit (2 spaces).
  Sel 3 (($Orig.IndexOf(']')) + 1)
  Char 9
  Check 'Tab indents each selected line' (Text) "{`r`n    `"a`": 1,`r`n    `"b`": [2, 3]`r`n}"
  # A Tab with no selection in a space-indented file types spaces to the next
  # stop. The lines now step by 4, so at column 1 that is 3 spaces.
  Sel 1 1
  Char 9
  Check 'Tab with no selection types spaces to the next stop' (Text).StartsWith("{   `r`n") $true
  Cmd $UNDO; Cmd $UNDO
  Check 'undo takes back both Tabs' (Text) $Orig

  # The history stays with the tab when word wrap makes a new editor.
  Sel 0 0; Char ([char]'z')
  Cmd $WRAP; Start-Sleep -Milliseconds 200
  $e = [Q]::VisibleEdit($main)
  Cmd $UNDO
  Check 'undo works after a word wrap toggle' (Text) $Orig
  Cmd $WRAP
} finally {
  if ($e -and $e -ne [IntPtr]::Zero) { [void][Q]::SendMessageW($e, 0xB9, [IntPtr]0, [IntPtr]0) }
  if ($p -and -not $p.HasExited) {
    [void][Q]::PostMessageW($main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
    if (-not $p.WaitForExit(5000)) { Stop-Process -Id $p.Id -Force }
  }
  Remove-Item $Dir -Recurse -ErrorAction SilentlyContinue
  Remove-Item $TestKey -Recurse -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
