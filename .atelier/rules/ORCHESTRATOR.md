# Orchestrator rules

1. Start with `atelier status` and `atelier agents`. Fix a failed preflight line first, as its own item.
2. Plan items, one topic each. For each: `atelier start <branch> --path <worktree>` (add `--on <lower branch>` when it builds on one), then `atelier task build <branch> goal=… files=… done=… risks=…` (or `build-hard`), and start that role's agent with the printed line as the prompt. Start the next item as soon as what it builds on has committed; whenever a worker finishes, look for work to start before you wait.
3. On each report: read its tests and its `compile:` line. Review by risk; skip it for a mechanical change. Round 1: `atelier task review <branch> intent=… risks=…`. Round 2: `atelier task review <branch> --fixes findings=@<file>`. A stacked branch is reviewed for its own commits only; its lower branch is approved on its own and merged in the same batch or before. Fixes go back to the worker that wrote the code. Then `atelier approve <branch> <the commit you reviewed>`.
4. Merge everything approved in one batch, as a background command: `atelier merge <branch>…`. It runs the full check on the merged tree, on every platform. A branch that changes `.atelier/` merges in its own batch. When a gate ends, the next batch takes everything approved since.
   - Exit 1, or 5 "with main": send the output to the worker who wrote the code; for a conflict it runs `atelier follow` and resolves.
   - Exit 5 "with <earlier branch>": merge again without the later branch; once the earlier one has landed, the later branch's worker runs `atelier follow` and resolves.
   - Exit 7: don't touch the main checkout; run `atelier promote` once it is clean.
   - "main moved": run the same merge again.
   - "<branch> is behind a rewrite of <lower>": ask that branch's worker to run `atelier follow`. `atelier status` lists every such branch.
   - "gate worktree has unknown files": look at each, keep what someone saved, remove the rest, merge again.
   - "a promotion was interrupted": stop merging, change nothing in the main checkout, give the owner atelier's list.
   - "rules changed": re-read these rules; tell running workers to `atelier follow` and re-read.
   - Exit 3, no machine for a platform: waive it (`--waive <platform>`) only on the owner's word, quoted in `--reason`.
4b. Worktrees are the default, not a requirement; you decide.
   - Several small items with disjoint files may share one worktree and branch, reviewed, approved and merged as one. Tell each agent its files; one commit per item, so a failing one can be dropped.
   - A change only to files that no build, test or check reads (top-level `docs/` and Markdown outside the app; not bundled help) may be committed straight to `main` by one agent, after checking its relative links, and only while no merge is running: a commit on `main` voids a running merge.
   - Anything else goes through `atelier merge`, however small.
5. A test that `atelier status` lists as flaky (failed, then passed on the same tree and platform in full runs) is a fix item with the failing output, before anything else.
6. After each merge: `atelier clean`, read the plan, then `atelier clean --yes`.
7. Done when everything is merged, no promotion is pending, and every platform passed on `main` or the owner waived it.

Choosing agents: by role, from `atelier agents`. Effort per task: low for lookups and mechanical edits; medium for an ordinary item; high for cross-crate, concurrency, migration or security-relevant work and for reviews of risky diffs. Never name models, retries or CI in a task. Reuse a worker for its fixes while its session is small; otherwise start a fresh one in the same worktree with a short handover. A report that is only a pointer ("the report above stands") is lost: ask for it in full.

## Silo
- Owner log: one or two lines per judgement call, guess or skipped review, in the build's OWNER_REVIEW.md. Never block on it.
- Live checks need the Dev engine: give it to one worker at a time, and say so in its task's notes.
- GUI tasks: only the Dev build and throwaway `e2e-*` computers; say so in the task's `steps`.
