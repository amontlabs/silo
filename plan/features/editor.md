## From the old code

The old code had it partly built, about 7,500 lines, and it needs reworking.

Lessons:
- Folder names must be percent-encoded per segment, and names with control characters refused.
- SSH and SFTP first ran in / instead of the home folder and SFTP returned failure after a good transfer; both fixed in the runtime patch.
- When Silo cannot edit the user's SSH config it shows the exact Include line to add.
- Offer only apps the handoff accepts (Xcode was the default editor on many Macs).
- The URL crate silently drops tabs and newlines, changing the folder identity.
- macOS agents open with ssh -F and a per-computer config so ~/.ssh/config stays untouched.
- URL parsers drop tabs and newlines instead of encoding them, so refuse control characters in folder names or the editor opens another path.
- OpenSSH refuses a private key with any group or other bits; repair modes to 0600.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/terminal.rs`
- `app/SiloUI/src-tauri/engine/src/editor.rs`
- `docs/SiloUI-EDITOR-HANDOFF.md`
- `docs/SiloUI-EDITOR-HANDOFF.md#L111`
- `app/SiloUI/src-tauri/src/commands/editor.rs`
- `app/SiloUI/src/features/application/components/editor-include-notice.tsx`
- `docs/research/editor-folder-identity-2026-10-02.md`
- `docs/product/JOURNEYS.md#L29`
- `app/SiloUI/host/src/terminal.rs`
- `app/SiloUI/host/src/applications/launch.rs`
- `docs/product/JOURNEYS.md#L26`
- `app/SiloUI/src/features/preferences/components/application-preference-fields.tsx`

Calls: `editor.command`, `editor.includeNotice`, `guest.sshAlias.ensure`, `silo ssh-setup`, `ssh main.silo-dev`.
