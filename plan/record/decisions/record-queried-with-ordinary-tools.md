# Can the record be queried with ordinary tools

## The choice

An agent's part, 22 November 2026. You want every host main reached last week, sorted by bytes sent,
and the app has no such view.

The options:

- Through the `silo` command (recommended). It prints one line of JSON per fact or report, and you
  pipe it into grep, jq or a database. It costs a command between you and the files, slower on a
  very large record than reading them directly.
- Files in an open format. The record is kept as, say, a SQLite database that any tool opens. It
  costs a format Silo can no longer change, and a tool that reads while Silo writes can get it
  wrong.
- Export in named formats. Silo also writes the record in the formats other tools read, such as
  OpenTelemetry for the log tool your company already uses. It costs formats to follow as they
  change, for nobody who has asked yet.
- The app only. It costs every question the app did not foresee.

Recommended: through the `silo` command. It answers the question you have today without freezing how
the record is stored. One plain output that every tool reads also does the job of an export: a small
script or an extension turns it into what another tool reads.

## Notes

In the plan's notes, as an idea for later: "Export in a standard format is an idea for later."
