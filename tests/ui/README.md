# UI tests

Scripts that drive the real exe without stealing focus. The exe starts
**hidden** and is driven only through window messages: `WM_COMMAND` for menu
commands, `WM_SETTEXT` / `EM_REPLACESEL` for typing, posted mouse messages for
the tab strip, a posted `WM_SYSCHAR` for Alt+letter. Nothing is sent as real
keystrokes, so whatever you're working in is never touched.

When a check needs real layout or pixels, `Show-Ghost` (in `lib.ps1`) shows the
window at 1/255 opacity, click-through, without a taskbar button and without
activating it, and `Snap` captures it with `PrintWindow`. A window moved off
every monitor doesn't work: DWM skips it and the capture comes out blank.

| Script | What it checks |
|---|---|
| `smoke.ps1` | Find (Down, wrap-around, Alt+R label mnemonic, closing Find under its "Cannot find" box), Replace All, Go To (and its refused-line box); Win11 dialogs disable and dim the owner; 16 px text inset, also after zoom; scrollbars only when there's something to scroll; tabs (Ctrl+N, Ctrl+Tab, `+` button, middle-click close, the save prompt, close-window prompt); settings written on close. Exits 1 on any FAIL. |
| `menus.ps1` | Alt+F opens File, Left/Right walk the menu bar and wrap to the gear, Esc closes. A real popup menu has to open, so one flashes near the top-left of the screen for about a second. It still never takes focus. |
| `shot.ps1` | Screenshot only (`-Out file.png [-Light] [-Tabs N] [-Lines N] [-Dialog save\|goto\|about] [-File notes.txt]`), cropped to the visible window. The README screenshots in `docs/screenshots/` are made with it. `-Dialog` composes the dialog over the window darkened the way the app dims it, with DWM's rounded corners and border drawn in, since `PrintWindow` sees neither. |
| `update.ps1` | Needs the internet. The startup update check stays quiet (no dialog, a clickable status-bar notice); clicking the notice and *Settings › Check for updates* both answer with a dialog; a repo with no release says so. Uses `RENOTEPAD_UPDATE_REPO` to point the check at other repos. |
| `lib.ps1` | Shared helpers: window lookup, message helpers, `Show-Ghost`, `Snap`. |

```sh
cargo build && pwsh tests/ui/smoke.ps1 && pwsh tests/ui/menus.ps1
pwsh tests/ui/smoke.ps1 -Exe target\release\RENotepad.exe   # release too
```

Limits:
- **Hover highlights can't be captured.** The app asks Windows to report when the
  mouse leaves; with no real cursor over the ghost, that report arrives at once
  and the highlight clears before the capture.
- `smoke.ps1` closes the app normally, so it writes `HKCU\Software\RENotepad`.
  It exports any existing key first, runs from default settings, and restores
  the export in a `finally` block. A run killed outright (not Ctrl+C) skips
  that; the export stays in `%TEMP%
enotepad-smoke-settings.reg`.
- Windows 11 dialogs over a ghost are ghosts too: same opacity, click-through,
  never activated. So dialogs opened after `Show-Ghost` never appear on
  screen. The Find dialog (a classic one) still flashes for a moment.
