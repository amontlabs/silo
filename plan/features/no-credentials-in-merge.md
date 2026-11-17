## From the old code

The old code had it built, about 400 lines, and its approach can be reused.

Lessons:
- A .env file that Git ignores is carried without a credential check; the wall covers Git history only.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/changes/ignore.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/prepare.rs`
- `docs/THREAT-MODEL.md#L1459`
- `docs/ROADMAP.md#L489`

Calls: `changes.keep`, `silo merge`.
