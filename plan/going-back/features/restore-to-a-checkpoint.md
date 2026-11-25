# Restore

## The choice

An agent's part, 22 November 2026. In a fork where one agent works, a migration goes wrong at 15:40.
You restore the fork to 15:38. Silo first saves 15:40 as a checkpoint, then the disk and the agent's
conversation go back to 15:38.

The options:

- Build it (recommended). One step puts a computer back to a checkpoint, main included, while the
  record, the rules and what was spent stay as they are now. It costs the programs that were
  running, which end; in main every agent goes back, not only the one that went wrong.
- Leave it out. You go back only by forking from a checkpoint and making the fork main. It costs
  steps each time, and on a Mac a second macOS computer to run the fork, which Apple allows only
  once beside main.

Recommended: build it. When one agent works alone in a computer, putting that computer back is the
direct answer, and forks stay for when others are at work in it.

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 9 there). In option A, restore is the only way back on the first morning. Codex deletes `~/work/shared-fixtures/` at 02:40, and Claude Code works in `~/work/app` until 05:58. Restoring main to 02:40 takes back three hours of Claude Code's work to bring back one folder. Done by hand, it is two restores and a copy out and in over SSH, and the tmux session in main ends twice. #370's step 5 has `silo look` and `silo put-back` for exactly this case, and #373 moves both out of A. Recommended: keep them in A. `put-back` needs the reader, which A builds in step 5, and #378's care for a copy made as root in a computer an agent controls.
