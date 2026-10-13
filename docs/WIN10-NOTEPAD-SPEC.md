# Windows 10 Notepad — behavior spec

What RENotepad reproduces, and the evidence behind each rule. Written from
reverse engineering. Contains behaviors only, no Microsoft code.

RENotepad keeps these *behaviors* but wears the Windows 11 *look*: tabs in the
title bar, a flat menu bar, the Win11 status bar. Where tabs change a rule, it
says so below (*RENotepad:*).

## Reference binary

- `notepad.exe` 10.0.19041.5856 (Win10 22H2, x64). SHA-256 `1217174e…0aa0995`.
  Downloaded from Microsoft's symbol server (`msdl.microsoft.com`), looked up through
  the Winbindex entry for 19041.5794 (same timestamp and size key).
  Inbox files are catalog-signed, so `Get-AuthenticodeSignature` reporting
  "NotSigned" is expected.
- Not in this repository: it's Microsoft's file. Neither is the raw REA/Ghidra
  output (strings, imports, decompiled functions); this spec is what was
  learned from it, in plain words. `re/rea-batch.mjs` is the script that drove
  REA.
- Menus, dialogs, accelerators and UI strings live in `notepad.exe.mui`, which the
  symbol server doesn't have (its timestamp is 0). Menu labels and dialog text
  here come from Win10 Notepad as shipped, **not** from a decoded MUI.

Evidence anchors below are function addresses in that build
(`FUN_14000xxxx`, image base 0x140000000).

## Edit control

- A plain `Edit` window. Style: child, visible, `WS_VSCROLL`, `ES_MULTILINE`,
  `ES_NOHIDESEL`, plus `WS_HSCROLL` when word wrap is off. Word wrap is toggled
  by recreating the control, which loses the undo buffer. *(init `FUN_140011edc`)*
- Undo is the EDIT control's single-level toggle. That's authentic, not a bug.
- Text limit is lifted (`EM_LIMITTEXT 0`).
- Uses the Win10 1809 extensions:
  - mixed CR/LF/CRLF are preserved (`ES_EX_ALLOWEOL_ALL`, line-ending mode
    detected from content);
  - pasted line endings are converted to the document's;
  - `ES_EX_ZOOMABLE` gives Ctrl+wheel zoom.
- Ln/Col count file lines, not wrapped lines (`EM_FILELINEFROMCHAR`), so they
  stay right with word wrap on.
- The edit has no caret-moved notification. Notepad hooks
  `EVENT_OBJECT_LOCATIONCHANGE` for `OBJID_CARET` (it imports `SetWinEventHook`).

## Encoding detection (`FUN_140008074`, load path `FUN_14000ef78`)

In this order:

1. `EF BB BF` → UTF-8 with BOM (needs ≥ 3 bytes).
2. `FF FE` → UTF-16 LE.
3. `FE FF` → UTF-16 BE.
4. `IsTextUnicode` with every test enabled. A hit means UTF-16 LE, *unless* the
   only flag is `IS_TEXT_UNICODE_STATISTICS` and the file is under 100 bytes
   (that is the "Bush hid the facts" guard).
5. Strict UTF-8 (`MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS)`): valid
   means UTF-8; `ERROR_NO_UNICODE_TRANSLATION` means ANSI.

