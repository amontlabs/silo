## From the old code

The old code had it partly built, about 900 lines, and it would be written again.

Lessons:
- A canary showed removed logins still in the default export's layers.
- A canary check found the login bytes in the file: a fork's shared read-only disk layers are written whole, so trimming the copy does not reach them.
- Checkpoint, macOS and container exports keep logins and warn first.

Where it lived, at the tag legacy-2026-11-16:
- `docs/direction/evidence/T14.txt`
- `docs/direction/checks/t14_export_scrub_check.sh`
- `docs/SiloUI-CLI.md#sharing-a-computer-as-a-file`
- `docs/DIRECTION.md#L174`
- `docs/direction/PLAN.md#L404`
- `app/SiloUI/src-tauri/engine/src/backup_controller/scrub.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/scrub_env.rs`

Calls: `silo export --include-logins`, `silo export`, `exports.check`.
