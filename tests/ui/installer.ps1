# The installer, end to end: silent per-user install, a reinstall, then
# uninstall. Runs only when RENotepad isn't installed for this user, so it never
# touches a real install. Notepad's pins are imported from a made-up jump list
# (RENOTEPAD_PINS_FROM / RENOTEPAD_PINS_TO), never the real ones.
# - Setup imports the pins once; a reinstall doesn't offer it again.
# - The Start menu shortcut and the RENotepad.txt ProgID carry the app ID.
# - Uninstall removes the app and every registry entry Setup made.
# HKCU\Software\RENotepad (the real settings) gets the import marker from Setup;
# the test puts it back the way it was.
# Usage: pwsh tests/ui/installer.ps1 [-Setup dist\RENotepad-x.y.z-setup.exe]
param([string]$Setup = (Get-ChildItem (Join-Path $PSScriptRoot '..\..\dist') -Filter 'RENotepad-*-setup.exe' | Select-Object -First 1).FullName)
. "$PSScriptRoot\lib.ps1"
. "$PSScriptRoot\jumplist.ps1"
$script:failed = $false

$app = Join-Path $env:LOCALAPPDATA 'Programs\RENotepad'
if (Test-Path $app) { Write-Host "RENotepad is installed for this user ($app); not touching it."; exit 2 }
if (-not $Setup -or -not (Test-Path $Setup)) { Write-Host 'No setup found: run scripts/package.ps1 first.'; exit 2 }

$exts = 'txt', 'log', 'ini', 'cfg', 'conf', 'md', 'csv', 'tsv', 'json', 'xml', 'yaml', 'yml', 'toml', 'nfo', 'srt'
function ExtState { foreach ($e in $exts) { reg query "HKCU\Software\Classes\.$e" /s 2>&1 | Out-String } }
$extBefore = ExtState
$marker = (Get-ItemProperty HKCU:\Software\RENotepad -ErrorAction SilentlyContinue).fNotepadPinsImported

$stamp = [guid]::NewGuid().ToString('N').Substring(0, 8)
$from = "RENotepad.Test.SetupFrom.$stamp"; $to = "RENotepad.Test.SetupTo.$stamp"
$dir = Join-Path ([IO.Path]::GetTempPath()) "renotepad-setup-$stamp"
New-Item -ItemType Directory $dir | Out-Null
$f1 = Join-Path $dir 'pin one.txt'; $f2 = Join-Path $dir 'pin two.md'; Set-Content $f1 x; Set-Content $f2 x

function Install {
  $p = Start-Process $Setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/CURRENTUSER', '/NORESTART' -PassThru -Wait
  $p.ExitCode
}

try {
  [Pins.L]::Pin($from, $f1); [Pins.L]::Pin($from, $f2)
  $env:RENOTEPAD_PINS_FROM = $from; $env:RENOTEPAD_PINS_TO = $to
  Remove-ItemProperty HKCU:\Software\RENotepad -Name fNotepadPinsImported -ErrorAction SilentlyContinue
  Check 'install exit code' (Install) 0
  Check 'setup imported the pins' (([Pins.L]::Pinned($to) | Split-Path -Leaf) -join ',') 'pin one.txt,pin two.md'
  Check 'import marker written' ((Get-ItemProperty HKCU:\Software\RENotepad).fNotepadPinsImported) 1
  $lnk = (New-Object -ComObject Shell.Application).Namespace((Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs')).ParseName('RENotepad.lnk')
  Check 'Start menu shortcut app ID' $lnk.ExtendedProperty('System.AppUserModel.ID') 'NikkoGonzales.RENotepad'
  Check 'ProgID app ID' ((Get-ItemProperty 'HKCU:\Software\Classes\RENotepad.txt').AppUserModelID) 'NikkoGonzales.RENotepad'
  [Pins.L]::Clear($to)
  Check 'reinstall exit code' (Install) 0
  Check 'reinstall does not import again' ([Pins.L]::Pinned($to).Count) 0
} finally {
  $env:RENOTEPAD_PINS_FROM = $null; $env:RENOTEPAD_PINS_TO = $null
  if (Test-Path "$app\unins000.exe") {
    Start-Process "$app\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait
    Start-Sleep -Seconds 3 # the uninstaller finishes from a temp copy
  }
  if ($null -eq $marker) { Remove-ItemProperty HKCU:\Software\RENotepad -Name fNotepadPinsImported -ErrorAction SilentlyContinue }
  else { Set-ItemProperty HKCU:\Software\RENotepad -Name fNotepadPinsImported -Value $marker }
  [Pins.L]::Clear($from); [Pins.L]::Clear($to)
  $ad = Join-Path $env:APPDATA 'Microsoft\Windows\Recent\AutomaticDestinations'
  foreach ($id in $from, $to) { Remove-Item (Join-Path $ad "$([Pins.L]::FileName($id)).automaticDestinations-ms") -ErrorAction SilentlyContinue }
  Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}
Check 'uninstall removed the app' (Test-Path $app) $false
foreach ($k in 'Software\Classes\RENotepad.txt', 'Software\Classes\Applications\RENotepad.exe', 'Software\Nikko Gonzales', 'Software\Microsoft\Windows\CurrentVersion\App Paths\RENotepad.exe') {
  Check "uninstall removed HKCU\$k" (Test-Path "HKCU:\$k") $false
}
Check 'file types exactly as before' ((ExtState) -join '') ($extBefore -join '')
if ($script:failed) { exit 1 }
