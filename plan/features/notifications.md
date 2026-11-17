## From the old code

The old code had it partly built, about 4,600 lines, and it needs reworking.

Lessons:
- Visible delivery in Notification Center was never verified; policy and authorization were tested natively.
- The Tauri notification plugin has no desktop click callback or replace-by-id, so UNUserNotificationCenter is used directly.
- The panel window and its page must both be transparent or a second rectangular outline shows.
- Native corner and tray clicks were never driven by automation.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/host/src/notifications.rs`
- `app/SiloUI/src/features/application/pages/notifications-page.tsx`
- `docs/SiloUI-NOTIFICATIONS.md`
- `docs/SiloUI-DEPENDENCIES-BACKUP-TESTING.md#L340`
- `app/SiloUI/src/features/status-bar/status-bar.tsx`
- `app/SiloUI/src/features/status-bar/status-folder-picker.tsx`
- `app/SiloUI/src-tauri/src/tray.rs`
- `app/SiloUI/src-tauri/src/status_panel.rs`
- `docs/SiloUI-STATUS-PANEL.md`
