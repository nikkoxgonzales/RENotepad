# Jump list pins, end to end, on made-up jump lists only: RENOTEPAD_PINS_FROM /
# RENOTEPAD_PINS_TO point the app at throwaway app IDs, so Notepad's and
# RENotepad's real lists are never read or written.
# - /ImportNotepadPins pins the source's files in order and reports what it did
#   in its exit code (10 bits each: pinned, already pinned, missing).
# - A pinned file that no longer exists is skipped: a pin to it would only open
#   a "can't find" box. A second run pins nothing new.
# - /PinToJumpList pins one file; again says already pinned; a missing file isn't.
# - The gear menu import answers with a dialog; File > Pin to jump list pins the
#   open file.
# Usage: pwsh tests/ui/pins.ps1 [-Exe ...]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
. "$PSScriptRoot\jumplist.ps1"
$script:failed = $false

$stamp = [guid]::NewGuid().ToString('N').Substring(0, 8)
$from = "RENotepad.Test.PinsFrom.$stamp"
$to = "RENotepad.Test.PinsTo.$stamp"
$dir = Join-Path ([IO.Path]::GetTempPath()) "renotepad-pins-$stamp"
New-Item -ItemType Directory $dir | Out-Null
$files = 'b first.txt', 'a second.log', 'gone.txt', 'script.ps1' | % { $p = Join-Path $dir $_; Set-Content $p 'x'; $p }
$extra = Join-Path $dir 'open me.txt'; Set-Content $extra 'x'

function Code($pinned, $already, $missing) { $pinned -bor ($already -shl 10) -bor ($missing -shl 20) }
function Run([string[]]$argv) {
  $env:RENOTEPAD_PINS_FROM = "$from;RENotepad.Test.NoSuchList.$stamp"
  $env:RENOTEPAD_PINS_TO = $to
  (Start-Process $Exe -ArgumentList $argv -PassThru -Wait -WindowStyle Hidden).ExitCode
}

$p = $null
try {
  foreach ($f in $files) { [Pins.L]::Pin($from, $f) }
  Remove-Item $files[2] # pinned, then deleted
  Check 'import: 3 pinned, 1 missing skipped' (Run '/ImportNotepadPins') (Code 3 0 1)
  Check 'pinned in the same order' (([Pins.L]::Pinned($to) | Split-Path -Leaf) -join ',') 'b first.txt,a second.log,script.ps1'
  Check 'second import pins nothing new' (Run '/ImportNotepadPins') (Code 0 3 1)
  Check 'source list untouched' ([Pins.L]::Pinned($from).Count) 4

  Check 'pin one file' (Run '/PinToJumpList', "`"$extra`"") (Code 1 0 0)
  Check 'it is last in the list' (([Pins.L]::Pinned($to) | Select-Object -Last 1) -eq $extra) $true
  Check 'pin it again: already pinned' (Run '/PinToJumpList', "`"$extra`"") (Code 0 1 0)
  Check 'pin a missing file: not pinned' (Run '/PinToJumpList', "`"$($files[2])`"") (Code 0 0 1)

  # The menus run the same code in a child process.
  [Pins.L]::Clear($to)
  $p = Start-Process $Exe -ArgumentList "`"$extra`"" -WindowStyle Hidden -PassThru
  $main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value)
  Show-Ghost $main
  Cmd $main 52 # File > Pin to jump list
  $deadline = (Get-Date).AddSeconds(10); while ([Pins.L]::Pinned($to).Count -lt 1 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  Check 'File > Pin to jump list pins the open file' (([Pins.L]::Pinned($to)) -join ',') $extra
  Cmd $main 51 # Settings > Import pinned files from Notepad
  $dlg = Wait-Top $p.Id 'RENotepadDialog' 'Import pinned files from Notepad'
  Check 'menu import answers with a dialog' ($dlg -ne [IntPtr]::Zero) $true
  Check 'menu import pinned them' ([Pins.L]::Pinned($to).Count) 4
  Cmd $dlg 1
} finally {
  $env:RENOTEPAD_PINS_FROM = $null; $env:RENOTEPAD_PINS_TO = $null
  if ($p) { Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force }
  [Pins.L]::Clear($from); [Pins.L]::Clear($to)
  # Each list is a file named by the ID's hash; delete the two test ones.
  $ad = Join-Path $env:APPDATA 'Microsoft\Windows\Recent\AutomaticDestinations'
  foreach ($id in $from, $to) { Remove-Item (Join-Path $ad "$([Pins.L]::FileName($id)).automaticDestinations-ms") -ErrorAction SilentlyContinue }
  Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
