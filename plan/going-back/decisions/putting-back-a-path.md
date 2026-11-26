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

## Where the drafts of 25 November disagree (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

#378, section 3: #370's put-back writes with `sudo tar -x` and `mv` through paths in a folder an agent may control. An agent that swaps a folder for a link to `/etc/sudoers.d` between the listing and the copy makes root write there. Recommended: a small program in main does the put-back through open folders (`openat` with no-follow), unlocks files the agent locked, and moves a FIFO or socket aside without opening it.

## Put back with a preview, prototyped

An agent's part, 25 November 2026, from [put back with a preview](../../../research/2026-11-25-put-back-and-chain/README.md). #370 step 5 was built for any change, not only
deletions, on #399's night, and tested.

- #370 says a write made between the preview and the swap is lost from main but kept in the
  checkpoint saved before the put-back. It is not kept. The write comes after that save, so no
  checkpoint holds it. A test shows it.
- Two changes close the gap. The confirm is bound to what the preview showed: if a shown path
  changes before the swap, Silo stops, keeps its save, and shows you again. A checkpoint taken
  just after the swap, while the staging folder still holds what was replaced, covers the last
  instant between that check and the swap.
- The swap should be one exchange of names (`RENAME_SWAP` on macOS, as #370's restore already
  does). Two renames left the path missing for a program reading it, in 612 to 1,460 reads per
  run. The exchange never did.
- The preview lists what a put-back undoes, grouped by who made it, by `author-from-traffic`'s
  rules, or "not known; while … ran". For a folder two agents worked in, this is where you see you
  would undo the other agent's work. It is the same list `putting-back-a-runs-paths` shows.

Proposed for the answer, after "so nothing is lost": "If a path changes after you saw what it
replaces, Silo stops and shows you again." Without the check, "nothing is lost" is not true.
