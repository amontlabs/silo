## Proposed, not confirmed by the owner

### For the owner to settle

- Across versions, is the engine's state upgraded in place, or does the new engine start beside it and import it?
  - Recommendation: Import, leaving the old state untouched, so a failed update goes back by starting the old engine.

### For whoever builds it

- Rule: Update notices appear in the app, and Silo never tells the user anything by putting text into an agent's context.
- Property: An update or a changed default never lets a computer reach more than the user's rules allowed before it.
- Rule: An update itself changes no computer; a computer takes Silo's new parts at its next start, through keep-current.
- Property: After an update, an existing install has every integration and plugin that a fresh install of that version gets.
- Test: A test opens computers and engine state saved by an older version of Silo, and each one works.
- Docs: Every release note says what happens to stored state: nothing, migrated by itself, or what the user must do.
- Detail: An agent can ask the `silo` command whether the installed version is out of date.

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
