## Proposed

- Rule: A setting is a device-wide default that a computer can override.
- Rule: An unfinished feature is switched off and does not appear in Settings.
- Settings has an entry for the default shell.

## From the old code

The old code had it built and driven end to end, about 4,700 lines, and it needs reworking.

Lessons:
- The login switch shows what macOS reports, never the saved flag; Requires approval stays off with an Open System Settings action.
- Start at launch once started nothing because the saved switch had no computer list; the default must be resolved at startup.
- Quit must ask the window to flush queued edits before exiting, or the last edit is lost.
- A corrupt or future-version settings file is protected from all writes for the session instead of being overwritten.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/settings.rs`
- `app/SiloUI/src-tauri/engine/src/settings.rs`
- `docs/direction/evidence/V.txt`
- `app/SiloUI/src/features/application/pages/general-page.tsx`
- `app/SiloUI/host/src/system_integrations.rs`
- `docs/SiloUI-SETTINGS.md#L9`
- `docs/SiloUI-DEPENDENCIES-BACKUP-TESTING.md#L340`
- `app/SiloUI/src/features/preferences/settings-store.ts`
- `docs/SiloUI-SETTINGS.md#L314`

Calls: `settings.read`, `settings.update`, `settings.flush`.
