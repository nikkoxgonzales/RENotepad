# Folder pane, focus-free. It proves:
# - A folder on the command line opens in the pane. Folders come first, then
#   files in Explorer number order. Hidden files do not show.
# - Enter on a file opens it in a tab, and the pane shows that file in bold.
# - Right arrow on a folder lists it (lazy load).
# - A file made or deleted on disk shows or goes without a refresh (watcher).
# - Rename in place renames the file on disk and the title of its open tab.
# - New file makes "New Text Document.txt" and starts its rename.
# - Delete moves a file to the Recycle Bin, and its open tab becomes modified.
# - View > Folder pane hides and shows the pane. A narrow window hides it.
# - A splitter drag changes the pane width.
# - The pane, its root and its width come back at the next start. Close folder hides it.
# The rename needs an active window (the label editor ends when it has no
# focus), so this script shows a normal window. Run it on the test PC.
# Usage: pwsh tests/ui/folderpane.ps1 [-Exe target\release\RENotepad.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\RENotepad.exe'))
. "$PSScriptRoot\lib.ps1"
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class T {
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern IntPtr VirtualAllocEx(IntPtr p, IntPtr a, IntPtr size, uint type, uint protect);
  [DllImport("kernel32.dll")] static extern bool VirtualFreeEx(IntPtr p, IntPtr a, IntPtr size, uint type);
  [DllImport("kernel32.dll")] static extern bool WriteProcessMemory(IntPtr p, IntPtr a, byte[] b, IntPtr n, out IntPtr done);
  [DllImport("kernel32.dll")] static extern bool ReadProcessMemory(IntPtr p, IntPtr a, byte[] b, IntPtr n, out IntPtr done);
  [DllImport("user32.dll")] static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, Cb cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, int[] r);
  delegate bool Cb(IntPtr h, IntPtr l);
  public static IntPtr Tree(IntPtr main) {
    IntPtr f = IntPtr.Zero;
    EnumChildWindows(main, (h, l) => { var c = new StringBuilder(64); GetClassNameW(h, c, 64);
      if (c.ToString() == "SysTreeView32") { f = h; return false; } return true; }, IntPtr.Zero);
    return f;
  }
  public static bool Visible(IntPtr h) { return (GetWindowLongPtrW(h, -16).ToInt64() & 0x10000000) != 0; }
  public static int Width(IntPtr h) { var r = new int[4]; GetWindowRect(h, r); return r[2] - r[0]; }
  public static IntPtr Next(IntPtr tree, IntPtr item, int code) { return SendMessageW(tree, 0x110A, (IntPtr)code, item); }
  public static int State(IntPtr tree, IntPtr item) { return (int)SendMessageW(tree, 0x1127, item, (IntPtr)0xFF); }
  /// The text of a tree item, read through a buffer in the app's process.
  public static string Text(IntPtr tree, uint pid, IntPtr item) {
    IntPtr p = OpenProcess(0x0008 | 0x0010 | 0x0020, false, pid); // VM_OPERATION | VM_READ | VM_WRITE
    IntPtr mem = VirtualAllocEx(p, IntPtr.Zero, (IntPtr)4096, 0x3000, 0x04);
    var tv = new byte[56]; IntPtr done;
    BitConverter.GetBytes(0x11).CopyTo(tv, 0);                         // TVIF_TEXT | TVIF_HANDLE
    BitConverter.GetBytes(item.ToInt64()).CopyTo(tv, 8);               // hItem
    BitConverter.GetBytes(mem.ToInt64() + 64).CopyTo(tv, 24);          // pszText
    BitConverter.GetBytes(260).CopyTo(tv, 32);                         // cchTextMax
    WriteProcessMemory(p, mem, tv, (IntPtr)56, out done);
    SendMessageW(tree, 0x113E, IntPtr.Zero, mem);                      // TVM_GETITEMW
    var buf = new byte[520]; ReadProcessMemory(p, mem + 64, buf, (IntPtr)520, out done);
    VirtualFreeEx(p, mem, IntPtr.Zero, 0x8000); CloseHandle(p);
    var s = Encoding.Unicode.GetString(buf); int z = s.IndexOf('\0'); return z < 0 ? s : s.Substring(0, z);
  }
}
'@
$TVGN_ROOT, $TVGN_NEXT, $TVGN_CHILD, $TVGN_CARET = 0, 1, 4, 9
$TVIS_BOLD, $TVIS_EXPANDED = 0x10, 0x20
$WM_KEYDOWN = 0x100

