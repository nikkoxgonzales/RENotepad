# Fitting the scrollbars must not recurse: with word wrap on, resizing the
# editor makes it send EN_VSCROLL back mid-layout, and refitting from there
# used to flip the bar back and forth until the stack overflowed (0xc000041d).
# Sweeps the window height 1 px at a time over documents that sat on that
# edge, typing at the end each step; the process must survive and the vertical
# bar must be on screen exactly when there is something to scroll.
# Usage: pwsh tests/ui/scrollbars.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
Add-Type -Name S -Namespace Z -MemberDefinition '[DllImport("user32.dll")] public static extern bool GetScrollInfo(IntPtr h, int bar, int[] si);'

$Key = $TestKey
$script:failed = $false
try {

# These lengths both crashed 0.4.0 at 1084 px wide (at heights 421 and 565).
foreach ($len in 1500, 2500) {
  New-Item $Key -Force | Out-Null
  Set-ItemProperty $Key fWrap 1 -Type DWord
  $p = Start-Process $Exe -WindowStyle Hidden -PassThru
  $main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 400
  Show-Ghost $main 1084 936
  $edit = [Q]::VisibleEdit($main)
  # Note-like text: short words, lines of 5-60 of them. Seeded, so the same every run.
  $rng = New-Object Random $len; $sb = New-Object Text.StringBuilder
  while ($sb.Length -lt $len) {
    $n = $rng.Next(5, 60)
    for ($i = 0; $i -lt $n; $i++) { [void]$sb.Append(-join (1..$rng.Next(1, 10) | % { [char]$rng.Next(97, 123) })); [void]$sb.Append(' ') }
    [void]$sb.Append("`r`n")
  }
  [Q]::SendMessageW($edit, 0xC2, [IntPtr]1, $sb.ToString()) | Out-Null # EM_REPLACESEL
  $died = 'no'; $mismatch = 0
  for ($h = 250; $h -le 936; $h++) {
    [Q]::SendMessageW($edit, 0xB1, [IntPtr](-1), [IntPtr](-1)) | Out-Null # caret to the end
    [Q]::SetWindowPos($main, [IntPtr]::Zero, 0, 0, 1084, $h, 0x0002 -bor 0x0004 -bor 0x0010) | Out-Null # NOMOVE|NOZORDER|NOACTIVATE
    foreach ($c in 'a', ' ', "`b", "`b") { [Q]::SendMessageW($edit, 0x0102, [IntPtr][int][char]$c, [IntPtr]1) | Out-Null } # WM_CHAR
    if ($p.HasExited) { $died = "at height $h, exit 0x{0:x8}" -f $p.ExitCode; break }
    $si = New-Object int[] 7; $si[0] = 28; $si[1] = 0x17 # SCROLLINFO, SIF_ALL
    [Z.S]::GetScrollInfo($edit, 1, $si) | Out-Null # SB_VERT
    $need = $si[4] -gt 0 -and (($si[3] - $si[2] + 1) -gt $si[4] -or $si[5] -gt $si[2])
    $er = New-Object int[] 4; $cr = New-Object int[] 4
    [Q]::GetWindowRect($edit, $er) | Out-Null; [Q]::GetClientRect($main, $cr) | Out-Null
    # An unneeded bar is pushed past the right edge, so the editor is wider than the client.
    if ($need -ne (($er[2] - $er[0]) -le $cr[2])) { $mismatch++ }
  }
  Check "$len chars: survives the height sweep" $died 'no'
  Check "$len chars: vertical bar shown exactly when needed" $mismatch 0
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  Start-Sleep -Milliseconds 300
}

} finally {
  Remove-Item $Key -Recurse -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
