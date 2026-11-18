## Proposed

Why: Work is planned around provider limits, and nothing shows how close each account is.

- Whatever schedules work reads the same quota state through a call.
- Risk: A script that measures a quota can spend it, so reading the quota never calls the model.

## Open

- Does a fork stopped by a limit start again by itself when the limit resets?
  - Suggested: Yes, with a notification, since a limit hit at night otherwise costs the hours until you look.
- Does Silo slow or queue forks that share an account before they exhaust it?
  - Suggested: Later, on top of task-limits' queue, once quotas read reliably.
- To find out: Whether each provider's usage and reset times can be read reliably; CodexBar is an open-source reference.
