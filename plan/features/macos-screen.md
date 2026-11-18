## Notes

- An agent drives this screen too: see `macos-computer-use` (2026-11-17).

## From the old code

The old code had it built and driven end to end, about 1,500 lines, and its approach can be reused.

Lessons:
- The guest owns Command+C/V, so the window takes no keyboard shortcuts of its own.
- pbcopy and pbpaste interpret RTF headers; use NSPasteboard with explicit types.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-MACOS-COMPUTERS.md#L13`
- `app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/input.rs`
- `app/SiloUI/src-tauri/src/commands/macos_remote_viewer.rs`
- `app/SiloUI/src/desktop/macos-remote-viewer.tsx`
- `docs/SiloUI-MACOS-COMPUTERS.md#L515`
- `app/SiloUI/src-tauri/engine/src/macos_computers/guest_clipboard.rs`

Calls: `display.show`, `macos.display.session`, `macos.display.resize`.
