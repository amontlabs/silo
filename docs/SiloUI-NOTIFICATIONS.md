# Notifications

Everything Silo tells the user outside a direct UI response goes through one router,
`app/SiloUI/src-tauri/src/notifications.rs`. A `Notice` has a category, a stable
`key`, a title, a body, and optionally the computer (`id`, `name`) it is about.

## Ownership

| Owner | Results | Delivery |
| --- | --- | --- |
| Frontend | Results of commands it awaits: push, GitHub apply, ports, checkpoints, storage reclaim, log export | Shows its own toast, mirrors to the system with `deliver_notice` |
| Backend | Computer lifecycle (local and remote), export/import, computer setup/configuration, Git identity | `notify_native`: the frontend already shows state from the backend |
| Backend, no other UI owner | Unexpected computer changes, startup and update-resume failures | `notify`: in-app toast (`silo://notice`) plus system notification |

Computer setup and Git identity failures are also shown in the setup UI; the backend
adds a system notice for when Silo is in the background. A stale-edit rejection is shown
inline by the editor and never notifies.

## Policy

- Focus: a system notification is never shown while the main window is visible and
  focused. The macOS delegate presents `List` only (no banner), so a notice that races a
  focus change lands quietly in Notification Center.
- Categories: `failures`, `changes`, `completions`, `approvals`, gated by the master switch and
  `notifyFailures` / `notifyChanges` / `notifyCompletions` / `notifyApprovals` (legacy keys are a fallback).
- Approvals: a computer's agent asking for an answer. The engine publishes the notice with the key
  `approval:<id>`; the window shows a toast with a Review button, and a click on the system
  notification opens the Pending page at that request (route `{tab: "changes", approval: <id>}`).
- Completions: successes notify only when the operation took at least 3 seconds
  (`LONG_OPERATION`), measured from the command start. Failures always notify.
- Never notified: user cancellations, requests deduplicated into an already queued
  action (`GateError::AlreadyQueued`, D-13), dismiss-error, synchronous rejections of an
  export/import that the UI already shows (E-48).
- Delivery ordering: OS calls for the same key share a gate. Queued requests carry
  revisions assigned before background dispatch; superseded requests are skipped.
  Different keys have independent gates.
- Replacement: the `key` is the notification identity (`computer:{id}:lifecycle`,
  `computer:{id}:transfer`, ...). A newer notice with the same key replaces the older one: on
  macOS as the request identifier, on Linux through `replaces_id`.
- Titles name the computer ("Couldn’t start dev", "dev is running"); bodies are one line,
  at most 200 characters.
- Click routing (macOS): the notification carries the route as user info. The delegate
  opens the main window at `{"tab":"computers","computer":<name>}`, or
  `{"tab":"computers"}` without a computer, through `status_panel::open_main`.
  Notifications group by thread: computer id, else the category name.
- Clearing: queued and delivered identifiers are tracked per computer id. Deleting a
  computer (local `change_computer_configuration`, remote `remote_delete_computer`)
  invalidates queued deliveries and withdraws in-flight and delivered notifications
  (`removeDeliveredNotificationsWithIdentifiers`, or `CloseNotification`). A delayed
  withdrawal preserves a newer successful submission for the same key.

## Platform gaps

Linux replaces and closes notifications by key but does not route clicks: that needs a
GLib main loop for the `ActionInvoked` signal and varies by desktop. Notification
Center on macOS may keep pending clicks across relaunch; the route comes from user info,
not process memory.

## Tool choice

`tauri-plugin-notification` (2.3.0) delivers on desktop through `notify-rust`. Per its
DeepWiki index (not exercised by us), its ids, `group` and click actions are aimed at
mobile, and the desktop path exposes no click callback. Silo needs replace-by-identifier,
thread grouping, click routing to a computer, and clearing on macOS, all of which
`UNUserNotificationCenter` provides directly, and it already owns the authorization flow
and delegate there. Silo therefore keeps direct UserNotifications (`objc2-user-notifications`)
and the freedesktop D-Bus interface, and keeps product policy in the router. Revisit if
the plugin gains desktop click handlers.

### Linux notification server identity

Replacement and withdrawal share a lock with ID publication. Each cached ID
belongs to the unique D-Bus owner that issued it; a different owner starts a new
notice instead of reusing that ID. Calls target the captured unique owner, so a
restart between lookup and delivery cannot redirect the call to a new service.
This uses the existing GIO connection and the standard
[notification protocol](https://specifications.freedesktop.org/notification/latest/protocol.html),
whose replacement and close operations address notices by server-assigned ID.
