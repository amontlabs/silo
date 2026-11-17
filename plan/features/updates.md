## From the old code

The old code had it partly built, about 5,200 lines, and it needs reworking.

Lessons:
- Release archives must contain only the Silo.app root; BSD tar's AppleDouble entries broke the first archive.
- Losing the updater private key strands every installed app.
- Hold the installer lock until the new package is in place; the engine's watchdog aborts the update if the app dies.
- A rebuild in progress makes Quit and updates wait.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-DISTRIBUTION-PLAN.md#L73`
- `docs/SiloUI-RELEASES.md#L405`
- `app/SiloUI/src-tauri/vendor/tauri-plugin-updater`
- `app/SiloUI/src/features/updates/updates.tsx`
- `app/SiloUI/src-tauri/src/commands/updates.rs`
- `docs/SiloUI-NATIVE-MENUS.md#L37`
- `app/SiloUI/api/src/methods/updates.rs`
- `app/SiloUI/api/src/methods/engine.rs`
- `app/SiloUI/src-tauri/engine/src/updates.rs`
- `app/SiloUI/src-tauri/engine/src/engine_update`
- `docs/SiloUI-DISTRIBUTION-PLAN.md`
- `docs/research/release-dry-run-2026-09-30.md`

Calls: `updates.*`, `engine.update.prepare`, `engine.shutdown`, `updates.check`, `updates.download`, `updates.focused`, `updates.setAutomaticChecks`, `updates.state`, `engine.update.abort`.
