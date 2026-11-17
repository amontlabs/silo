## From the old code

The old code had it built, about 6,300 lines, and it would be written again.

Lessons:
- The migration screen was hidden behind an error screen during a real upgrade (M finding 4).
- The move is one-way and journals hold absolute paths, so repair must run in the original location with every VM stopped.
- The move is one way: the older runtime refuses the new home.
- Docs and branches say 0.7.15 while the merged runtime is 0.7.14.
- MicroSandbox VMDK descriptors name image files by absolute path, so a copied cache still reads the old runtime's files.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/migration.rs`
- `app/SiloUI/src-tauri/engine/src/runtime_migration.rs`
- `app/SiloUI/src-tauri/engine/src/pre_upgrade_backup.rs`
- `docs/direction/evidence/M.txt`
- `app/SiloUI/src/desktop/runtime-migration-boundary.tsx`
- `app/SiloUI/src/desktop/migration-backup-notice.tsx`
- `app/SiloUI/src/features/storage/pre-upgrade-backup.tsx`
- `app/SiloUI/src-tauri/src/commands/pre_upgrade_backup.rs`
- `app/SiloUI/src-tauri/engine/src/runtime_migration/protected_storage.rs`
- `docs/direction/MSB-0.7.15-MIGRATION.md`
- `app/SiloUI/src-tauri/engine/src/runtime_migration/vocabulary.rs`
- `docs/SiloUI-VOCABULARY.md`

Calls: `migration.state`, `migration.preUpgradeBackup.get`, `migration.preUpgradeBackup.measure`, `migration.preUpgradeBackup.delete`, `migration.preUpgradeBackup.acknowledgeNotice`, `migration.preUpgradeBackup.*`, `msb self repair`.
