# Update check, end to end (needs the internet). RENOTEPAD_UPDATE_REPO points
# the check at another repo: one that has releases newer than ours, and one
# that doesn't exist.
# - The startup check stays quiet: no dialog, only a status-bar line
#   (saved to -Out for a look).
# - Settings > Check for updates always answers with a dialog.
# Usage: pwsh tests/ui/update.ps1 [-Exe ...] [-Out status.png]
param(
  [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'),
  [string]$Out
)
. "$PSScriptRoot\lib.ps1"
Add-Type -Name Dpi -Namespace Z -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);'
$script:failed = $false

function Launch($repo) {
  $env:RENOTEPAD_UPDATE_REPO = $repo
  $p = Start-Process $Exe -WindowStyle Hidden -PassThru
  $env:RENOTEPAD_UPDATE_REPO = $null
  $main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value)
  Show-Ghost $main
  $p, $main
}

try {
  $p, $main = Launch 'microsoft/terminal'
  Start-Sleep -Seconds 4 # the startup check runs in the background
  Check 'startup check opens no dialog' ([Q]::Top($p.Id, 'RENotepadDialog', [NullString]::Value) -eq [IntPtr]::Zero) $true
  if ($Out) { Snap $main $Out }
  # Clicking the status-bar notice runs the same check as the menu.
  $status = [IntPtr]::Zero
  [Q]::EnumChildWindows($main, { param($h, $l) $c = New-Object Text.StringBuilder 64; [Q]::GetClassNameW($h, $c, 64) | Out-Null
    if ($c.ToString() -eq 'RENotepadStatus') { $script:status = $h; return $false }; $true }, [IntPtr]::Zero) | Out-Null
  $scale = [Z.Dpi]::GetDpiForWindow($main) / 96.0
  [Q]::PostMessageW($script:status, 0x202, [IntPtr]::Zero, [IntPtr]([int](16 * $scale) -shl 16 -bor [int](340 * $scale))) | Out-Null
  $dlg = Wait-Top $p.Id 'RENotepadDialog' 'Update available'
  Check 'clicking the notice offers the download' ($dlg -ne [IntPtr]::Zero) $true
  Cmd $dlg 2
  Cmd $main 49
  $dlg = Wait-Top $p.Id 'RENotepadDialog' 'Update available'
  Check 'menu check offers the download' ($dlg -ne [IntPtr]::Zero) $true
  Cmd $dlg 2 # Later
  Check 'Later closes it' ([Q]::Top($p.Id, 'RENotepadDialog', [NullString]::Value) -eq [IntPtr]::Zero) $true
  Stop-Process -Id $p.Id -Force

  $p, $main = Launch 'nikkoxgonzales/this-repo-does-not-exist'
  Cmd $main 49
  $dlg = Wait-Top $p.Id 'RENotepadDialog' 'Check for updates'
  Check 'no release: says it could not check' ($dlg -ne [IntPtr]::Zero) $true
  Cmd $dlg 1
} finally {
  Get-Process -Id $p.Id -ErrorAction SilentlyContinue | Stop-Process -Force
}
if ($script:failed) { exit 1 }
