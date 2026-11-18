## Proposed, not confirmed by the owner

### For the owner to settle

- Which path does the viewer take: Selkies (where the old code ended), Wayland with Waypipe (its flags need careful choosing), or another?
  - Recommendation: Selkies, unless a short trial shows Waypipe clearly better, because its lessons from the old code are already paid for.

## From the old code

The old code had it partly built, about 11,800 lines, and it needs reworking.

Lessons:
- desktop.action also installs Node and Claude Code (SetupAgentTools), which belongs with agents install.
- WKWebView's user agent lacks the Safari token, so KasmVNC read the clipboard on every click and macOS showed a Paste prompt.
- The viewer moved from KasmVNC to Selkies 2.0.0; its settings come from localStorage, not URL parameters.
- Softness came from CSS scaling and a runtime relay stall, not the network: keypress-to-picture p90 went from 405 ms to 35 ms.
- Agent success does not prove human input works in the viewer.
- Human and agent input are concurrent and the human owns conflicts; no leases.
- Selkies decodes audio only through WebCodecs, present in system WebKit from Safari 26; detect the engine, not the OS version.
- Selkies by default kills the first viewer when a second opens the same display.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/desktop.rs`
- `app/SiloUI/api/src/methods/display.rs`
- `app/SiloUI/src-tauri/silod/src/methods/display.rs`
- `app/SiloUI/src-tauri/engine/src/desktop.rs`
- `docs/direction/evidence/M.txt`
- `app/SiloUI/src-tauri/src/commands/desktop_viewer.rs`
- `app/SiloUI/src/desktop/linux-desktop-viewer.tsx`
- `docs/SiloUI-DESKTOP.md#L536`
- `docs/SiloUI-LUDA-AGENT-TESTS.md#L146`
- `docs/SiloUI-DESKTOP-VIEWER-DIRECTION.md#L45`
- `docs/research/desktop-stream-tuning-2026-10-09.md`
- `docs/SiloUI-DESKTOP-STACK-ASSESSMENT.md`

Calls: `desktop.status`, `desktop.connect`, `desktop.action`, `display.show`, `display.close`, `display.viewer.open`, `display.viewer.close`, `display.viewer.audio.set`, `display.viewer.screen.reset`, `display.viewer.sound.check`, `display.viewer.sound.cancel`.
