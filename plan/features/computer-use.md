## Decided

- 2026-11-17, owner: an agent drives the screen of a macOS computer as well as a Linux desktop.

## Notes

- Screenshot and mouse and keyboard input exposed through the engine and MCP, usable by any harness (Anthropic computer use, Cua Driver), without depending on a closed vendor app.

## From the old code

The old code had it built, about 7,000 lines, and it needs reworking.

Lessons:
- Chromium builds its web accessibility tree only after an AT-SPI client asks for extended properties.
- Turn the driver's telemetry and update check off however it is started.
- Never replay a click or key after the driver restarted; only side-effect-free calls are repeated.
- Agents stop at 'selected' cues instead of checking the effect; computer-use tasks need to verify the outcome.
- Computer use that needs a large per-device vendor download breaks 'created means ready'.
- The driver's telemetry and update checks are on by default and are turned off in the guest.
- Agents start MCP servers from shells that predate the desktop session; the launcher must read the live session's D-Bus address.
- Only a person at the device may pick auto approval.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/computer_use.rs`
- `app/SiloUI/src-tauri/engine/src/computer_use.rs`
- `docs/SiloUI-COMPUTER-USE.md`
- `docs/direction/evidence/T5b.txt`
- `app/SiloUI/src-tauri/guest/silo-computer-use.py`
- `app/SiloUI/src-tauri/guest/silo-accessibility.py`
- `app/SiloUI/src-tauri/guest/macos/silo-computer-use.zsh`
- `app/SiloUI/src-tauri/guest/macos/silo-cua-register.py`
- `app/SiloUI/src-tauri/guest/silo-cua-driver-mcp`
- `app/SiloUI/src-tauri/guest/cua-driver-lock.json`
- `docs/SiloUI-COMPUTER-USE-PLAN.md#L15`
- `docs/research/cua-vs-codex-computer-use-2026-11-03.md`

Calls: `computerUse.setup`, `computerUse.state`, `computerUse.approval`, `computer_use_screenshot`, `computer_use_click`, `fork_gui`, `silo fork --gui`.
