## From the old code

The old code had it partly built, about 3,000 lines, and its approach can be reused.

Lessons:
- Live checks are outstanding: text and images both ways, and no WebKit Paste popup.
- Older desktops have clipboard off on the server; check the page's settings before reading the Mac clipboard.
- The guest view is a separate child webview, so drops must be caught on both it and the shell window.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/clipboard.rs`
- `app/SiloUI/src-tauri/engine/src/clipboard.rs`
- `app/SiloUI/src-tauri/engine/src/viewer_clipboard.rs`
- `app/SiloUI/src-tauri/src/commands/viewer_clipboard.rs`
- `app/SiloUI/src-tauri/src/viewer_shortcuts.rs`
- `docs/SiloUI-DESKTOP.md#L826`
- `app/SiloUI/src/desktop/viewer-file-drop.tsx`
- `app/SiloUI/src-tauri/src/commands/transfer.rs`
- `docs/SiloUI-FILES.md#L160`

Calls: `clipboard.read`, `clipboard.write`, `clipboard.transfer`.
