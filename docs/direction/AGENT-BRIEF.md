# Direction build: rules for implementing agents

You are one of several agents building [the plan](PLAN.md). Your task message names your item.
Read the plan's Design section for your item, and [the direction](../DIRECTION.md), before you
write code. The repository's own rules are in [AGENTS.md](../../AGENTS.md); they apply too.

## Your checkout

- You work in your own git worktree. The build is integrated on the local branch `direction`
  (checked out at `~/code/projects/silo-direction`). Before anything else run
  `git merge --ff-only direction` so you start from the latest merged work. If that fails because
  your new branch was cut from `main` (which has moved on its own) and holds no work of yours,
  run `git reset --hard direction` instead. Commit on your own
  branch as you go. Do not push, do not open a pull request, and do not edit the main checkout
  at `/Users/polarzero/code/projects/microsandbox-workspaces` or the integration checkout.
- Native builds need the private GitHub build file. Copy it once, without printing it:
  `cp /Users/polarzero/code/projects/microsandbox-workspaces/app/SiloUI/github-build.local.json app/SiloUI/`.
  It is ignored by Git; never commit or print it.
- Build in your worktree's own `app/SiloUI/target`. Do not set `CARGO_TARGET_DIR`. A git hook
  fills that folder at worktree creation with a copy-on-write clone of the main checkout's build,
  so dependencies are already built and the folder only takes space for what you change. Do not
  delete it when you finish; the orchestrator does that after the merge.
  Frontend needs `npm --prefix app/SiloUI ci` once in your worktree.
- Disk is tight. Send build and test output to a log file and read the summary lines. Do not
  build release profiles unless your item needs one.

## Scope

- Do your item and nothing beside it. If you find something broken that is not yours, write it
  in your report.
- Other agents are working at the same time on other parts. Keep changes to shared files small
  and additive: `api/src/methods/registry.rs`, `silod/src/methods/mod.rs`,
  `src/features/application/model/application-source.ts`, `src/desktop/production-source.ts`,
  `application-app.tsx`, `application-shell.tsx`, fixtures.
- After any API change run both generators and commit their output:
  `cargo run -p silo-api --example generate_requests` and `--example generate_schema`
  (from `app/SiloUI`). Update the pinned counts in `registry.rs` and `schema.rs` tests.
- Wire names, saved file formats and checkpoint kinds keep their spelling unless your item says
  otherwise. Old saved data must still load.
- A user-visible change needs a changeset file in `app/SiloUI/.changeset/` (`"silo-ui": minor`
  for a feature, `patch` for a fix), written for someone using the app.
- Comments describe what the code does. No notes about the change, the plan or the reviewer.
- User-facing words follow the direction: computer, device, merge, replace main. Plain language
  in every message a person or an agent will read.

## Checks before you report

- `cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check`
- The focused tests for the crates you touched, for example
  `cargo test --manifest-path app/SiloUI/Cargo.toml -p silo-api --locked`,
  `-p silod <filter>`, `-p silo-engine <filter>`, `-p silo-cli`.
- Frontend: `npm --prefix app/SiloUI run typecheck`, `run lint`, and the Vitest files you touched.
- Do not start, stop or connect to any running Silo or Silo Dev engine or app, and do not use the
  production app or its data, unless your task message says your item includes a live check and
  gives you the Dev engine. Unit tests use temporary folders.

## Review

When your checks pass, get a review before you report:

1. Start a review with the Agent tool, `model: "reviewer"`, `run_in_background: false`. Give it
   your worktree path, the command `git diff direction...HEAD`, what the change is meant to do, and
   ask for correctness and security findings only, most severe first, at most ten, each with
   file and line and a concrete failure.
2. Fix what is real. Say why for anything you reject.
3. Review again after substantial fixes. Stop after three rounds or when it finds nothing
   that matters.

If the reviewer cannot be started, say so in your report and stop there.

## Report

End with a report of at most 400 words: your branch name and last commit; what you built, in
plain words; what you checked and the result, with the exact commands; what the reviewer found
and what you did; what is not done or not verified; anything the owner should decide. Do not
claim a check you did not run.
