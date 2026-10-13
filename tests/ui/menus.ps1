# Menu-bar keyboard navigation: Alt+F opens File, Right/Left walk the bar and
# wrap around to the settings gear, Escape closes. A real popup menu has to
# open for this, so one flashes near the top-left of the screen for about a
# second. It never takes focus.
# Usage: pwsh tests/ui/menus.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
Add-Type -Name M -Namespace Z -MemberDefinition '[DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);'
$script:failed = $false

$p = Start-Process $Exe -WindowStyle Hidden -PassThru
$main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 300
Show-Ghost $main

# Items in the open popup: File 13, Edit 17, View 3, settings 6.
function OpenMenuItems {
  Start-Sleep -Milliseconds 400
  $pop = [Q]::Top($p.Id, '#32768', [NullString]::Value)
  if ($pop -eq [IntPtr]::Zero -or -not [Q]::IsWindowVisible($pop)) { return 'none' }
  [Z.M]::GetMenuItemCount([Q]::SendMessageW($pop, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)) # MN_GETHMENU
}
function Key($vk) { [Q]::PostMessageW($main, 0x100, [IntPtr]$vk, [IntPtr]::Zero) | Out-Null }

[Q]::PostMessageW($main, 0x112, [IntPtr]0xF100, [IntPtr][int][char]'f') | Out-Null # SC_KEYMENU, Alt+F
Check 'Alt+F opens File' (OpenMenuItems) 15
Key 0x27; Check 'Right moves to Edit' (OpenMenuItems) 17
Key 0x27; Check 'Right moves to View' (OpenMenuItems) 3
Key 0x25; Key 0x25; Check 'Left twice is back on File' (OpenMenuItems) 15
Key 0x25; Check 'Left from File wraps to the gear' (OpenMenuItems) 8
Key 0x1B; Check 'Escape closes' (OpenMenuItems) 'none'
[Q]::PostMessageW($main, 0x1F, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null # WM_CANCELMODE, in case

Stop-Process -Id $p.Id -Force
if ($script:failed) { exit 1 }
