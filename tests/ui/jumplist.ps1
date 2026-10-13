# Jump list access for the tests, through the shell's own (undocumented)
# AutomaticDestinationList, the object src/jumplist.rs uses. Dot-source it.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace Pins {
  [ComImport, Guid("E9C5EF8D-FD41-4F72-BA87-EB03BAD5817C"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IADL {
    [PreserveSig] int Initialize([MarshalAs(UnmanagedType.LPWStr)] string id, [MarshalAs(UnmanagedType.LPWStr)] string a, [MarshalAs(UnmanagedType.LPWStr)] string b);
    [PreserveSig] int HasList(out int has);
    [PreserveSig] int GetList(int type, int max, int flags, ref Guid riid, [MarshalAs(UnmanagedType.IUnknown)] out object list);
    [PreserveSig] int AddUsagePoint([MarshalAs(UnmanagedType.IUnknown)] object item);
    [PreserveSig] int PinItem([MarshalAs(UnmanagedType.IUnknown)] object item, int index);
    [PreserveSig] int IsPinned([MarshalAs(UnmanagedType.IUnknown)] object item, out int index);
    [PreserveSig] int RemoveDestination([MarshalAs(UnmanagedType.IUnknown)] object item);
    [PreserveSig] int SetUsageData(IntPtr a, IntPtr b, IntPtr c);
    [PreserveSig] int GetUsageData(IntPtr a, IntPtr b, IntPtr c);
    [PreserveSig] int ResolveDestination(IntPtr h, int f, IntPtr si, ref Guid riid, out IntPtr o);
    [PreserveSig] int ClearList(int pinned);
  }
  [ComImport, Guid("92CA9DCD-5622-4BBA-A805-5E9F541BD8C9"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IObjectArray { [PreserveSig] int GetCount(out uint n); [PreserveSig] int GetAt(uint i, ref Guid riid, [MarshalAs(UnmanagedType.IUnknown)] out object o); }
  [ComImport, Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellItem { void BindToHandler(); void GetParent(); [PreserveSig] int GetDisplayName(uint sigdn, out IntPtr name); }
  public static class L {
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern int SHCreateItemFromParsingName(string p, IntPtr bc, ref Guid riid, [MarshalAs(UnmanagedType.IUnknown)] out object o);
    static Guid A = new Guid("92CA9DCD-5622-4BBA-A805-5E9F541BD8C9"), I = new Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe");
    static IADL Open(string id) { var l = (IADL)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("F0AE1542-F497-484B-A175-A20DB09144BA"))); Marshal.ThrowExceptionForHR(l.Initialize(id, null, null)); return l; }
    public static void Pin(string id, string path) { object o; Marshal.ThrowExceptionForHR(SHCreateItemFromParsingName(path, IntPtr.Zero, ref I, out o)); Marshal.ThrowExceptionForHR(Open(id).PinItem(o, -1)); }
    public static string[] Pinned(string id) {
      var l = Open(id); object lo; if (l.GetList(0, 100, 1, ref A, out lo) != 0) return new string[0];
      var arr = (IObjectArray)lo; uint n; arr.GetCount(out n); var r = new string[n];
      for (uint i = 0; i < n; i++) { object it; arr.GetAt(i, ref I, out it); IntPtr p; ((IShellItem)it).GetDisplayName(0x80058000, out p); r[i] = Marshal.PtrToStringUni(p); Marshal.FreeCoTaskMem(p); }
      return r;
    }
    public static void Clear(string id) { Open(id).ClearList(1); }
    // The list's file name: CRC-64 (reflected, poly 0x92C64265D32139A4, init ~0)
    // of the upper-cased ID in UTF-16 LE, as lowercase hex.
    public static string FileName(string id) {
      ulong c = ulong.MaxValue;
      foreach (var b in System.Text.Encoding.Unicode.GetBytes(id.ToUpperInvariant())) { c ^= b; for (int i = 0; i < 8; i++) c = (c & 1) != 0 ? (c >> 1) ^ 0x92C64265D32139A4UL : c >> 1; }
      return c.ToString("x");
    }
  }
}
'@
