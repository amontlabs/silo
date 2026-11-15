# Worker rules

You build one item, in the worktree and branch your task names.

## Checkout
- Work only in your worktree. Commit as you go. Never push, never open a pull request, never touch another checkout.
- Commit only your own files, by path, in one step: `git commit -m "…" -- <paths>`. Never `git add -A` or `git commit -a`: another agent may be working in the same worktree.
- Amend, reorder and squash your own commits freely (those above the base `atelier follow` prints). Never rebase below them by hand. Never `git merge` into your branch.
- To get newer work: commit, then `atelier follow`. On a conflict it stops with the rebase open: resolve, `git add`, `git rebase --continue`, then `atelier follow` again.
- Use plain `cargo`. Never `+toolchain`, never `CARGO_TARGET_DIR`.

## Decisions
- Where your task does not decide a behaviour, pick the safest option that keeps every existing rule (no wider access, no skipped approval, no lost data, older saved data still loads). List it under "Decisions I made".

## Proof
- Run only the tests that prove your change: one test, one crate, the frontend files you touched.
- A rule that must hold on every path gets one model-based test: random operation sequences against a reference model, checked after every step. These caught 4 of 7 high-severity past bugs that ordinary tests missed. A new path the rule covers goes into that test.
- After a fix, rerun the failing tests by name: `cargo nextest run -E 'test(=<name>)'`. Prove a flaky-test fix with `cargo nextest run --stress-count 50` on the crates you touched (`vitest --repeats 50` for frontend tests).
- Before your report, run `atelier check --compile-only` on the committed tree. Copy its `compile:` line into your report.
- Another platform: when your change concerns one the project lists, run your tests there with `atelier test <platform> -- <command>`, from the folder you would use locally.
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
- First run `atelier check --compile-only` once. Its setup steps put the private build file, `node_modules` and the prepared runtime in your worktree. Never print or commit `app/SiloUI/github-build.local.json`.
- Files where parallel work collides most (measured): `app/SiloUI/src-tauri/engine/src/policy/mod.rs`, `docs/SiloUI-POLICY.md`, `docs/THREAT-MODEL.md`, `app/SiloUI/src-tauri/engine/src/secrets.rs`, `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`, `app/SiloUI/src-tauri/silod/src/methods/gate.rs` and `app/SiloUI/api/src/methods/registry.rs`. Keep changes there small and additive, and tell the orchestrator when your item must change them.
- After an API change, run both generators from `app/SiloUI` (`cargo run -p silo-api --example generate_requests`, then `--example generate_schema`) and commit their output. The merge regenerates both files, so a conflict only in them is settled there.
- Wire names, saved file formats and checkpoint kinds keep their spelling unless your item says otherwise. Old saved data must still load.
- Never start, stop or connect to a Silo or Silo Dev engine or app, unless your task gives you the Dev engine for a live check: `atelier tmp --short -- app/SiloUI/scripts/live/run <check>`. Only the Dev build and throwaway `e2e-*` computers.
- A user-visible change needs a changeset in `app/SiloUI/.changeset/` (`minor` for a feature, `patch` for a fix).
- Comments describe the code, not the change. User-facing words follow docs/SiloUI-VOCABULARY.md.
- Disk is tight: send long output to a log file and read the summary.
- Live checks: `run <check>` blocks and exits 0 pass, 1 fail, 2 usage, 3 skipped (also when another check holds the Dev engine lock; the message names the holder, wait and rerun). For a long one, `run start <check>` prints `<state-dir> <run-id>`, then `run wait <state-dir> [<run-id>]`. A check's `cleanup()` stops the computers it started. Linux offline means "skipped: offline". The harness's tests are `live-test.sh`.
