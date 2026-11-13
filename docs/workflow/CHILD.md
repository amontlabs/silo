# Child rules

You are one agent building one item. Your task message names it and gives a base branch, a worktree and the checks that matter. The repository rules in [AGENTS.md](../../AGENTS.md) apply too.

## Checkout

- Work only in the worktree you were given. Start from the base named in your task; if the branch holds no work of yours and the base moved, `git reset --hard <base>`, otherwise `git merge --ff-only <base>`.
- Commit as you go (stage and commit in one step). Never push, never open a pull request, never edit the main checkout (`/Users/polarzero/code/projects/microsandbox-workspaces`) or another worktree.
- Native builds need the private GitHub build file. Copy it once and never print or commit it:
  `cp /Users/polarzero/code/projects/microsandbox-workspaces/app/SiloUI/github-build.local.json app/SiloUI/`
- Your task names the build state directory; `export SILO_BUILD_STATE=<it>` in your shell (the default is `<integration checkout>.state`). It holds the flaky log; never put your work there.
- Frontend: `npm --prefix app/SiloUI ci` once in your worktree.
- Before anything builds `silo-ui`, run `tools/agent/runtime-cache` once in your worktree instead of `runtime:prepare` (`--status` shows hit or miss).

## Build folder

- Build in your worktree's own `app/SiloUI/target`. Never set `CARGO_TARGET_DIR`.
- Rust builds go through mbx (the `cargo` on PATH is its shim): compiled crates are shared from one cache across every checkout, so a fresh worktree restores what is already built and only compiles what you changed. `target` is a link into that cache; never replace it with a real folder or copy it.
- `rust-toolchain.toml` at the root pins the Rust version: run plain `cargo`, never `+1.94.0`, `+stable` or similar. Tests run with cargo-nextest 0.9.151 (`cargo install cargo-nextest --locked --version 0.9.151`).
- Do not delete the folder when you finish; the orchestrator does after the merge (`tools/agent/clean`).
- Disk is tight. Send build and test output to a log file and read the summary lines. No release profiles unless your item needs one.

## Scope

- Do your item and nothing beside it. Something broken that is not yours goes in your report.
- Other children work at the same time. Keep changes to shared files small and additive (`api/src/methods/registry.rs`, `silod/src/methods/mod.rs`, `application-source.ts`, `production-source.ts`, `application-app.tsx`, `application-shell.tsx`, fixtures).
- After an API change run both generators and commit their output: `cargo run -p silo-api --example generate_requests` and `--example generate_schema` (from `app/SiloUI`). The committed-schema test (`committed_schema_matches_the_wire_types_and_registry`) pins the whole API surface; there are no counts to update.
- Wire names, saved file formats and checkpoint kinds keep their spelling unless your item says otherwise. Old saved data must still load.
- Do not start, stop or connect to a running Silo or Silo Dev engine or app, and do not touch production data, unless your task gives you the Dev engine for a live check.

## Proof

- Run tests to prove your change or to find what broke, not as a ritual.
- `tools/agent/affected [--base REF] [--why]` shows what your diff touches. `tools/agent/check [--base REF] [--quick]` runs fmt, `cargo check --tests`, the affected tests, touched script tests and, for frontend changes, typecheck, oxlint and related Vitest files, then prints failures and a verdict. `--quick` stops after compile and typecheck. The base defaults to the nearest of `main`/`direction` (`$SILO_INTEGRATION_BRANCH`). The full log is in `app/SiloUI/target/agent-check/`; "already green" means the same tree already passed.
- Use a narrower command (one test, one file) when it proves the point faster.
- Never run `--workspace` or the whole frontend suite; the orchestrator runs the full suites once per merge batch.
- A test that fails then passes: `check` appends it to `$SILO_BUILD_STATE/flaky.log` (outside your branch; it edits no tracked file, so never edit [FLAKY.md](FLAKY.md) yourself). Name it in your report, do not loop on it.
- Linux: `tools/agent/linux-warm [--ref REF] [-- NEXTEST_ARGS]` syncs and tests on the Linux machine; it prints `skipped: offline` and exits 0 when it does not answer. Report that as "Linux skipped: offline".

## Waiting

- Run long commands in the foreground and block until they finish. Never end a turn to wait, never sleep-poll.
- Start no background wait (Monitor, `sleep` loop, watcher, `&`) unless you must. Stop every one before your final message: a wait that fires after it replaces your last message and the orchestrator loses the report.
- Never push to see CI. Never run `gh run` or `gh pr checks`.
- Live checks: `SILO_AGENT_ITEM=<item> app/SiloUI/scripts/live/run <check> [--host mac|linux]` blocks until done; its exit code is 0 pass, 1 fail, 2 usage, 3 skipped (also when another check holds the shared Dev engine: the message names the holder, wait and rerun). To keep working meanwhile, `run start <check> ...` (prints `<state-dir> <run-id>`) then `run wait <state-dir> [<run-id>]` (same exit codes; never `run <check> &` + `run wait`). Under `atelier tmp --short -- ...` the harness keeps its state, short temp homes and one `leftovers` line per throwaway computer in `$ATELIER_TMP`. A check's `cleanup()` must stop the computers it started.

## Changesets and comments

- A user-visible change needs a changeset in `app/SiloUI/.changeset/` (`"silo-ui": minor` for a feature, `patch` for a fix), written for someone using the app. Internal changes need none.
- Comments describe what the code does. No notes about the change, the plan or a reviewer; match the density of neighbouring code.
- User-facing words follow [the vocabulary](../SiloUI-VOCABULARY.md): computer, device, merge, replace main. Plain language in every message a person or agent will read.

## Report

Your **final message is the whole report**, in at most 400 words. The orchestrator sees nothing else: never write "the report above stands" or end on a tool call, a wait or a notification. Before it, stop every background wait. Include:

1. Branch and last commit.
2. What you built, in plain words.
3. Checks you ran: exact commands and results, including the `check` verdict line (copy it, for example `VERDICT: GREEN` or `PARTIAL`) for your final tree. Do not claim a check you did not run.
4. What is not done or not verified.
5. Flaky tests (from `$SILO_BUILD_STATE/flaky.log`), broken things outside your scope, and anything the orchestrator should decide.
