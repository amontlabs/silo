# Observing and waiting

## For the owner

An agent's part, 22 November 2026. A build in main writes 50,000 files faster than Silo can write
them down.

The options:

- The build never waits (recommended). Silo merges what it writes, and if that is not enough it
  loses some and records a gap. It costs a record with gaps on your heaviest work.
- Nothing is ever lost. The build is slowed until every change is written down. It costs speed
  exactly where builds run, and an agent that makes noise slows its own computer and the others on
  your Mac.

Recommended: the build never waits. People stop using a tool that slows their builds, and a gap that
says so is honest.

## Notes

A synchronous precaution, and the limit of its wait, are in the parts `rules` and `boundary`.
The record names no check for this sentence: `live check` is put forward here.
