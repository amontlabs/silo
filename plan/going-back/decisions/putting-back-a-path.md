# Bringing back one path from a checkpoint

## The choice

An agent's part, 24 November 2026. At 03:00 Codex deletes `~/work/app/src`, which it never
committed, while Claude Code works through the night in `~/work/site`. Forks do not exist yet.

The options:

- Read and put back a path you name (recommended). `silo look 02:40 ~/work/app/src` shows the
  folder as it was; `silo put-back 02:40 ~/work/app/src` copies it into main after a checkpoint of
  now. Claude Code's night is untouched. It costs a destructive action (`kinds-of-other-actions`
  lists "put back paths"), made undoable by the checkpoint before it.
- Restore the whole computer. It costs every other agent's work since 02:40.
- Fork from 02:40 and copy the folder out. It costs the wait for forks, and the folder still does
  not leave through Git, since it was never committed.

Recommended: read and put back a path you name. It is the smallest way back, and it only reads
checkpoints Silo already keeps.

## Notes

Agent, 24 November 2026. From draft #354 (gap 6).

`fork-from-the-moment` (taken) says that on a shared main you do not rewind, you fork from the
moment. `one-run-grain` and `putting-back-a-runs-paths` (both taken) already allow putting back
paths for one run, where the run is Silo's opinion. This entry is the plainer case, with paths you
name and no opinion, so it rests on those two. If polarzero reads `fork-from-the-moment` as
forbidding it, the way is a decision that replaces `fork-from-the-moment` with both sentences.

A path is read from the checkpoint by the same reader that compares checkpoints
(`file-changes-from-outside`), never by mounting the disk on the Mac.
