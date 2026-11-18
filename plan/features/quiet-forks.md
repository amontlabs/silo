## Proposed

- A fork can be made with the agent's memory and configuration emptied and its login kept, so nothing the parent's agent remembered carries over.

## Open

- Can a recipe name services that Silo starts in a fork before its task, while the other services the fork copied stay off?
  - Suggested: Yes, as an explicit list in the recipe, so a task finds its daemon running with no setup, as `tasks-start-ready` asks.

## Notes

- Old-app finding: a Linux fork of a running main was a twin. A poller ran 24 times and cron twice in 130 s; hostname, machine id and tokens were the same.
- Fork with a cold boot; regenerate machine id, hostname and SSH host keys; pause cron, launchd and the recipe's `quiet:` commands; apply the recipe's scrub paths.
- Test: fork a main running a chat-agent gateway, `gh` and a Telegram bot; no double replies, and main's tokens still work.

## From the old code

The old code had it built and driven end to end, about 1,300 lines, and it needs reworking.

Lessons:
- MicroSandbox has no hook to edit a snapshot's disk before boot, so the quiet script runs from Silo's boot hook first and the engine confirms its marker after start.
- Match process names by exact base name, never by pattern.
- Login-time programs and file-stored secrets are not covered; scrub is the answer for files.
- A memory snapshot copies logins that a token proxy cannot remove, so forks are disk only.
- A fork is stopped and never runs briefly as a side effect.
- About 9.8 s and 0.5 GiB per Linux fork, 59 s per macOS fork, measured on one M4 Max.
- MicroSandbox has no hook to edit a snapshot before boot; quieting runs from a boot hook carried in the runtime patch.
- Shell profiles and login-started programs are not covered.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CHECKPOINTS-PLAN.md#L132`
- `app/SiloUI/src-tauri/engine/src/recipes/quiet.rs`
- `app/SiloUI/patches/microsandbox-create-stopped-0.7.14.patch`
- `docs/v1/evidence/phase-1/linux-quiet-check.txt`
- `docs/forks-ci/BUILD_LOG.md#L77`
- `docs/v1/evidence/final/README.md`
- `docs/SiloUI-RECIPES.md`
- `docs/SiloUI-CHECKPOINTS-PLAN.md#quiet-linux-forks`

Calls: `checkpoints.fork`, `silo fork`, `fork -n`, `silo status`.
