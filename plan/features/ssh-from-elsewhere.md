## Proposed

- Rule: Every SSH path into a computer or between devices accepts keys only, and passwords are refused by default.
- One command on another device reaches a computer and opens the website it serves in that device's browser.

## From the old code

The old code had it built, about 3,000 lines, and it needs reworking.

Lessons:
- Turning access off is free for an agent; turning it on asks and states the address and key fingerprints.
- Enabled does not mean listening: connections never start a stopped computer.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/ssh_access.rs`
- `app/SiloUI/src-tauri/engine/src/ssh_access.rs`
- `docs/SiloUI-MANAGED-SSH.md`
- `app/SiloUI/scripts/test-managed-ssh-live.py`
- `app/SiloUI/src/features/application/pages/ssh-access-panel.tsx`
- `app/SiloUI/src-tauri/src/commands/ssh_access.rs`
- `app/SiloUI/docs/silo-help.html`

Calls: `sshAccess.state`, `sshAccess.save`, `sshAccess.connection`, `access.ssh`.
