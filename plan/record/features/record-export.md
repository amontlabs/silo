# Export of the record

## For the owner

An agent's part, 22 November 2026. Your company wants the hosts main reached to appear in the log
tool it already uses.

The options:

- Leave it out (recommended). The `silo` command already prints the record as JSON, one line per
  fact or report, and a small script or an extension turns that into what the log tool reads. It
  costs writing that script.
- Build it. Silo writes the record in named formats, such as OpenTelemetry. It costs formats to
  follow as they change, for nobody who has asked yet.

Recommended: leave it out. One plain output that every tool reads does the job, and it is already
recommended in `record-queried-with-ordinary-tools`.

## Notes

In the plan's notes, as an idea for later: "Export in a standard format is an idea for later."
