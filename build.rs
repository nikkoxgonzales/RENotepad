// Embed the application manifest (Common Controls 6 + PerMonitorV2 DPI), the
// icon and the version info. Without the manifest the edit control's EOL/zoom
// extensions and dark theming do nothing.
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = root.join("RENotepad.manifest");
    let icon = root.join("assets").join("RENotepad.ico");
    println!("cargo:rerun-if-changed=RENotepad.manifest");
    println!("cargo:rerun-if-changed=assets/RENotepad.ico");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());

    let version = env!("CARGO_PKG_VERSION");
    let mut parts: Vec<u16> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let numeric = parts.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Nikko Gonzales"
      VALUE "FileDescription", "RENotepad"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "RENotepad"
      VALUE "LegalCopyright", "© 2026 Nikko Gonzales. MIT licensed."
      VALUE "OriginalFilename", "RENotepad.exe"
      VALUE "ProductName", "RENotepad"
      VALUE "ProductVersion", "{version}"
      VALUE "Comments", "Windows 10 Notepad, reverse-engineered and rebuilt in Rust."
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("RENotepad.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
