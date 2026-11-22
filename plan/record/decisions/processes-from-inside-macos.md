# Processes from inside, on macOS

## For the owner

An agent's part, 22 November 2026. In a macOS computer, an agent runs a test suite that starts 300
short processes in ten seconds, and one of them writes `config.json`. What the record can say
depends on what macOS lets Silo see with nobody clicking.

The options:

- Full detail first, then fall back (recommended). First comes Apple's own service for following
  processes and files, with its permission already set in the image Silo makes, so that no computer
  asks you anything. Then Apple's command-line tool that gives the same detail. Last, reading the
  list of processes at intervals. If only the last one works, the record misses most of the 300
  processes, says its process lines are sampled, and never says which process wrote `config.json`.
  It costs a trial on a macOS computer, and maybe a permission asked of Apple, which Apple can
  refuse. If a click once per image turns out to be needed, or turning off System Integrity
  Protection inside the computer, each comes back to you as a question of its own.
- Sampling only, from the start. No permission, nothing asked of Apple and nothing to click. It
  costs the record on macOS: most of the 300 processes are missed, and `config.json` is never tied
  to the process that wrote it.

Recommended: full detail first. Which agent wrote which file is the first thing you asked of the
record, and sampling cannot say it.

## Notes

To find out.

macOS has Endpoint Security, which gives detailed process and file events. It needs a permission
from Apple, or the system's own command-line tool with full disk access. Whether either can be set
up with no clicks is unknown.
