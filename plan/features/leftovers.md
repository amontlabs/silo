## From the old code

The old code had it and then dropped it, about 600 lines, and its approach can be reused.

Lessons:
- Backup and restore were driven end to end before removal; restore was renamed import so Restore means checkpoints only.
- Never capture or fake a desktop wallpaper; use the native material.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-DEPENDENCIES-BACKUP-TESTING.md#L199`
- `docs/SiloUI-NATIVE-MENUS.md#L37`
- `app/SiloUI/src/features/application/components/alpha-notice.tsx`
- `app/SiloUI/src-tauri/src/window_material.rs`
- `app/SiloUI/src-tauri/src/titlebar.rs`
- `docs/SiloUI-GLASS-STUDY.md`
- `docs/SiloUI-BUILD-CHANNELS.md#L98`
- `app/SiloUI/scripts/import-production-settings.mjs`
- `docs/forks-ci/PLAN.md`
- `docs/ROADMAP.md#L510`

Calls: `npm run dev:import-production-settings`.
