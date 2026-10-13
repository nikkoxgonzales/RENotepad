//! File encodings, detected and written the way Windows 10 Notepad does it.
//! See docs/WIN10-NOTEPAD-SPEC.md § Encoding detection.

use windows_sys::Win32::Globalization::{
    IsTextUnicode, MultiByteToWideChar, WideCharToMultiByte, CP_ACP, IS_TEXT_UNICODE_STATISTICS,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Ansi,
    Utf16Le,
    Utf16Be,
    Utf8Bom,
    Utf8,
}

impl Encoding {
    pub const ALL: [Encoding; 5] = [
        Encoding::Ansi,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
        Encoding::Utf8,
        Encoding::Utf8Bom,
    ];

    /// Status-bar / dialog label, same wording as Windows 10.
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Ansi => "ANSI",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
        }
    }

    pub fn to_index(self) -> u32 {
        Encoding::ALL.iter().position(|&e| e == self).unwrap() as u32
    }

    pub fn from_index(i: u32) -> Option<Encoding> {
        Encoding::ALL.get(i as usize).copied()
    }
}

/// Win10 order: BOMs, then IsTextUnicode (statistics-only verdicts on < 100 bytes
/// are rejected), then strict UTF-8 — valid means UTF-8, invalid means ANSI.
pub fn detect(bytes: &[u8]) -> Encoding {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Encoding::Utf8Bom;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Encoding::Utf16Le;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Encoding::Utf16Be;
    }
    if bytes.is_empty() {
        return Encoding::Utf8;
    }
    if is_text_unicode(bytes) {
        return Encoding::Utf16Le;
    }
    if std::str::from_utf8(bytes).is_ok() {
        Encoding::Utf8
    } else {
        Encoding::Ansi
    }
}

fn is_text_unicode(bytes: &[u8]) -> bool {
    let len = bytes.len().min(i32::MAX as usize) as i32;
    let mut result: u32 = u32::MAX; // ask for every test, as Notepad does
    let hit = unsafe { IsTextUnicode(bytes.as_ptr().cast(), len, &mut result) } != 0;
    hit && !(result == IS_TEXT_UNICODE_STATISTICS && len < 100)
}

/// Bytes -> UTF-16 text for the edit control. BOMs are stripped.
pub fn decode(bytes: &[u8], enc: Encoding) -> Vec<u16> {
    match enc {
        Encoding::Utf8Bom | Encoding::Utf8 => {
            let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            String::from_utf8_lossy(b).encode_utf16().collect()
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let le = enc == Encoding::Utf16Le;
            let mut units: Vec<u16> = bytes
                .as_chunks::<2>().0.iter()
                .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
                .collect();
            if units.first() == Some(&0xFEFF) {
                units.remove(0);
            }
            units
        }
        Encoding::Ansi => ansi_to_wide(bytes),
    }
}

/// Text -> bytes on disk. `Err(())` means ANSI cannot represent every character
/// (the caller warns before writing, like Notepad).
pub fn encode(text: &[u16], enc: Encoding, allow_lossy: bool) -> Result<Vec<u8>, ()> {
    Ok(match enc {
        Encoding::Utf8 => String::from_utf16_lossy(text).into_bytes(),
        Encoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend(String::from_utf16_lossy(text).into_bytes());
            v
        }
        Encoding::Utf16Le => std::iter::once(0xFEFFu16).chain(text.iter().copied()).flat_map(u16::to_le_bytes).collect(),
        Encoding::Utf16Be => std::iter::once(0xFEFFu16).chain(text.iter().copied()).flat_map(u16::to_be_bytes).collect(),
        Encoding::Ansi => {
            let (bytes, lossy) = wide_to_ansi(text);
            if lossy && !allow_lossy {
                return Err(());
            }
            bytes
        }
    })
}

fn ansi_to_wide(bytes: &[u8]) -> Vec<u16> {
    if bytes.is_empty() {
        return Vec::new();
    }
    unsafe {
        let n = MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
        let mut out = vec![0u16; n.max(0) as usize];
        MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, out.as_mut_ptr(), n);
        out
    }
}

fn wide_to_ansi(text: &[u16]) -> (Vec<u8>, bool) {
    if text.is_empty() {
        return (Vec::new(), false);
    }
    unsafe {
        let mut used_default: i32 = 0;
        let n = WideCharToMultiByte(CP_ACP, 0, text.as_ptr(), text.len() as i32, std::ptr::null_mut(), 0, std::ptr::null(), &mut used_default);
        let mut out = vec![0u8; n.max(0) as usize];
        WideCharToMultiByte(CP_ACP, 0, text.as_ptr(), text.len() as i32, out.as_mut_ptr(), n, std::ptr::null(), &mut used_default);
        (out, used_default != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn bom_utf8() {
        assert_eq!(detect(b"\xEF\xBB\xBFhi"), Encoding::Utf8Bom);
    }

    #[test]
    fn bom_utf16le() {
        assert_eq!(detect(b"\xFF\xFEh\0i\0"), Encoding::Utf16Le);
    }

    #[test]
    fn bom_utf16be() {
        assert_eq!(detect(b"\xFE\xFF\0h\0i"), Encoding::Utf16Be);
    }

    #[test]
    fn plain_ascii_is_utf8() {
        assert_eq!(detect(b"hello world\r\n"), Encoding::Utf8);
    }

    #[test]
    fn empty_is_utf8() {
        assert_eq!(detect(b""), Encoding::Utf8);
    }

    #[test]
    fn invalid_utf8_is_ansi() {
        assert_eq!(detect(b"caf\xE9 au lait"), Encoding::Ansi);
    }

    #[test]
    fn valid_multibyte_utf8() {
        assert_eq!(detect("café ñandú".as_bytes()), Encoding::Utf8);
    }

    #[test]
    fn bomless_utf16le_long_text() {
        let bytes: Vec<u8> = w(&"The quick brown fox jumps over the lazy dog. ".repeat(4))
            .iter()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(detect(&bytes), Encoding::Utf16Le);
    }

    #[test]
    fn short_ascii_not_mistaken_for_utf16() {
        // The classic "Bush hid the facts" false positive: statistics-only verdict on < 100 bytes.
        assert_eq!(detect(b"Bush hid the facts"), Encoding::Utf8);
    }

    #[test]
    fn roundtrip_all_unicode_encodings() {
        let text = w("line one\r\nλ → ✓ 𝄞\r\n");
        for enc in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be] {
            let bytes = encode(&text, enc, false).unwrap();
            assert_eq!(detect(&bytes), enc, "{enc:?}");
            assert_eq!(decode(&bytes, enc), text, "{enc:?}");
        }
    }

    #[test]
    fn ansi_refuses_lossy_without_consent() {
        assert!(encode(&w("snow ☃"), Encoding::Ansi, false).is_err());
        assert!(encode(&w("snow ☃"), Encoding::Ansi, true).is_ok());
    }

    #[test]
    fn ansi_plain_roundtrip() {
        let text = w("plain ascii");
        let bytes = encode(&text, Encoding::Ansi, false).unwrap();
        assert_eq!(decode(&bytes, Encoding::Ansi), text);
    }
}
