## Proposed, not confirmed by the owner

### For the owner to settle

- Does what an agent taught itself in a fork (a skill, a tool, its rules files, its memory) merge with the rest, or only on its own approval?
  - Recommendation: Its own approval, since these are unreviewed instructions that steer every later task.
- Can one group or item be left out of a merge while the rest lands, or does the first merge take the whole fork?
  - Recommendation: The whole fork, since `git-words` makes this merge a fast-forward and `merge-conflicts` brings item by item later.

### For whoever builds it

- Property: After a merge, main's history shows which fork brought each change, with each merge kept as one unit.
- Test: A fork undoes a fix that an earlier merge brought into main; the merge view flags it before the merge.

## Notes

- Old live test: merges onto a running main with Postgres and a dev server only passed once migrations, a crontab merge and service restarts were added (see merge-hooks).
- Order: checkpoint main, replay packages, git fetch and merge, copy files by writing a temporary file and renaming.
- Check three ways: the base at fork time, the fork, and main.
- Live database folders are listed, never copied. When live data changed without a migration, offer replace-main instead.
- Fallback if file copy is unreliable: merge git and packages only, plus replace main.

## From the old code

The old code had it built and driven end to end, about 6,000 lines, and it needs reworking.

Lessons:
- A database is never combined; only one side is taken.
- Keeping a fork without asking when its tests pass was dropped: the tests run in the fork, so the fork can fake them.
- An agent merging into its own computer asks the owner and must carry the digest of a diff it ran.
- A Linux merge reads and writes as root; run repo commands as the folder's owner.
- Hash both computers live at merge time; a file the collector could not hash is never taken as unchanged.
- A journal synced before main changes lets the next merge, undo or engine start restore an interrupted merge.
- Databases are never combined: a database change needs the recipe's migrate step or an explicit data-ok.
- A request whose fork changed after it was made must be refused; the agent asks again.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/keep.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/keep.rs`
- `docs/direction/evidence/V.txt`
- `docs/direction/evidence/M.txt`
- `docs/SiloUI-CHANGES.md`
- `docs/SiloUI-CHANGES.md#merging-a-fork-silo-merge-and-silo-undo`
- `app/SiloUI/cli/src/flows.rs`
- `docs/product/JOURNEYS.md`
- `docs/product/JOURNEYS.md#L45`
- `app/SiloUI/src-tauri/engine/src/changes/keep/mod.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/prepare.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/apply.rs`

Calls: `changes.keep`, `silo merge`, `changes.requests.*`.
