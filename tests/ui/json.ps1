# JSON: a minified .json opens formatted and not marked as changed, Edit ›
# Format JSON formats as one undo step, text that is not JSON stays as it is,
# a .jsonl file never changes, and the gear setting turns the format on open off.
# The exe starts hidden and gets window messages only.
# Usage: pwsh tests/ui/json.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
$script:failed = $false
$Dir = Join-Path $env:TEMP 'renotepad-json'
New-Item -ItemType Directory -Force $Dir | Out-Null
$UNDO = 16; $FORMAT = 30
Remove-Item $TestKey -Recurse -ErrorAction SilentlyContinue # a setting left by an earlier run

function Text { $sb = New-Object Text.StringBuilder 4096; [void][Q]::SendMessageW($e, 0x0D, [IntPtr]4096, $sb); $sb.ToString() } # WM_GETTEXT
function Cmd($id) { [void][Q]::SendMessageW($main, $WM_COMMAND, [IntPtr]$id, [IntPtr]::Zero) }
function Modified { [int][Q]::SendMessageW($e, 0xB8, [IntPtr]::Zero, [IntPtr]::Zero) } # EM_GETMODIFY
function Launch($name, $content) {
  $f = Join-Path $Dir $name
  [IO.File]::WriteAllText($f, $content)
  $script:p = Start-Process $Exe -ArgumentList "`"$f`"" -WindowStyle Hidden -PassThru
  $script:main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 300
  $script:e = [Q]::VisibleEdit($main)
}
function Quit {
  [void][Q]::SendMessageW($e, 0xB9, [IntPtr]0, [IntPtr]0) # EM_SETMODIFY: no save prompt
  [void][Q]::PostMessageW($main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
  if (-not $p.WaitForExit(5000)) { Stop-Process -Id $p.Id -Force }
}

$Min = '{"a":1,"b":[true,{"c":"x"}],"d":[]}'
$Pretty = "{`r`n  `"a`": 1,`r`n  `"b`": [`r`n    true,`r`n    {`r`n      `"c`": `"x`"`r`n    }`r`n  ],`r`n  `"d`": []`r`n}"
try {
  Launch 'min.json' $Min
  Check 'a minified .json opens formatted' (Text) $Pretty
  Check 'the formatted file is not marked as changed' (Modified) 0
  Quit

  $Odd = "{`"a`":`r`n1,  `"b`" :[2]}"
  Launch 'odd.json' $Odd
  Check 'a .json with line breaks opens as it is' (Text) $Odd
  Cmd $FORMAT
  Check 'Format JSON formats it' (Text) "{`r`n  `"a`": 1,`r`n  `"b`": [`r`n    2`r`n  ]`r`n}"
  Check 'Format JSON marks it as changed' (Modified) 1
  Cmd $UNDO
  Check 'one undo takes back Format JSON' (Text) $Odd
  Quit

  $Bad = '{"a":1,}'
  Launch 'bad.json' $Bad
  Check 'a minified file that is not JSON opens as it is' (Text) $Bad
  Cmd $FORMAT
  Check 'Format JSON leaves text that is not JSON' (Text) $Bad
  Check 'the caret goes to where it stops being JSON' (Sel $e) '7-7'
  Quit

  Launch 'rows.jsonl' '{"a":1}'
  Check 'a .jsonl file opens as it is' (Text) '{"a":1}'
  Quit

  New-Item $TestKey -Force | Out-Null
  Set-ItemProperty $TestKey fFormatJson 0 -Type DWord
  Launch 'off.json' $Min
  Check 'with the setting off, a minified .json opens as it is' (Text) $Min
  Quit
} finally {
  if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  Remove-Item $Dir -Recurse -ErrorAction SilentlyContinue
  Remove-Item $TestKey -Recurse -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
