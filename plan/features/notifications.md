## Proposed, not confirmed by the owner

### For whoever builds it

- Rule: Silo notifies only on a transition that needs a person (an approval or a question pending, a task finished, failed or stalled) and never sends a stream of every event.
- Rule: The engine raises 'needs you' itself when a computer waits on the user, so the signal never depends on the agent marking it.
- Detail: Everything that waits on the user sits in one queue across all computers and forks, ordered by importance.
- Detail: Each item of the queue is answered in place, in one step, one after another.
- Detail: A computer can raise a softer 'I may need you' signal, which shows in its status in the app and sends no notification.
- Detail: Items that ask for attention are deduplicated by fingerprint and counted, with the first and the last time seen.
- Detail: An item can be acknowledged, resolved or silenced, and the record keeps who did each.
- Test: When an agent says it waits on an answer, the pending item exists in the app, or the claim is shown as unfounded.
- Detail: The menu bar item shows what finished, stalled or waits on you since you last looked, with what each changed.

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
