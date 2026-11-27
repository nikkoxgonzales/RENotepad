# File > Recent files, end to end. The first-start import reads a made-up jump
# list only (RENOTEPAD_PINS_FROM), so Notepad's real list is never read.
# - The first start adds the list's recent files below RENotepad's own entry,
#   and skips a file that no longer exists. A second start imports nothing.
# - Opening a file puts it on top. A recent entry opens from the menu command.
# - A recent entry for a missing file is removed when picked. Clear empties the list.
# Usage: pwsh tests/ui/recent.ps1 [-Exe ...]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
. "$PSScriptRoot\jumplist.ps1"
$script:failed = $false

$stamp = [guid]::NewGuid().ToString('N').Substring(0, 8)
$from = "RENotepad.Test.RecentFrom.$stamp"
$dir = Join-Path ([IO.Path]::GetTempPath()) "renotepad-recent-$stamp"
New-Item -ItemType Directory $dir | Out-Null
$a, $b, $gone, $own = 'a.txt', 'b.txt', 'gone.txt', 'own.txt' | % { $p = Join-Path $dir $_; Set-Content $p 'x'; $p }

$Key = $TestKey
function Recent { @((Get-ItemProperty $Key -EA SilentlyContinue).RecentFiles) | ? { $_ } }
# Not New-Item -Force on an existing key: that recreates it and drops fRecentImported.
function Set-Recent([string[]]$list) { if (-not (Test-Path $Key)) { New-Item $Key | Out-Null }; New-ItemProperty $Key RecentFiles -PropertyType MultiString -Value $list -Force | Out-Null }
function Start-App([string[]]$argv) {
  $env:RENOTEPAD_PINS_FROM = $from
  $p = if ($argv) { Start-Process $Exe -ArgumentList $argv -WindowStyle Hidden -PassThru } else { Start-Process $Exe -WindowStyle Hidden -PassThru }
  $main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value)
  return $p, $main
}
function Wait-Until([scriptblock]$cond) { for ($i = 0; $i -lt 50 -and -not (& $cond); $i++) { Start-Sleep -Milliseconds 200 } }

$p = $null
try {
  foreach ($f in $a, $b, $gone) { [Pins.L]::Use($from, $f) }
  Remove-Item $gone # used, then deleted
  Set-Recent @($own)

  $p, $main = Start-App
  Wait-Until { (Get-ItemProperty $Key -EA SilentlyContinue).fRecentImported -eq 1 -and (Recent).Count -gt 1 }
  $got = Recent
  Check 'import: own entry stays on top' $got[0] $own
  Check 'import: adds the 2 files that exist' (($got | Select -Skip 1 | Split-Path -Leaf | Sort) -join ',') 'a.txt,b.txt'
  Check 'import: marks itself done' (Get-ItemProperty $Key).fRecentImported 1
  Stop-Process -Id $p.Id -Force; Start-Sleep -Milliseconds 300

  Set-Recent @($own)
  $p, $main = Start-App
  Start-Sleep -Seconds 3
  Check 'second start imports nothing' ((Recent) -join ',') $own

  Cmd $main 100 # IDM_RECENT + 0
  Check 'recent entry opens the file' ((Title $main) -like 'own.txt*') $true
  Stop-Process -Id $p.Id -Force; Start-Sleep -Milliseconds 300

  $p, $main = Start-App @("`"$a`"")
  Start-Sleep -Milliseconds 500
  Check 'opening a file puts it on top' ((Recent)[0]) $a
  Check 'no duplicate after reopening' ((Recent | ? { $_ -eq $a }).Count) 1

  $missing = Join-Path $dir 'missing.txt'
  Set-Recent @($missing, $own)
  Cmd $main 100 # opens the error box; the entry is removed before it shows
  Wait-Until { (Recent) -notcontains $missing }
  Check 'missing file is removed when picked' ((Recent) -join ',') $own
  Stop-Process -Id $p.Id -Force; Start-Sleep -Milliseconds 300

  $p, $main = Start-App
  Cmd $main 53 # IDM_CLEAR_RECENT
  Check 'clear empties the list' (Recent).Count 0
} finally {
  if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  Start-Sleep -Milliseconds 300
  [Pins.L]::ClearRecent($from)
  $file = Join-Path $env:APPDATA "Microsoft\Windows\Recent\AutomaticDestinations\$([Pins.L]::FileName($from)).automaticDestinations-ms"
  Remove-Item $file -EA SilentlyContinue
  Remove-Item $dir -Recurse -EA SilentlyContinue
  Remove-Item $Key -Recurse -EA SilentlyContinue
}
if ($script:failed) { exit 1 }