- An empty file uses the default encoding, UTF-8.
- `/A` and `/W` on the command line force ANSI or UTF-16 on open.
- Files of 1 GiB or more are refused.
- **Save:** UTF-16 is always written with a BOM. ANSI conversion checks
  `lpUsedDefaultChar` and warns before losing characters ("This file contains
  characters in Unicode format which will be lost…"). *(`FUN_14000e424`)*
- Status-bar names: `ANSI`, `UTF-16 LE`, `UTF-16 BE`, `UTF-8`, `UTF-8 with BOM`.
- GB18030 (code page 54936) exists in Win10 as encoding #6. It's left out
  here; it only matters on a Chinese system code page.

## .LOG and Time/Date (`FUN_14000e238`, `FUN_14000ef78`)

- If the first four characters of the opened document are exactly `.LOG`
  (case-sensitive), the caret moves to the end and Notepad inserts
  `CRLF + <time> <date> + CRLF`. The document becomes modified.
- **F5** inserts `<time> <date>` with no line breaks.
- Time uses `TIME_NOSECONDS`; date uses `DATE_SHORTDATE`, both in the user's
  locale. Time comes before date, e.g. `2:26 PM 10/13/2026`.

## Find / Replace (`FUN_14001c898`, `FUN_14001c7a0`, `FUN_14001c820`)

- Search uses `FindNLSString`. Flags are `FIND_FROMSTART` going down,
  `FIND_FROMEND` going up, plus `NORM_IGNORECASE` unless Match case is on.
- Going down searches from the end of the selection; going up searches before
  it. If the whole document is selected, the search starts from the top.
- Wrap around: when nothing is found, search the other part. The status bar
  then says "Found next from the top" / "Found next from the bottom".
- Not found: message "Cannot find "<text>"", titled "Notepad".
  *(Wording recalled, not observed: the string lives in the MUI.)*
- F3 / Shift+F3 work even while the Find dialog has focus. A separate
  accelerator table named `GlobalAcc` is loaded (observed in the decompile).
  Its contents live in the MUI, so the F3 / Shift+F3 binding is *recalled, not
  observed*. With no search string, F3 opens Find.
- Replace replaces the selection only if it matches, then finds the next one.
  Replace All goes start to end (here it's a single undo step).
- Settings persisted: `fMatchCase`, `fWrapAround`, `fReverse`, `searchString`,
  `replaceString`.

## Files, title and prompts

- Title is `<name> - Notepad`, prefixed with `*` when modified.
  `Untitled` when there's no file.
- Command line: the rest of the line is one filename, quoted or not.
  - With no extension and no such file, `.txt` is appended.
  - If the file is missing: "Cannot find the <path> file. Do you want to
    create a new file?" — Yes creates it, No starts Untitled, Cancel exits.
    *(Prompt wording recalled, not observed.)*
  *(init `FUN_140011edc`, `CreateFileW` with `OPEN_ALWAYS`)*
- On New, Open, drag-drop and close: "Do you want to save changes to
  <path|Untitled>?" with Save / Don't Save / Cancel. *(Wording recalled, not
  observed.)*
  *RENotepad:* New, Open and drag-drop open a tab instead (Open reuses an
  untouched Untitled tab and switches to a file that's already open), so the
  prompt comes when a modified tab or the window is closed. Closing the last
  tab closes the window, as in Windows 11.
- Opened files go to the shell's recent list (`SHAddToRecentDocs`).
- The Open and Save As dialogs both have an Encoding combo. Open defaults to
  Auto-Detect.

## Settings (`Software\Microsoft\Notepad` value names)

These are the value names Win10 uses:

- Font: `lfFaceName` and the other `lf*` fields, plus `iPointSize`
  (tenths of a point). RENotepad defaults to Consolas 11 (*recalled, not
  observed*). The binary itself contains the string `Lucida Console`, which is
  the classic default and is likely the fallback.
- View: `fWrap`, `StatusBar`.
- Window: `iWindowPosX`, `iWindowPosY`, `iWindowPosDX`, `iWindowPosDY`.
- Page margins: `iMarginTop`, `iMarginBottom`, `iMarginLeft`, `iMarginRight`
  (thousandths of an inch).
- Find state: as listed under Find / Replace.

RENotepad uses the **same names under `HKCU\Software\RENotepad`**, so it
never touches the real Notepad's settings. It adds `fDarkMode` and
`fMaximized`.

- Font height is `-MulDiv(iPointSize, dpi, 720)`.

## Printing (`FUN_14001d494`)

- Margins come from Page Setup, in thousandths of an inch, offset by the
  printer's physical offsets.
- Header and footer lines are reserved inside the margins: the header is the
  file name, the footer is `Page N`.
- Text is drawn with `DrawTextEx(DT_WORDBREAK | DT_EXPANDTABS | DT_NOPREFIX |
  DT_EDITCONTROL)`, 8-character tabs, and paged with `uiLengthDrawn`.
- Win10's header/footer codes (`&f`, `&p`, …) can be edited in its Page Setup;
  here they're fixed.

## Deliberately left out

These are in the Win10 binary (strings and telemetry events) but removed here:

- Search with Bing (`EM_ENABLESEARCHWEB`), Send Feedback (`feedback-hub://`),
  View Help (fwlink).
- All telemetry events (`TelemetryAssert`, `FileOpenComplete`, …).
- Autosave for the restart manager.
- The genuine-status check.
