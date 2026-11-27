<div align="center">

<img src="docs/icon.png" width="96" alt="">

# RE:Notepad

**Windows 10's Notepad, reverse-engineered and rebuilt in Rust, wearing Windows 11's look.**
Dark by default. No Copilot, no Bing, no sign-in, no telemetry. Just text.

[**Download**](https://github.com/nikkoxgonzales/RENotepad/releases/latest) ·
[Features](#features) ·
[The story](#the-story) ·
[Make it your default](#make-it-your-default) ·
[Build from source](#build-from-source) ·
[Changelog](CHANGELOG.md)

![RE:Notepad in dark mode](docs/screenshots/dark.png)

</div>

## Why

|  | Windows 10 Notepad | Windows 11 Notepad | RE:Notepad |
|---|:---:|:---:|:---:|
| Classic Notepad behavior (encodings, `.LOG`, line endings) | ✓ | mostly | ✓ |
| Tabs | | ✓ | ✓ |
| Dark mode | | ✓ | ✓ (default) |
| Windows 11 look | | ✓ | ✓ |
| Markdown | | formatting toolbar | read-only preview |
| Folder pane | | | ✓ (off by default) |
| Copilot, Bing search, sign-in | | ✓ | |
| Size | built in | Store app | one exe, about 700 KB |

RE:Notepad keeps what Windows 10's Notepad did and how it did it, and takes
only the look from Windows 11.

## Features

It's the classic Notepad, minus the extras.

- **Look:**
  - The Windows 11 style: icon and tabs in the title bar, a flat File / Edit /
    View menu bar, a settings gear.
  - Dark mode by default, light mode one click away.
  - Text inset 16 px from the edges, and scrollbars only when there's
    something to scroll.
  - Prompts, Go to line and About are Windows 11 style dialogs over the
    dimmed window, with the main button in your accent color.
- **Tabs:** a dot on a tab means unsaved changes. Opening a file that's
  already open switches to its tab.
- **File:** New tab, New window, Open, Open folder, Recent files, Save, Save as (with an
  Encoding picker), Save all, Page setup, Print, Close tab, Close window.
- **Edit:** Undo, Cut, Copy, Paste, Delete, Find, Find next/previous, Replace,
  Go to, Select all, Time/Date, Font.
- **View:** Zoom, Status bar, Word wrap, Folder pane, Markdown preview.
- **Status bar:** line and column, the character count ("5 of 120
  characters" while text is selected), then zoom, line endings and encoding.
- **Encodings:** ANSI, UTF-8, UTF-8 with BOM, UTF-16 LE/BE, detected the way
  Windows 10 does it. Line endings are kept as they are, even mixed in one file.
- **`.LOG` files** get a timestamp each time they're opened.
- **Command line:** `RENotepad [/A|/W] [file]`, or `RENotepad /P file` to print
  to the default printer. `RENotepad C:\notes` opens a folder in the folder pane.

### Markdown preview

The one addition Windows 10 Notepad never had. A *Preview* button appears in
the menu bar for `.md` files. It shows the file formatted and read-only, from
what's in the editor, unsaved changes too. *Edit* or Esc goes back.

| The file | The preview |
|---|---|
| ![A Markdown file in the editor](docs/screenshots/markdown-source.png) | ![The same file in the preview](docs/screenshots/markdown-preview.png) |

- It looks like GitHub's rendering, in GitHub's dark or light colors:
  headings, lists, task lists, quotes, tables, code blocks with syntax colors,
  alerts (`> [!NOTE]`), footnotes, `:emoji:` codes, and bare URLs as links.
- Front matter at the top of the file shows as a table, as on GitHub.
- The HTML that GitHub keeps works too: `<p align="center">`, `<img>`,
  `<details>` (click the summary to open it), `<kbd>`, `<sub>`, `<sup>` and more.
- Web links open in your browser. A link to another Markdown file opens it in
  a tab.
- Images show from files on your PC. The preview never runs anything or loads
  anything from the internet: a web image shows its alt text.

### Folder pane

The second addition, off until you turn it on: *File › Open folder*,
*View › Folder pane*, the pane button at the left of the menu bar, or
Ctrl+Shift+E. It shows one folder as a tree at the left of the window.

![The folder pane next to a Markdown file](docs/screenshots/folder-pane.png)

- Double-click a file, or press Enter, to open it in a tab. The file of the
  active tab shows in bold, and the tree follows you from tab to tab.
- Folders load when you open them, and the tree keeps up with changes on disk
  by itself. Hidden files stay hidden unless you ask (right-click › *Show
  hidden items*).
- Right-click for Open in new window, Reveal in File Explorer, Copy path,
  New file, New folder, Rename (F2) and Delete (Del). Delete goes to the
  Recycle Bin. A renamed file's open tab follows the new name, and a deleted
  file's tab stays open so you can still save the text.
- Drag a file from the pane onto the editor to open it. Drag the pane's edge
  to resize it. Esc goes back to the editor. The pane, its folder and its
  width come back next time.

### Keyboard shortcuts

| Keys | Does |
|---|---|
| Ctrl+N, Ctrl+T | New tab |
| Ctrl+Shift+N | New window |
| Ctrl+W, Ctrl+F4, middle-click a tab | Close tab |
| Ctrl+Shift+W | Close window |
| Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PgDn / Ctrl+PgUp | Next / previous tab |
| Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+P | Open, Save, Save as, Print |
| Ctrl+F, Ctrl+H, Ctrl+G | Find, Replace, Go to |
| F3 / Shift+F3 | Find next / previous |
| F5 | Insert time and date |
| Ctrl+Plus / Ctrl+Minus / Ctrl+0, Ctrl+wheel | Zoom in / out / reset |
| Ctrl+Shift+M | Markdown preview |
| Ctrl+Shift+E | Folder pane: show, focus, hide |
| F2, Del, F5 (in the folder pane) | Rename, Delete, Refresh |

| Light mode | Windows 11 dialogs |
|---|---|
| ![Light mode](docs/screenshots/light.png) | ![Save prompt](docs/screenshots/save-prompt.png) |

## The story

Notepad used to be the one program on Windows that never changed. You opened
it, you typed, you closed it. Windows 11's Notepad looks great, but it has
picked up Copilot, a Bing search button, sign-in prompts and a formatting
toolbar. Windows 10's Notepad still just edited text, but it looks its age.

I wanted both: the old Notepad's behavior with the new Notepad's looks.

So I took the real thing apart. I downloaded `notepad.exe` from Windows 10
22H2 (version 10.0.19041.5856) straight from Microsoft's symbol server, opened it in
[Ghidra](https://ghidra-sre.org/) through [REA](https://github.com/morluto/rea),
and read how it actually works. A few of the things I found:

- **How it guesses an encoding.** It checks for a byte-order mark, then asks
  Windows' `IsTextUnicode`, with a special guard for files under 100 bytes.
  That guard is the fix for the famous "Bush hid the facts" bug. Then it tries
  strict UTF-8, and falls back to ANSI.
- **How it keeps line endings.** Since 2018 Notepad keeps Windows (CRLF), Unix
  (LF) and old Mac (CR) line endings exactly as they are, even mixed in one file.
- **The `.LOG` trick.** A file whose first line is `.LOG` gets a timestamp
  appended every time you open it, so it works as a diary.
- **Find, Replace, Go To,** word wrap, zoom, printing: all the small rules.

I wrote every rule down in plain words in
[docs/WIN10-NOTEPAD-SPEC.md](docs/WIN10-NOTEPAD-SPEC.md), with the function in
the binary each one came from. Then I wrote the app again from that spec, from
scratch, in Rust. The binary was studied, not copied: no Microsoft code is
inside.

Then came the Windows 11 look. I measured Windows 11 Notepad from screenshots,
pixel by pixel, and rebuilt it by hand:
- the tabs in the title bar;
- the flat menu bar with the settings gear;
- the status bar;
- the 16 px text margin;
- the rounded dialogs with the button in your accent color.

It's all plain Win32 and GDI, so the whole app is a single exe of about 700 KB
that opens instantly.

The **RE** stands for **r**everse **e**ngineering. The app itself is called
**RENotepad**, because a colon can't go in a file name.

## Download

Get it from the [latest release](https://github.com/nikkoxgonzales/RENotepad/releases/latest):

- **Installer** (`RENotepad-x.y.z-setup.exe`), recommended. It installs for your
  user account (no admin needed; it can install for all users too) and
  registers RENotepad with Windows, so it shows up in *Open with* and in
  *Settings › Default apps*. It also adds a Start menu entry and an uninstaller.
- **Portable** (`RENotepad-x.y.z-portable.zip`). Unzip it anywhere and run
  `RENotepad.exe`. Nothing is installed. Settings are still kept in the
  registry (`HKCU\Software\RENotepad`), the same way Windows' Notepad does it.

Needs Windows 10 or 11, 64-bit. The files aren't code-signed yet, so
SmartScreen may warn the first time: choose *More info › Run anyway*. Each
release has a `SHA256SUMS.txt` you can check the downloads against.

## Make it your default

Windows doesn't let any installer make itself the default app; you choose. After installing:

- **For all text files at once:** open *Settings › Apps › Default apps*,
  search for **RENotepad**, and pick it for the file types you want
  (`.txt`, `.log`, `.ini`, `.md`, `.json`, `.csv`…). The installer's last page
  has a checkbox that opens this page for you.
- **For one file type:** right-click a file, then *Open with › Choose another
  app*. Pick **RENotepad** and tick *Always*.

The installer can also add **Edit with RENotepad** to every file's right-click
menu. On Windows 11 it's under *Show more options*.

## Jump list and recent files

**Bring your pinned files.** The files you pinned to Notepad's taskbar jump
list (right-click Notepad on the taskbar) can come along. The installer offers
it the first time you install. Afterwards, *Settings › Import pinned files from
Notepad* does it any time.
- It reads Windows 11's Notepad, and also Windows 10's classic Notepad, though
  that's untested so far.
- Notepad keeps its own pins; RENotepad only reads them.
- Pins to files that no longer exist (moved or deleted since you pinned them)
  are skipped, and the import tells you how many. If Windows refuses, the
  import says so.

**RENotepad's own jump list** works like Notepad's. Files you open show up under
*Recent*, and you can pin any of them. Recent stays empty while Windows'
"Show recently opened items in Start, Jump Lists, and File Explorer" setting is
off; pinned files stay either way. To pin the file you have open, use
*File › Pin to jump list*: it works whatever that setting is.

**File › Recent files** lists the last 10 files you opened or saved. The first
time it starts, RENotepad fills it with the files in Notepad's own *Recent*
jump list that still exist. That happens once; Notepad's list is only read.

## Updates and privacy

RENotepad never sends anything about you or your files anywhere. Its one
network request is the update check:
- What it sends: a single HTTPS request to GitHub's public API asking for this
  project's latest release. The request carries no data except the app's name
  and version.
- When it runs: at startup, in the background.
- What you see: a short notice in the status bar only when a newer version
  exists. Click the notice, or use *Settings › Check for updates*, to see the
  details and the download link.
- How to turn it off: *Settings › Check for updates at startup*.

RENotepad never downloads or installs anything by itself.

## Known limits

- **Find and Replace** open as a small dialog, the Windows 10 way, rather than
  Windows 11's bar inside the window.
- **Font dialog:** only partly dark, because Windows draws its font lists itself.
- **Open/Save dialog:** follows the system theme, not the app's.
- **Page Setup:** the header and footer are fixed (file name / `Page N`).
- **Character count:** in documents that mix line-ending types, it can be off
  by the number of mixed line breaks.
- **Screen readers** read the Windows 11 dialogs' titles but not their
  buttons, which are drawn by the app. The keyboard works fully: Tab, arrows,
  Enter, Esc, and Alt+letter.
- **On Windows 10** the window has no 1-pixel border along its top edge.
- **Importing Notepad's pins** uses the jump list interface Explorer itself uses,
  which Microsoft doesn't document. If a Windows update changes it, the import
  says it couldn't, and the editor is unaffected.
- **Portable copy:** Windows shows jump list entries only for file types the app
  is registered for, which the installer does. A portable copy's jump list may
  stay empty.
- **Markdown preview:** images show as a placeholder, quotes have no side bar,
  code blocks are shaded behind the text only, and a very wide table runs past
  the window's right edge. Find works on the text, not on the preview.
- **Folder pane:** a folder shows its first 2,000 items. Junctions and links
  to folders show but don't open in place (right-click › *Set as root* opens
  one). Folders aren't added to the jump list.
- **Left out on purpose:** the formatting toolbar, Copilot, spell check and
  session restore.

## Build from source

You need the Rust MSVC toolchain and the Windows SDK, which provides `rc.exe`
for the icon and version info.

```sh
cargo build --release          # target\release\RENotepad.exe
cargo test                     # encoding, find/replace, .LOG, Markdown, command line, version checks
pwsh scripts/package.ps1       # dist\: installer, portable zip, SHA256SUMS.txt
```

`package.ps1` downloads a pinned, hash-checked [Inno Setup](https://jrsoftware.org/isinfo.php)
into `.tools\` the first time it runs, in portable mode, so nothing is
installed system-wide. Pushing a version tag (`v0.5.1`) makes GitHub Actions
run the same script and publish the release.

**UI tests.** `tests/ui/` drives the real exe through window messages without
ever taking your keyboard or covering your screen. See
[tests/ui/README.md](tests/ui/README.md).

```sh
pwsh tests/ui/smoke.ps1      # find/replace/go to, dialogs, layout, tabs, settings
pwsh tests/ui/menus.ps1      # menu bar keyboard navigation
pwsh tests/ui/preview.ps1    # Markdown preview
pwsh tests/ui/folderpane.ps1 # folder pane (shows a normal window: the rename needs focus)
pwsh tests/ui/recent.ps1     # File > Recent files (on made-up jump lists only)
pwsh tests/ui/scrollbars.ps1 # scrollbar fitting while the window resizes
pwsh tests/ui/update.ps1     # update check (needs the internet)
pwsh tests/ui/pins.ps1       # importing Notepad's pins (on made-up jump lists only)
pwsh tests/ui/installer.ps1  # install, reinstall, uninstall (only if not installed)
pwsh tests/ui/shot.ps1 -Out shot.png  # a screenshot, without taking focus
```

### Project layout

- `src/main.rs`: entry point and message loop.
- `src/app.rs`: main window, tabs, menus and commands.
- `src/chrome.rs`: the Windows 11 title bar and menu bar.
- `src/status.rs`: the status bar.
- `src/content_dialog.rs`: the Windows 11 dialog (card, accent button, dimmed
  owner).
- `src/dialogs.rs`: Find/Replace, the message box, Go to line, About.
- `src/update.rs`: the update check.
- `src/jumplist.rs`: the taskbar identity, the jump list and the imports from
  Notepad.
- `src/markdown.rs`, `src/preview.rs`: the Markdown preview (Markdown to RTF,
  unit-tested; the read-only RichEdit that shows it).
- `src/explorer.rs`: the folder pane (tree, folder listing and watcher
  threads, file actions).
- `src/encoding.rs`, `src/text.rs`: the pure rules, unit-tested.
- `src/theme.rs`: dark and light palettes and drawing helpers.
- `src/print.rs`, `src/filedlg.rs`, `src/settings.rs`.
- `assets/`: the icon, plus `make_icon.py`, which draws it (needs Pillow).
- `installer/RENotepad.iss`: the Inno Setup installer.
- `scripts/package.ps1`: builds the release assets.
- `docs/WIN10-NOTEPAD-SPEC.md`: the behavior spec, with the reverse-engineering
  evidence.
- `re/`: the script that drove REA.
- `tests/ui/`: the focus-free UI tests.

![About RE:Notepad](docs/screenshots/about.png)

## Credits

- [REA](https://github.com/morluto/rea) and [Ghidra](https://ghidra-sre.org/) for the reverse engineering.
- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) for reading Markdown.
- [Inno Setup](https://jrsoftware.org/isinfo.php) for the installer.
- Microsoft, for twenty-odd years of Notepad. RE:Notepad is an independent
  project, not affiliated with or endorsed by Microsoft. Windows and Notepad
  are trademarks of Microsoft Corporation.

## License

[MIT](LICENSE) © 2026 Nikko Gonzales
