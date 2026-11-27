# Changelog

<!-- Newest on top. Format: `YYYY-MM-DD — version: what changed`. -->

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
