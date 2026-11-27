# Wall-clock times of the user journeys, focus-free (the exe starts hidden).
# Each journey runs -Runs times. The script prints the median and the minimum in ms.
# Usage: pwsh tests/perf/journeys.ps1 [-Exe target\release\RENotepad.exe] [-Runs 7] [-Json out.json]
# Journeys:
#   start-empty    process start until the message loop waits for input (typeable)
#   start-5mb      the same with a 5 MB text file on the command line
#   preview-md     View > Markdown preview on a 200 KB README (SendMessage returns when rendered)
#   preview-img    View > Markdown preview on a file with a 3 MB PNG
#   type-5mb       one typed character in the 5 MB file (mean of 200)
#   tab-switch     Ctrl+Tab between the 5 MB file and an empty tab (mean of 50)
# The window is hidden, so no journey counts the first paint. It still starts the
# exe many times: run it on a spare test PC, not on a PC that someone works on.
param(
  [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\RENotepad.exe'),
  [int]$Runs = 7,
  [string]$Json
)
. "$PSScriptRoot\..\ui\lib.ps1"
$Exe = (Resolve-Path $Exe).Path

$Dir = Join-Path $env:TEMP 'renotepad-perf'
New-Item -ItemType Directory -Force $Dir | Out-Null
$Big = Join-Path $Dir 'big.txt'
$Md = Join-Path $Dir 'README.md'
$ImgMd = Join-Path $Dir 'picture.md'
$Png = Join-Path $Dir 'noise.png'

# Inputs are made once and kept, so each run reads the same bytes.
if (-not (Test-Path $Big)) {
  $line = 'The quick brown fox jumps over the lazy dog 0123456789 ' * 1
  $sb = New-Object Text.StringBuilder
  $i = 0
  while ($sb.Length -lt 5MB) { [void]$sb.Append("$i $line`r`n"); $i++ }
  [IO.File]::WriteAllText($Big, $sb.ToString())
}
if (-not (Test-Path $Md)) {
  $section = @'
## Section {0}

Some **bold**, *italic*, `code` and a [link](https://example.com/{0}). A plain URL: https://example.com/x/{0} :smile:

- item one
- [x] done item
  - nested item with `inline code`

> [!NOTE]
> A note with **emphasis**.

| Name | Value | Notes |
|------|------:|-------|
| alpha | {0} | first |
| beta | 2 | second row with more text |

```rust
fn main() {{
    let x = {0};
    println!("hello {{}}", x); // comment
}}
```

1. first
2. second

---

'@
  $sb = New-Object Text.StringBuilder
  [void]$sb.Append("---`ntitle: Perf`ntags: [a, b]`n---`n# Title`n`n")
  $k = 0
  while ($sb.Length -lt 200KB) { [void]$sb.Append(($section -f $k)); $k++ }
  [IO.File]::WriteAllText($Md, $sb.ToString())
}
if (-not (Test-Path $Png)) {
  $bmp = New-Object Drawing.Bitmap 1000, 1000
  $rnd = New-Object Random 1
  for ($y = 0; $y -lt 1000; $y++) { for ($x = 0; $x -lt 1000; $x += 4) { $bmp.SetPixel($x, $y, [Drawing.Color]::FromArgb($rnd.Next(16777216) -bor 0xFF000000)) } }
  $bmp.Save($Png, [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  [IO.File]::WriteAllText($ImgMd, "# Picture`n`n![noise](noise.png)`n")
}

Add-Type -Name Snd -Namespace Perf -MemberDefinition '[DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);'
$sw = [Diagnostics.Stopwatch]

function Start-App($file) {
  $args_ = if ($file) { "`"$file`"" } else { '' }
  $t = $sw::StartNew()
  $p = if ($args_) { Start-Process $Exe -ArgumentList $args_ -WindowStyle Hidden -PassThru } else { Start-Process $Exe -WindowStyle Hidden -PassThru }
  [void]$p.WaitForInputIdle(30000)
  $ms = $t.Elapsed.TotalMilliseconds
  $main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value)
  [pscustomobject]@{ P = $p; Main = $main; Ms = $ms }
}
function Stop-App($a) {
  # Discard edits: a modified tab would prompt on WM_CLOSE.
  $e = [Q]::VisibleEdit($a.Main)
  if ($e -ne [IntPtr]::Zero) { [void][Perf.Snd]::SendMessageW($e, 0xB9, [IntPtr]0, [IntPtr]0) } # EM_SETMODIFY 0
  [void][Q]::PostMessageW($a.Main, $WM_CLOSE, [IntPtr]0, [IntPtr]0)
  if (-not $a.P.WaitForExit(5000)) { $a.P.Kill() }
}
function Cmd-Sync($h, $id) {
  $t = $sw::StartNew()
  [void][Perf.Snd]::SendMessageW($h, $WM_COMMAND, [IntPtr]$id, [IntPtr]::Zero)
  $t.Elapsed.TotalMilliseconds
}

$results = [ordered]@{}
function Add($name, $ms) { if (-not $results.Contains($name)) { $results[$name] = New-Object Collections.Generic.List[double] }; $results[$name].Add($ms) }

# One warm-up start, so the first measured run does not pay for a cold disk cache.
Stop-App (Start-App $null)

for ($r = 0; $r -lt $Runs; $r++) {
  $a = Start-App $null; Add 'start-empty' $a.Ms; Stop-App $a

  $a = Start-App $Big; Add 'start-5mb' $a.Ms
  $e = [Q]::VisibleEdit($a.Main)
  [void][Perf.Snd]::SendMessageW($e, 0xB1, [IntPtr]1000, [IntPtr]1000) # EM_SETSEL into the text
  $t = $sw::StartNew()
  for ($i = 0; $i -lt 200; $i++) { [void][Perf.Snd]::SendMessageW($e, 0x102, [IntPtr][int][char]'a', [IntPtr]1) } # WM_CHAR
  Add 'type-5mb' ($t.Elapsed.TotalMilliseconds / 200)
  [void][Perf.Snd]::SendMessageW($a.Main, $WM_COMMAND, [IntPtr]1, [IntPtr]::Zero) # New tab
  $t = $sw::StartNew()
  for ($i = 0; $i -lt 50; $i++) { [void][Perf.Snd]::SendMessageW($a.Main, $WM_COMMAND, [IntPtr]12, [IntPtr]::Zero) } # Next tab
  Add 'tab-switch' ($t.Elapsed.TotalMilliseconds / 50)
  # Two tabs, one modified: discard both before close.
  foreach ($k in 0..1) { $e = [Q]::VisibleEdit($a.Main); [void][Perf.Snd]::SendMessageW($e, 0xB9, [IntPtr]0, [IntPtr]0); [void][Perf.Snd]::SendMessageW($a.Main, $WM_COMMAND, [IntPtr]12, [IntPtr]::Zero) }
  Stop-App $a

  $a = Start-App $Md; Add 'preview-md' (Cmd-Sync $a.Main 54); Stop-App $a
  $a = Start-App $ImgMd; Add 'preview-img' (Cmd-Sync $a.Main 54); Stop-App $a
}

$table = foreach ($k in $results.Keys) {
  $v = $results[$k] | Sort-Object
  [pscustomobject]@{ journey = $k; median_ms = [math]::Round($v[[int][math]::Floor($v.Count / 2)], 2); min_ms = [math]::Round($v[0], 2); runs = $v.Count }
}
$table | Format-Table -AutoSize
if ($Json) { $table | ConvertTo-Json | Set-Content $Json }
Remove-Item $TestKey -Recurse -ErrorAction SilentlyContinue
