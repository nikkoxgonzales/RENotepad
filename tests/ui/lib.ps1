# Shared helpers for the focus-free UI scripts: find windows by process and
# class, drive them with window messages, capture them without activating.
# Dot-source it: . "$PSScriptRoot\lib.ps1"

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class Q {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, string l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, StringBuilder l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, int[] l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, P cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, int[] r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, int[] r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetLayeredWindowAttributes(IntPtr h, uint key, byte alpha, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, uint attr, int[] r, int size);
  [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
  public delegate bool P(IntPtr h, IntPtr l);
  static string Cls(IntPtr h) { var c = new StringBuilder(64); GetClassNameW(h, c, 64); return c.ToString(); }
  public static IntPtr Top(uint pid, string cls, string title) {
    IntPtr f = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      if (p == pid && Cls(h) == cls && (title == null || t.ToString() == title)) { f = h; return false; } return true; }, IntPtr.Zero);
    return f;
  }
  /// A top-level window of class `cls` owned by `owner` (dialogs over dialogs).
  public static IntPtr Owned(uint pid, string cls, IntPtr owner) {
    IntPtr f = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && Cls(h) == cls && GetWindow(h, 4) == owner) { f = h; return false; } return true; }, IntPtr.Zero); // GW_OWNER
    return f;
  }
  /// The active tab's editor: the one visible Edit child of the main window.
  public static IntPtr VisibleEdit(IntPtr main) {
    IntPtr f = IntPtr.Zero;
    // WS_VISIBLE on the edit itself: IsWindowVisible is false while the main window is hidden.
    EnumChildWindows(main, (h, l) => { if (Cls(h) == "Edit" && (GetWindowLongPtrW(h, -16).ToInt64() & 0x10000000) != 0) { f = h; return false; } return true; }, IntPtr.Zero);
    return f;
  }
  public static int CountEdits(IntPtr main) {
    int n = 0;
    EnumChildWindows(main, (h, l) => { if (Cls(h) == "Edit") n++; return true; }, IntPtr.Zero);
    return n;
  }
}
'@
$WM_COMMAND = 0x111
$WM_CLOSE = 0x10

function Wait-Top($pid_, $cls, $title) {
  for ($i = 0; $i -lt 100; $i++) {
    $h = [Q]::Top($pid_, $cls, $(if ($title) { $title } else { [NullString]::Value }))
    if ($h -ne [IntPtr]::Zero) { return $h }
    Start-Sleep -Milliseconds 100
  }
  [IntPtr]::Zero
}
function Sel($e) {
  $a = [Runtime.InteropServices.Marshal]::AllocHGlobal(8); [Q]::SendMessageW($e, 0xB0, $a, [IntPtr]::Add($a, 4)) | Out-Null
  $r = "$([Runtime.InteropServices.Marshal]::ReadInt32($a))-$([Runtime.InteropServices.Marshal]::ReadInt32($a, 4))"
  [Runtime.InteropServices.Marshal]::FreeHGlobal($a); $r
}
function Text($e) {
  $n = [int][Q]::SendMessageW($e, 0x0E, [IntPtr]::Zero, [IntPtr]::Zero); $sb = New-Object Text.StringBuilder ($n + 1)
  [Q]::SendMessageW($e, 0x0D, [IntPtr]($n + 1), $sb) | Out-Null; $sb.ToString()
}
function Title($h) { $sb = New-Object Text.StringBuilder 256; [Q]::GetWindowTextW($h, $sb, 256) | Out-Null; $sb.ToString() }
function Cmd($h, $id) { [Q]::PostMessageW($h, $WM_COMMAND, [IntPtr]$id, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 400 }
# Type as if by keyboard: replaces the selection and marks the document modified.
function Type-Text($e, $s) { [Q]::SendMessageW($e, 0xC2, [IntPtr]1, $s) | Out-Null; Start-Sleep -Milliseconds 150 }
function Check($name, $got, $want) {
  if ("$got" -eq "$want") { "PASS $name" } else { "FAIL $name : got [$got] want [$want]"; $script:failed = $true }
}
# Show the window so DWM renders it for PrintWindow, without the user ever
# noticing: 1/255 opaque, click-through, no taskbar button, never activated.
# (Off-screen doesn't work: DWM skips windows outside every monitor.)
function Show-Ghost($h, $w = 952, $hgt = 607) {
  $ex = [Q]::GetWindowLongPtrW($h, -20).ToInt64() -bor 0x80000 -bor 0x20 -bor 0x80 # LAYERED | TRANSPARENT | TOOLWINDOW
  [Q]::SetWindowLongPtrW($h, -20, [IntPtr]$ex) | Out-Null
  [Q]::SetLayeredWindowAttributes($h, 0, 1, 2) | Out-Null # LWA_ALPHA
  [Q]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, $w, $hgt, 0x0040 -bor 0x0010 -bor 0x0004) | Out-Null # SHOWWINDOW|NOACTIVATE|NOZORDER
  Start-Sleep -Milliseconds 700
}
# Crop a Snap of $h to what's visible on screen (drops the invisible resize borders).
function Crop-ToFrame($h, $png) {
  $w = New-Object int[] 4; $f = New-Object int[] 4
  [Q]::GetWindowRect($h, $w) | Out-Null; [Q]::DwmGetWindowAttribute($h, 9, $f, 16) | Out-Null # DWMWA_EXTENDED_FRAME_BOUNDS
  $src = [Drawing.Bitmap]::FromFile($png)
  $rect = New-Object Drawing.Rectangle ($f[0] - $w[0]), ($f[1] - $w[1]), ($f[2] - $f[0]), ($f[3] - $f[1])
  $out = $src.Clone($rect, $src.PixelFormat); $src.Dispose()
  $out.Save($png); $out.Dispose()
}
function Snap($h, $out) {
  $r = New-Object int[] 4; [Q]::GetWindowRect($h, $r) | Out-Null
  $bmp = New-Object Drawing.Bitmap ($r[2] - $r[0]), ($r[3] - $r[1]); $g = [Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
  [Q]::PrintWindow($h, $dc, 2) | Out-Null # PW_RENDERFULLCONTENT
  $g.ReleaseHdc($dc); $bmp.Save($out); $g.Dispose(); $bmp.Dispose()
}
