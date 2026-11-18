## Proposed

- Rule: The export format is plain and documented, so a user can take computers and their data out of Silo and leave.

## Notes

- Share the disk, not memory: memory snapshots only restore on the same CPU and runtime. The recipient cold-boots and running programs restart.
- Secrets travel as names only, resolved to the recipient's own values.
- Full computers for Linux only; a macOS share goes as recipe plus changes, because of Apple's licence.
- Apply the recipe's scrub paths and show the sender what is included (for example `.env` files).
- Store on the team's own machine or bucket first, not a hosted link.

## From the old code

The old code had it partly built, about 23,900 lines, and it needs reworking.

Lessons:
- Logins (Codex, Claude, gh) must be removed from a default export and the result names what it removed (T14).
- An import is closed to incoming connections, and keeps that through later policy changes.
- Removing login files from a fork does not remove their bytes: shared read-only layers go into the file whole.
- Exports carry disks only, never memory; a running Linux checkpoint with memory cannot be exported.
- Not an MCP tool: the file may hold logins, so it stays the person's decision.
- The guest image travels by digest, not inside the file; a different digest is refused.
- Exports never carry memory; a running Linux checkpoint cannot be exported.
- Exports must not use --with-parents: a layer written twice makes load refuse it.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/exports.rs`
- `app/SiloUI/src-tauri/engine/src/backup.rs`
- `app/SiloUI/src-tauri/engine/src/backup_controller.rs`
- `docs/direction/evidence/T14.txt`
- `docs/direction/evidence/T7.txt`
- `app/SiloUI/cli/src/share.rs`
- `app/SiloUI/cli/tests/share.rs`
- `docs/SiloUI-CLI.md#sharing-a-computer-as-a-file`
- `docs/forks-ci/evidence/f3-share/README.md`
- `docs/SiloUI-RUNTIME-BACKUP-FINDINGS.md`
- `docs/THREAT-MODEL.md#L448`
- `docs/SiloUI-VOCABULARY.md#L80`

Calls: `exports.state`, `exports.create`, `exports.check`, `exports.inspect`, `exports.import`, `exports.acknowledge`, `exports.discard`, `exports.dismiss`, `exports.destination.set`, `silo export`, `silo import`, `exports.*`, `exports.transfer`, `exports.file`, `silo --device export`, `silo --device X export`.
