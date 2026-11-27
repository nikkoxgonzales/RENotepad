//! Lab benchmarks of the hot paths (test builds only). Two numbers for each path:
//! the allocations of one run (exact, the same on each PC, so a ratchet can guard
//! it) and the fastest of N runs in ms (a check that the allocations follow time).
//! Report: `perf_report -- --ignored --nocapture --test-threads=1`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

// SAFETY: each call goes to System unchanged. The counter is a const thread
// local, so counting allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations on this thread while `f` runs.
pub fn allocs<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = ALLOCS.with(|c| c.get());
    let r = f();
    (r, ALLOCS.with(|c| c.get()) - before)
}

/// The fastest of `n` runs, in ms.
pub fn min_ms<R>(n: usize, mut f: impl FnMut() -> R) -> f64 {
    (0..n)
        .map(|_| {
            let t = std::time::Instant::now();
            std::hint::black_box(f());
            t.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::INFINITY, f64::min)
}

/// The same 200 KB README that tests/perf/journeys.ps1 previews.
pub fn readme() -> String {
    let mut s = String::from("---\ntitle: Perf\ntags: [a, b]\n---\n# Title\n\n");
    let mut k = 0;
    while s.len() < 200 * 1024 {
        s.push_str(&format!(
            "## Section {k}\n\nSome **bold**, *italic*, `code` and a [link](https://example.com/{k}). A plain URL: https://example.com/x/{k} :smile:\n\n\
             - item one\n- [x] done item\n  - nested item with `inline code`\n\n\
             > [!NOTE]\n> A note with **emphasis**.\n\n\
             | Name | Value | Notes |\n|------|------:|-------|\n| alpha | {k} | first |\n| beta | 2 | second row with more text |\n\n\
             ```rust\nfn main() {{\n    let x = {k};\n    println!(\"hello {{}}\", x); // comment\n}}\n```\n\n\
             1. first\n2. second\n\n---\n\n"
        ));
        k += 1;
    }
    s
}

/// 5 MB of text lines like the journeys' big.txt, CRLF line ends.
pub fn big_text() -> String {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < 5 << 20 {
        s.push_str(&format!("{i} The quick brown fox jumps over the lazy dog 0123456789 \r\n"));
        i += 1;
    }
    s
}

/// Pseudo-random bytes, the same on each run.
pub fn noise(n: usize) -> Vec<u8> {
    let mut x = 0x2545F4914F6CDD1Du64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{self, Options, Picture};
    use crate::wide;
    use windows_sys::Win32::UI::Controls::EM_SETLIMITTEXT;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn no_picture(_: &str) -> Option<Picture> {
        None
    }

    fn row(name: &str, ms: f64, allocs: u64) {
        println!("{name:<22} {ms:>9.2} ms {allocs:>10} allocs");
    }

    // Ratchets: allocation counts are exact, so these fail on a regression on
    // each PC. When a change lowers a count, lower its ceiling in the same commit.
    // Never raise a ceiling to get green: find the new allocations first.

    /// One allocation for the whole string, not one for each byte (was 3.1 M for 3 MB).
    #[test]
    fn picture_hex_allocates_once() {
        let pic = noise(1 << 20);
        assert_eq!(allocs(|| crate::preview::to_hex(&pic)).1, 1);
    }

    /// Valid UTF-8 decodes into one buffer (the system converter path).
    #[test]
    fn utf8_decode_allocates_once() {
        let text = big_text();
        assert_eq!(allocs(|| crate::encoding::decode(text.as_bytes(), crate::encoding::Encoding::Utf8)).1, 1);
    }

    /// to_rtf of the 200 KB README stays at or under its ceiling (120,502 on 2026-11-28).
    #[test]
    fn md_to_rtf_allocations_stay_under_ceiling() {
        const CEILING: u64 = 120_502;
        let md = readme();
        let opts = Options { width: 9000, toggled_details: &[], picture: &no_picture, code_char: 110.0 };
        let n = allocs(|| markdown::to_rtf(&md, &markdown::tests::C, &opts)).1;
        assert!(n <= CEILING, "to_rtf made {n} allocations, ceiling {CEILING}");
    }

    #[test]
    #[ignore]
    fn perf_report() {
        let md = readme();
        let opts = Options { width: 9000, toggled_details: &[], picture: &no_picture, code_char: 110.0 };
        let c = &markdown::tests::C;
        let (rtf, a) = allocs(|| markdown::to_rtf(&md, c, &opts));
        row("md_to_rtf_200k", min_ms(10, || markdown::to_rtf(&md, c, &opts)), a);
        println!("  rtf size {} bytes", rtf.len());

        let pic = noise(3 << 20);
        let (_, a) = allocs(|| crate::preview::to_hex(&pic));
        row("picture_hex_3mb", min_ms(5, || crate::preview::to_hex(&pic)), a);

        let text = big_text();
        let bytes = text.as_bytes();
        let (_, a) = allocs(|| crate::encoding::detect(bytes));
        row("detect_5mb", min_ms(5, || crate::encoding::detect(bytes)), a);
        let enc = crate::encoding::detect(bytes);
        let (wtext, a) = allocs(|| crate::encoding::decode(bytes, enc));
        row("decode_5mb", min_ms(5, || crate::encoding::decode(bytes, enc)), a);

        unsafe {
            let null = std::ptr::null_mut();
            let st = wide("STATIC");
            let parent = CreateWindowExW(0, st.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 900, 700, null, null, null, std::ptr::null());

            // The RichEdit parse of the RTF alone, then the whole render (to_rtf + parse).
            let h = crate::preview::create(parent, 1);
            SetWindowPos(h, null, 0, 0, 900, 700, SWP_NOZORDER);
            row("richedit_load_rtf", min_ms(5, || crate::preview::load_rtf(h, rtf.clone())), 0);
            let (_, a) = allocs(|| crate::preview::render(h, &md, None, &[]));
            row("preview_render_200k", min_ms(5, || crate::preview::render(h, &md, None, &[])), a);

            let ed = wide("Edit");
            let style = WS_CHILD | WS_VSCROLL | WS_HSCROLL | ES_MULTILINE as u32;
            let e = CreateWindowExW(0, ed.as_ptr(), std::ptr::null(), style, 0, 0, 900, 700, parent, null, null, std::ptr::null());
            SendMessageW(e, EM_SETLIMITTEXT, 0, 0);
            let mut z = wtext.clone();
            z.push(0);
            row("edit_settext_5mb", min_ms(5, || SetWindowTextW(e, z.as_ptr())), 0);
            DestroyWindow(parent);
        }
    }
}
