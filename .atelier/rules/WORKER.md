# Worker rules

You build one item, in the worktree and branch your task names.

## Checkout
- Work only in your worktree. Commit as you go. Never push, never open a pull request, never touch another checkout.
- Commit only your own files, by path, in one step: `git commit -m "…" -- <paths>`. Never `git add -A` or `git commit -a`: another agent may be working in the same worktree.
- Amend, reorder and squash your own commits freely (those above the base `atelier follow` prints). Never rebase below them by hand. Never `git merge` into your branch.
- To get newer work: commit, then `atelier follow`. On a conflict it stops with the rebase open: resolve, `git add`, `git rebase --continue`, then `atelier follow` again.

## Decisions
- Where your task does not decide a behaviour, pick the safest option that keeps every existing rule (no wider access, no skipped approval, no lost data, older saved data still loads). List it under "Decisions I made".

## Proof
- Run only the tests that prove your change: one test, one crate, the frontend files you touched.
- A rule that must hold on every path gets one model-based test: random operation sequences against a reference model, checked after every step. These caught 4 of 7 high-severity past bugs that ordinary tests missed. A new path the rule covers goes into that test.
- Before your report, run `atelier check --compile-only` on the committed tree. Copy its `compile:` line into your report.
- Temporary data: run the command as `atelier tmp -- <command>` (`--short` for a temp HOME with sockets) and use only `$ATELIER_TMP`.

## Waiting
- Run long commands in the foreground. Stop every background command before your report.

## Report (your final message, at most 400 words)
The stop hook sends you back while a rebase is open, something is uncommitted, a background task runs, or no line starts with `compile:`. Fix what it names, then report.
1. Branch and last commit.
2. What you built, in plain words.
3. Tests run and results; the `compile:` line.
4. What is not done or not verified.
5. Decisions I made, or "none".
6. Broken things outside your scope, for the orchestrator.

## Silo
- The plan's lists, every taken decision that is not replaced and the entries your task names bind you. `plan show <id>` prints them for one entry. Build by them. One you think should change goes in your report under 6, with your reason.
- Never change `plan/` unless your task says so.
- Use Node.js 26. Disk is tight: send long output to a log file and read the summary.
