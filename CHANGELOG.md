# Changelog

<!-- Newest on top. Format: `YYYY-MM-DD — version: what changed`. -->

- 2026-11-28 — 0.7.0:
  - The Markdown preview looks like GitHub's rendering: GitHub's colors, sizes
    and spacing, underlined h1 and h2, quote bars, code blocks with syntax
    colors, bordered tables, and nested lists numbered 1, i, a.
  - Front matter shows as a table of keys and values, as on GitHub.
  - Alerts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`),
    footnotes, `:emoji:` codes and bare `www.` / `https://` / email links.
  - The HTML that GitHub keeps: alignment, `<img>`, `<details>` that open on a
    click, `<kbd>`, `<sub>`, `<sup>`, `<mark>` and more. Other tags go and
    keep their text. `<script>` and `<style>` show as text.
  - Images from files on your PC show in the preview. Web images show their
    alt text: the preview loads nothing from the internet.

- 2026-11-27 — 0.6.1:
  - Markdown preview: front matter at the top of a file (YAML between `---`
    lines, or TOML between `+++` lines) shows as a block of its own text. Before,
    it showed as a rule and a heading.
  - Inline code in a heading has the size of the heading, and its gray box
    no longer reaches above or below the line.
  - A heading as the first line of a list item (`- # Title`) keeps its bullet
    on the same line.
  - An image whose alt text starts with code or emphasis shows its label
    correctly.

- 2026-11-27 — 0.6.0:
  - **Folder pane:** a tree of one folder at the left of the window, off until
    you turn it on (*File › Open folder*, *View › Folder pane*, the button at
    the left of the menu bar, Ctrl+Shift+E, or a folder on the command line).
    Open files as tabs, see changes on disk at once, and make, rename, delete
    (to the Recycle Bin) or reveal files from the right-click menu.
  - New screenshots.

- 2026-11-27 — 0.5.1:
  - The Markdown preview and the status bar paint correctly when you make the
    window larger.
  - A preview link to a network path (`//server/share/file.md`) does nothing:
    a click could send your Windows sign-in to that server.
  - *File › Recent files*: a file on an unplugged drive keeps its entry, and
    opening a file that is already open moves it to the top.
  - Setup offers to bring over Notepad's pinned files only once, also when you
    leave the box unchecked.
- 2026-11-27 — 0.5.0:
  - Markdown preview: a *Preview* button for `.md` files, also *View › Markdown
    preview* (Ctrl+Shift+M).
  - *File › Recent files*: the last 10 files, seeded once from Notepad's
    recent files.
  - Fixed a crash when typing with the cursor scrolled out of view (word wrap on).
- 2026-10-13 — 0.4.0:
  - Brings over the files pinned to Notepad's taskbar jump list: offered by the
    installer, and *Settings › Import pinned files from Notepad*. Pins to files
    that no longer exist are skipped.
  - *File › Pin to jump list* pins the open file, even with Windows' recent
    items turned off.
  - RENotepad has its own taskbar identity, so its jump list stays the same
    across updates. If you pinned 0.3.0 to the taskbar, unpin it and pin it again.
- 2026-10-13 — 0.3.0:
  - Checks GitHub for a newer version at startup and shows a notice in the
    status bar; *Settings › Check for updates* does it on demand.
  - Installer and portable zip.
  - Windows 11 style dialogs for prompts, Go to line and About.
- 2026-10-13 — 0.2.0: renamed RE:Notepad. Windows 11 look (title bar with tabs,
  flat menu bar, settings gear, status bar with character count), tabs, Save
  all, its own icon, an About box with the story.
- 2026-10-13 — 0.1.0: the full Windows 10 Notepad feature set, dark mode by default.
