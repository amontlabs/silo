# Can the record be queried with ordinary tools

## For the owner

An agent's part, 22 November 2026. You want every host main reached last week, sorted by bytes sent,
and the app has no such view.

The options:

- Through the `silo` command (recommended). It prints one line of JSON per fact or report, and you
  pipe it into grep, jq or a database. It costs a command between you and the files, slower on a
  very large record than reading them directly.
- Files in an open format. The record is kept as, say, a SQLite database that any tool opens. It
  costs a format Silo can no longer change, and a tool that reads while Silo writes can get it
  wrong.
- The app only. It costs every question the app did not foresee.

Recommended: through the `silo` command. It answers the question you have today without freezing how
the record is stored.
