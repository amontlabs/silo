# Matching reports to facts

## The choice

An agent's part, 22 November 2026. Between the checkpoints of 14:00 and 14:05, Claude Code and Codex
in main both report writing /work/site/package-lock.json, and the file changed once in that time.
Most of this entry is how Silo checks what programs report against what it saw itself; this case is
the part you meet.

The options:

- Unattributed, naming both runs (recommended). The change shows as unattributed, with the two runs
  that reported writing it named. It costs precision in a shared checkout, where this will be
  common: putting back one run's paths cannot take that file, and you look at it yourself.
- Put down to the last run that reported writing it. It costs the truth when the change holds both
  runs' work: the file shows as Codex's alone, and putting back Codex's paths would undo Claude
  Code's part too.

Recommended: unattributed, naming both runs. The record should never say one run did what two did,
and it still tells you exactly which two.

## Notes

Left for when it is built.