$Dir = Join-Path $env:TEMP "renotepad-folderpane-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
$Root = Join-Path $Dir 'root'
New-Item -ItemType Directory -Force (Join-Path $Root 'sub') | Out-Null
foreach ($f in 'file10.txt', 'file2.txt', 'z.txt', 'sub\a.txt', 'sub\b.md') { Set-Content (Join-Path $Root $f) 'x' }
$hidden = Join-Path $Root 'hidden.txt'; Set-Content $hidden 'x'; attrib +h $hidden
$script:failed = $false
$p = $null

function Item($name, $under) {
  $c = [T]::Next($tree, $under, $TVGN_CHILD)
  while ($c -ne [IntPtr]::Zero) { if ([T]::Text($tree, $p.Id, $c) -eq $name) { return $c }; $c = [T]::Next($tree, $c, $TVGN_NEXT) }
  [IntPtr]::Zero
}
function Names($under) {
  $out = @(); $c = [T]::Next($tree, $under, $TVGN_CHILD)
  while ($c -ne [IntPtr]::Zero) { $out += [T]::Text($tree, $p.Id, $c); $c = [T]::Next($tree, $c, $TVGN_NEXT) }
  $out -join ','
}
function Wait-For([scriptblock]$cond) { for ($i = 0; $i -lt 40 -and -not (& $cond); $i++) { Start-Sleep -Milliseconds 100 } }
function Key($h, $vk) { [Q]::PostMessageW($h, $WM_KEYDOWN, [IntPtr]$vk, [IntPtr]1) | Out-Null; Start-Sleep -Milliseconds 400 }
function Select-Item($item) { [Q]::SendMessageW($tree, 0x110B, [IntPtr]$TVGN_CARET, $item) | Out-Null } # TVM_SELECTITEM
function Start-App([string]$arg) {
  $script:p = if ($arg) { Start-Process $Exe -ArgumentList "`"$arg`"" -PassThru } else { Start-Process $Exe -PassThru }
  $script:main = Wait-Top $p.Id 'RENotepad' ([NullString]::Value); Start-Sleep -Milliseconds 300
  [Q]::SetWindowPos($main, [IntPtr]::Zero, 0, 0, 1000, 700, 0x0010 -bor 0x0004) | Out-Null # NOACTIVATE | NOZORDER
  $script:tree = [T]::Tree($main)
}

try {
Start-App $Root
$rootItem = [T]::Next($tree, [IntPtr]::Zero, $TVGN_ROOT)
Wait-For { (Names $rootItem) -notmatch 'Loading' -and (Names $rootItem) -ne '' }
Check 'a folder argument shows the pane' ([T]::Visible($tree)) $true
Check 'the root item is the folder' ([T]::Text($tree, $p.Id, $rootItem)) 'root'
Check 'folders first, Explorer number order, no hidden file' (Names $rootItem) 'sub,file2.txt,file10.txt,z.txt'
Check 'no file opens' (Title $main) 'Untitled - RENotepad'

$f2 = Item 'file2.txt' $rootItem
Select-Item $f2; Key $tree 0x0D # Enter
Check 'Enter opens the file in a tab' (Title $main) 'file2.txt - RENotepad'
Check 'the open file shows in bold' (([T]::State($tree, $f2) -band $TVIS_BOLD) -ne 0) $true

$sub = Item 'sub' $rootItem
Select-Item $sub; Key $tree 0x27 # Right
Wait-For { (Names $sub) -eq 'a.txt,b.md' }
Check 'Right lists and expands a folder' ("$(Names $sub) $(([T]::State($tree, $sub) -band $TVIS_EXPANDED) -ne 0)") 'a.txt,b.md True'

Set-Content (Join-Path $Root 'file3.txt') 'x'
Wait-For { (Names $rootItem) -match 'file3' }
Check 'a new file on disk shows in place' (Names $rootItem) 'sub,file2.txt,file3.txt,file10.txt,z.txt'
Remove-Item (Join-Path $Root 'z.txt')
Wait-For { (Names $rootItem) -notmatch 'z.txt' }
Check 'a file deleted on disk goes' (Names $rootItem) 'sub,file2.txt,file3.txt,file10.txt'

# A tool that replaces the open file (delete, then write) must not lose its bold.
Remove-Item (Join-Path $Root 'file2.txt'); Start-Sleep -Milliseconds 700
Set-Content (Join-Path $Root 'file2.txt') 'y'
$f2 = [IntPtr]::Zero
for ($i = 0; $i -lt 40 -and ($f2 -eq [IntPtr]::Zero -or ([T]::State($tree, $f2) -band $TVIS_BOLD) -eq 0); $i++) { Start-Sleep -Milliseconds 100; $f2 = Item 'file2.txt' $rootItem }
Check 'a replaced open file is bold again' (($f2 -ne [IntPtr]::Zero) -and (([T]::State($tree, $f2) -band $TVIS_BOLD) -ne 0)) $true

# Rename the open file in place: F2, new text in the label editor, then end the edit.
Select-Item $f2; Key $tree 0x71 # F2
Wait-For { [Q]::SendMessageW($tree, 0x110F, [IntPtr]::Zero, [IntPtr]::Zero) -ne [IntPtr]::Zero } # TVM_GETEDITCONTROL
$ed = [Q]::SendMessageW($tree, 0x110F, [IntPtr]::Zero, [IntPtr]::Zero)
Check 'F2 starts the rename' ($ed -ne [IntPtr]::Zero) $true
[Q]::SendMessageW($ed, 0x0C, [IntPtr]::Zero, 'renamed.txt') | Out-Null # WM_SETTEXT
[Q]::SendMessageW($tree, 0x1116, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 600 # TVM_ENDEDITLABELNOW, save
Check 'the file is renamed on disk' ("$(Test-Path (Join-Path $Root 'renamed.txt')) $(Test-Path (Join-Path $Root 'file2.txt'))") 'True False'
Check 'its open tab takes the new name' (Title $main) 'renamed.txt - RENotepad'
Check 'the tree shows it in sorted place' (Names $rootItem) 'sub,file3.txt,file10.txt,renamed.txt'

# A name Windows refuses: the file keeps its name and a message box says why.
$f3 = Item 'file3.txt' $rootItem
Select-Item $f3; Key $tree 0x71
Wait-For { [Q]::SendMessageW($tree, 0x110F, [IntPtr]::Zero, [IntPtr]::Zero) -ne [IntPtr]::Zero }
$ed = [Q]::SendMessageW($tree, 0x110F, [IntPtr]::Zero, [IntPtr]::Zero)
[Q]::SendMessageW($ed, 0x0C, [IntPtr]::Zero, 'bad:name.txt') | Out-Null
[Q]::SendMessageW($tree, 0x1116, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$box = Wait-Top $p.Id 'RENotepadDialog' 'RENotepad'
Check 'a bad name is refused with a message' ("$($box -ne [IntPtr]::Zero) $(Test-Path (Join-Path $Root 'file3.txt'))") 'True True'
if ($box -ne [IntPtr]::Zero) { [Q]::PostMessageW($box, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 400 }

Select-Item $rootItem; Cmd $main 204 # New file
Wait-For { [Q]::SendMessageW($tree, 0x110F, [IntPtr]::Zero, [IntPtr]::Zero) -ne [IntPtr]::Zero }
Check 'New file makes the file' (Test-Path (Join-Path $Root 'New Text Document.txt')) $true
Check 'and starts its rename' ([Q]::SendMessageW($tree, 0x110F, [IntPtr]::Zero, [IntPtr]::Zero) -ne [IntPtr]::Zero) $true
[Q]::SendMessageW($tree, 0x1116, [IntPtr]1, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 300 # cancel the edit

# Delete the open file: the shell asks, we answer Yes; the tab stays, now modified.
$rn = Item 'renamed.txt' $rootItem
Select-Item $rn; Key $tree 0x2E # Del
$ask = [IntPtr]::Zero
for ($i = 0; $i -lt 50 -and $ask -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100; $ask = [Q]::Top($p.Id, '#32770', [NullString]::Value) }
if ($ask -ne [IntPtr]::Zero) { [Q]::PostMessageW($ask, $WM_COMMAND, [IntPtr]6, [IntPtr]::Zero) | Out-Null } # IDYES
Wait-For { -not (Test-Path (Join-Path $Root 'renamed.txt')) }
Check 'Delete moves the file away' (Test-Path (Join-Path $Root 'renamed.txt')) $false
Start-Sleep -Milliseconds 400
Check 'its tab stays open, modified' (Title $main) '*renamed.txt - RENotepad'
Check 'the item goes from the tree' ((Names $rootItem) -notmatch 'renamed') $true

Cmd $main 56 # View > Folder pane: hide
Check 'View > Folder pane hides it' ([T]::Visible($tree)) $false
Cmd $main 56
Check 'and shows it again' ([T]::Visible($tree)) $true

[Q]::SetWindowPos($main, [IntPtr]::Zero, 0, 0, 450, 600, 0x0010 -bor 0x0004 -bor 0x0002) | Out-Null; Start-Sleep -Milliseconds 300
Check 'a narrow window hides the pane' ([T]::Visible($tree)) $false
[Q]::SetWindowPos($main, [IntPtr]::Zero, 0, 0, 1000, 700, 0x0010 -bor 0x0004 -bor 0x0002) | Out-Null; Start-Sleep -Milliseconds 300
Check 'a wide window shows it again' ([T]::Visible($tree)) $true

# Splitter drag: press on the splitter right of the tree, move 80 px right, release.
$w0 = [T]::Width($tree)
$y = 300
$down = [IntPtr](($y -shl 16) -bor ($w0 + 2)); $move = [IntPtr](($y -shl 16) -bor ($w0 + 82))
# Sent, not posted: the capture makes Windows add a move at the real pointer, which
# must not come between them when the pointer rests over the window.
[Q]::SendMessageW($main, 0x201, [IntPtr]1, $down) | Out-Null
[Q]::SendMessageW($main, 0x200, [IntPtr]1, $move) | Out-Null
[Q]::SendMessageW($main, 0x202, [IntPtr]0, $move) | Out-Null; Start-Sleep -Milliseconds 300
$w1 = [T]::Width($tree)
Check 'a splitter drag widens the pane' ($w1 -eq $w0 + 80) $true

# Close: the tab is modified, so answer the save prompt with Don't save.
[Q]::PostMessageW($main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
$ask = Wait-Top $p.Id 'RENotepadDialog' 'RENotepad'
if ($ask -ne [IntPtr]::Zero) { [Q]::PostMessageW($ask, $WM_COMMAND, [IntPtr]7, [IntPtr]::Zero) | Out-Null } # IDNO
Check 'the window closes' ($p.WaitForExit(4000)) $true
$k = Get-ItemProperty $TestKey
Check 'the pane state is saved' ("$($k.fFolderPane) $($k.FolderRoot)") "1 $Root"

Start-App ''
$rootItem = [T]::Next($tree, [IntPtr]::Zero, $TVGN_ROOT)
Wait-For { (Names $rootItem) -notmatch 'Loading' -and (Names $rootItem) -ne '' }
Check 'the pane comes back at the next start' ("$([T]::Visible($tree)) $([T]::Text($tree, $p.Id, $rootItem))") 'True root'
Check 'with its width' ([T]::Width($tree)) $w1
Cmd $main 212 # Close folder
Check 'Close folder hides the pane' ([T]::Visible($tree)) $false
[Q]::PostMessageW($main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Check 'the window closes again' ($p.WaitForExit(4000)) $true
Check 'the closed folder is saved' ((Get-ItemProperty $TestKey).fFolderPane) 0

} finally {
  if ($p -and -not $p.HasExited) { $p.Kill() }
  Remove-Item $TestKey -Recurse -ErrorAction SilentlyContinue
  Remove-Item $Dir -Recurse -Force -ErrorAction SilentlyContinue
}
if ($script:failed) { exit 1 }
