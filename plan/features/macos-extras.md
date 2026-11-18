## Open

- Needs `export`, which comes later in the order.

## From the old code

The old code had it partly built, about 2,000 lines, and it needs reworking.

Lessons:
- The framework's private _VZVNCServer listens on every interface; the guest's own Screen Sharing reachable only from the user is safer.
- The suspected cause is the Recovery partition blocking expansion; nothing retries it later.
- Screen Sharing does not work in Setup Assistant or Recovery; Apple's VNC server is private API.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-MACOS-COMPUTERS.md#L605`
- `app/SiloUI/src-tauri/engine/src/macos_computers/remote_display.rs`
- `docs/SiloUI-CONNECTIONS.md#L280`
- `docs/direction/OWNER_REVIEW.md#L100`
- `docs/research/macos-remote-display-2026-10-10.md#L18`
- `docs/SiloUI-MACOS-COMPUTERS.md#L691`
- `app/SiloUI/src-tauri/engine/src/backup/macos_archive.rs`

Calls: `macos.snapshot`, `macos.create`, `macos.action`, `macos.display.connect`, `macos.display.stream`, `macos.display.resize`, `exports.create`, `exports.import`.
