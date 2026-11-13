# Worker rules

You build one item, in one worktree, on one branch, named in your task.

## Checkout
- Work only in your worktree. Commit as you go. Never push, never open a pull request, never touch another checkout.
- Commit only your own files, by path, in one step: `git commit -m "…" -- <paths>`. Never `git add -A` or `git commit -a`: another agent may be working in the same worktree.
- Amend, reorder and squash your own commits as you like (they are the ones above the base `atelier follow` prints). Never rebase below them by hand, and never `git merge` into your branch: newer work comes only through `atelier follow`.
- Getting newer work: commit, then `atelier follow`. It brings your branch up to date with what it is built on (another item, or `main` once that item is merged). On a conflict it stops with the rebase open: resolve, `git add`, `git rebase --continue`, then `atelier follow` once more.
- Use plain `cargo`. Never `+toolchain`, never `CARGO_TARGET_DIR`.

## Proof
- Choose the tests that prove your change: one test, one crate, the frontend files you touched. Run them yourself.
- After a fix, rerun the failing tests by name (`cargo nextest run -E 'test(=<name>)'`). Fixing a flaky test: prove it with `cargo nextest run --stress-count 50` on the crates you touched (`vitest --repeats 50` for frontend tests).
- Before your report: `atelier check --compile-only` on the committed tree (format, every target compiles, typecheck). Copy its `compile:` line into your report.
- Another platform: when your change is about one the project lists, run your tests there with `atelier test <platform> -- <command>`, from the same folder you would run them in locally.
- Temporary data: run the command that needs it as `atelier tmp -- <command>` (`--short` for a temp HOME with sockets). It gets the folder in `$ATELIER_TMP`. Nowhere else.

## Waiting
- Run long commands in the foreground. Stop every background command before your final report.

## Report (your final message, at most 400 words)
When you end your turn, atelier's stop hook sends you back while a rebase is open, something is uncommitted, a background task is running, or your last message has no line starting `compile:`. Fix what it names, then report.
1. Branch and last commit.
2. What you built, in plain words.
3. Tests you ran and their results; the `compile:` line.
4. What is not done or not verified.
5. Broken things outside your scope; what the orchestrator should decide.

## Silo
- First run `atelier check --compile-only` once. Its setup steps put the private build file, `node_modules` and the prepared runtime in your worktree. Never print or commit `app/SiloUI/github-build.local.json`.
- Files where parallel work collides most (measured): `app/SiloUI/src-tauri/engine/src/policy/mod.rs`, `docs/SiloUI-POLICY.md`, `docs/THREAT-MODEL.md`, `app/SiloUI/src-tauri/engine/src/secrets.rs`, `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`, `app/SiloUI/src-tauri/silod/src/methods/gate.rs` and `app/SiloUI/api/src/methods/registry.rs`. Keep changes there small and additive, and tell the orchestrator when your item must change them.
- After an API change, run both generators from `app/SiloUI` (`cargo run -p silo-api --example generate_requests`, then `--example generate_schema`) and commit their output. The merge regenerates both files, so a conflict only in them is settled there.
- Wire names, saved file formats and checkpoint kinds keep their spelling unless your item says otherwise. Old saved data must still load.
- Never start, stop or connect to a Silo or Silo Dev engine or app, unless your task gives you the Dev engine for a live check: `atelier tmp --short -- app/SiloUI/scripts/live/run <check>`. Only the Dev build and throwaway `e2e-*` computers.
- A user-visible change needs a changeset in `app/SiloUI/.changeset/` (`minor` for a feature, `patch` for a fix).
- Comments describe the code, not the change. User-facing words follow docs/SiloUI-VOCABULARY.md.
- Disk is tight: send long output to a log file and read the summary.
