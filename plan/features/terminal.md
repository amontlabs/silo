## Proposed, not confirmed by the owner

### For whoever builds it

- Detail: A new terminal tab or split can open directly in a computer or in a fresh fork.
- Test: Start a process from the app, and it runs with the user's own shell and PATH, under fish as well as bash.
- Test: Run a command-line agent program under tmux in a computer, and it works unchanged.
- Detail: Clicking a file path in a terminal or a log opens that file.
- To find out: For the entry app-terminal, does the app own a renderer or embed an existing terminal view? Study how Ghostty's app builds on libghostty, which gives parsing and terminal state only, and what Conductor does.

## From the old code

The old code had it built and driven end to end, about 2,400 lines, and it needs reworking.

Lessons:
- The editor/terminal command is built by the engine from a fixed agent list; nothing else can be named.
- Showing the SSH name adds an Include line to ~/.ssh/config as a side effect (V G3c).
- Write only an included file, never the user's ~/.ssh/config body.
- Key aliases by computer id, never by name.
- Unsupported terminals must fail clearly instead of opening a local shell that looks like success.
- Ghostty needs Automation permission and the Apple Events entitlement.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/guest.rs`
- `app/SiloUI/api/src/methods/terminal.rs`
- `docs/SiloUI-MANAGED-SSH.md`
- `docs/product/JOURNEYS.md#L26`
- `docs/direction/evidence/V.txt`
- `app/SiloUI/cli/src/access.rs`
- `docs/product/JOURNEYS.md`
- `docs/product/JOURNEYS.md#L27`
- `app/SiloUI/src-tauri/engine/src/guest/ssh_alias.rs`
- `app/SiloUI/src-tauri/engine/src/guest/macos.rs`
- `app/SiloUI/src-tauri/engine/src/guest/commands.rs`
- `app/SiloUI/host/src/terminal.rs`

Calls: `guest.ssh`, `guest.sshAlias.ensure`, `guest.sshAlias.remove`, `guest.prepare`, `terminal.command`, `computers.shell`, `silo ssh-setup`, `sshAccess.save`, `editor.command`.
