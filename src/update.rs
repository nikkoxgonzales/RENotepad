//! Update check: asks GitHub for the latest release of the public repo and
//! compares its tag with this build's version. One HTTPS GET, nothing sent
//! but the User-Agent; runs on a worker thread and posts the outcome back.

use crate::wide;
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const REPO: &str = "nikkoxgonzales/RENotepad";
/// Posted to the main window: wParam = 1 for a check the user asked for,
/// lParam = Box<Outcome> to take back with `Box::from_raw`.
pub const WM_UPDATE_RESULT: u32 = WM_APP + 1;

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

pub enum Outcome {
    UpToDate,
    Available { version: String, url: String },
    Failed(String),
}

/// Start a check; the answer arrives as WM_UPDATE_RESULT.
pub fn check_async(hwnd: HWND, manual: bool) {
    let target = hwnd as usize;
    std::thread::spawn(move || {
        // A test hook: point the check at another repo.
        let repo = std::env::var("RENOTEPAD_UPDATE_REPO").unwrap_or_else(|_| REPO.to_string());
        let outcome = match latest(&repo) {
            Ok((tag, url)) if is_newer(&tag, current()) => Outcome::Available { version: tag.trim_start_matches(['v', 'V']).to_string(), url },
            Ok(_) => Outcome::UpToDate,
            Err(e) => Outcome::Failed(e),
        };
        let ptr = Box::into_raw(Box::new(outcome));
        if unsafe { PostMessageW(target as HWND, WM_UPDATE_RESULT, manual as WPARAM, ptr as LPARAM) } == 0 {
            drop(unsafe { Box::from_raw(ptr) }); // the window is gone
        }
    });
}

/// (tag, release page) of the latest release of `repo`.
pub fn latest(repo: &str) -> Result<(String, String), String> {
    let body = get("api.github.com", &format!("/repos/{repo}/releases/latest"))?;
    let tag = json_string(&body, "tag_name").ok_or("The reply had no version in it.")?;
    Ok((tag, release_url(json_string(&body, "html_url"))))
}

/// The page the Download button opens. It goes to ShellExecute, so anything
/// that isn't a GitHub web address falls back to the releases page.
fn release_url(html_url: Option<String>) -> String {
    html_url.filter(|u| u.starts_with("https://github.com/") && !u.contains(['"', ' '])).unwrap_or_else(releases_page)
}

/// Is release tag `latest` ("v1.2.3") newer than `current` ("1.2.0")?
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let core = v.split(['-', '+']).next().unwrap_or("");
        core.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (mut a, mut b) = (parts(latest), parts(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// The string value of the first `"key": "..."` in a JSON text. Enough for
/// GitHub's release object; not a general JSON parser.
pub fn json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut rest = &json[json.find(&needle)? + needle.len()..];
    rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?).unwrap_or('\u{fffd}'));
                }
                other => out.push(other), // \" \\ \/
            },
            c => out.push(c),
        }
    }
    None
}

/// Closes a WinHTTP handle when dropped.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn get(host: &str, path: &str) -> Result<String, String> {
    let fail = |what: &str| format!("{what} failed (error {}).", unsafe { windows_sys::Win32::Foundation::GetLastError() });
    unsafe {
        let agent = wide(&format!("RENotepad/{}", current()));
        let session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0));
        if session.0.is_null() {
            return Err(fail("Starting WinHTTP"));
        }
        WinHttpSetTimeouts(session.0, 5000, 5000, 10000, 10000);
        let host_w = wide(host);
        let conn = Handle(WinHttpConnect(session.0, host_w.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if conn.0.is_null() {
            return Err(fail("Connecting"));
        }
        let verb = wide("GET");
        let path_w = wide(path);
        let req = Handle(WinHttpOpenRequest(conn.0, verb.as_ptr(), path_w.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), WINHTTP_FLAG_SECURE));
        if req.0.is_null() {
            return Err(fail("Opening the request"));
        }
        let headers = wide("Accept: application/vnd.github+json\r\n");
        WinHttpAddRequestHeaders(req.0, headers.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD);
        if WinHttpSendRequest(req.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0 {
            return Err(fail("Reaching GitHub"));
        }
        let mut status = 0u32;
        let mut size = 4u32;
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), (&mut status as *mut u32).cast(), &mut size, std::ptr::null_mut());
        let mut body = Vec::new();
        loop {
            let mut buf = [0u8; 8192];
            let mut got = 0u32;
            if WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got) == 0 {
                return Err(fail("Reading the reply"));
            }
            if got == 0 || body.len() > 4 << 20 {
                break;
            }
            body.extend_from_slice(&buf[..got as usize]);
        }
        match status {
            200 => Ok(String::from_utf8_lossy(&body).into_owned()),
            404 => Err("No release has been published yet.".into()),
            403 | 429 => Err("GitHub is limiting requests right now. Try again later.".into()),
            s => Err(format!("GitHub answered with HTTP {s}.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions() {
        assert!(is_newer("v0.3.1", "0.3.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(!is_newer("v0.3.0", "0.3.0"));
        assert!(!is_newer("0.3", "0.3.0"));
        assert!(!is_newer("v0.2.9", "0.3.0"));
        assert!(!is_newer("v0.3.0-beta", "0.3.0"));
    }

    #[test]
    fn reads_release_json() {
        let j = r#"{"url":"x","html_url":"https://github.com/a/b/releases/tag/v1.2.0","id":1,"tag_name" : "v1.2.0","name":"Say \"hi\"é"}"#;
        assert_eq!(json_string(j, "tag_name").as_deref(), Some("v1.2.0"));
        assert_eq!(json_string(j, "html_url").as_deref(), Some("https://github.com/a/b/releases/tag/v1.2.0"));
        assert_eq!(json_string(j, "name").as_deref(), Some("Say \"hi\"\u{e9}"));
        assert_eq!(json_string(j, "missing"), None);
    }

    #[test]
    fn download_link_is_always_github() {
        let ok = "https://github.com/a/b/releases/tag/v1.2.0";
        assert_eq!(release_url(Some(ok.into())), ok);
        assert_eq!(release_url(Some(r"C:\Windows\System32\calc.exe".into())), releases_page());
        assert_eq!(release_url(Some("https://github.com.evil.example/x".into())), releases_page());
        assert_eq!(release_url(None), releases_page());
    }

    /// Network: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_a_real_release() {
        let (tag, url) = latest("microsoft/terminal").expect("fetch");
        assert!(tag.starts_with('v'), "{tag}");
        assert!(url.starts_with("https://github.com/microsoft/terminal/releases/"), "{url}");
        assert!(latest("nikkoxgonzales/this-repo-does-not-exist").is_err());
    }
}
