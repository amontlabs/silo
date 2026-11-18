## Proposed

- Rule: The cases where both sides changed are designed first, because with parallel forks a merge onto an unchanged main is the exception.
- Databases are the hardest case at merge, and the working answer is to run the fork's migration again on main.
- At merge, Silo shows what changed in main since the fork was made.
- A merge onto a changed main can have main's changes reviewed against the fork, to catch conflicts that are not textual.
- A merge records each conflict it met and how it was resolved, and shows them for review.
- Rule: If a chunk of a file can be accepted on its own, an existing implementation of chunk staging is reused before one is written.

## Open

- Should this feature move into the starter, next to merge, since with parallel forks only the first merge finds main unchanged?
  - Suggested: Yes, because the starter is used daily with several forks and every merge after the first needs it.
- How fine is the choice at merge: whole items such as a file or a package, or also a chunk of a file?
  - Suggested: Whole items, because a conflicting repository already goes to a branch for Git to settle, and chunk staging is thousands of lines of edge cases.
- May Silo settle a conflict itself from the brief each fork started with, when git-words says conflicts are asked, or only show the briefs beside your choice?
  - Suggested: Show the briefs and let the agent propose a settlement, and you choose, as git-words says.
- To find out: Before any merge logic for code is written, does Mergiraf, a syntax-aware merge driver, already do it well enough?
- To find out: When a fork depends on a sibling's unfinished work, which loses less: stopping to replan, or continuing and settling it at merge in the order the forks land?

## From the old code

The old code had it built, about 1,400 lines, and its approach can be reused.

Lessons:
- Silo never force-resets a repository; a repo conflict can only go to a branch or stay as main.
- Silo never force-resets a repository: a conflicting one can only go to a branch or stay as main.
- Fast-forward must not swap computers, because the agent lives in main.
- Silo's own per-computer files and per-login files conflicted on every merge until left out.
- Undo was flaky on Postgres sequences in v1.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CHANGES.md#conflicts-are-questions`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-CHANGES.md#L245`
- `app/SiloUI/src-tauri/engine/src/changes/items.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/plan.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/cron.rs`
- `docs/DIRECTION.md#L112`
- `docs/product/JOURNEYS.md#L45`
- `docs/v1/evidence/phase-2/linux-check.txt`

Calls: `silo merge --choose`, `silo merge --skip`, `changes.keep`, `silo merge`, `silo replace-main`, `silo undo`.
