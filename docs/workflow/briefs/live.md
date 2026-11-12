# Live check: {CHECK} ({ITEM})

Run one live check with the harness and report. Rules: [CHILD.md](../CHILD.md) (waiting, scope), [AGENTS.md](../../../AGENTS.md#running-and-debugging).

- Worktree: {WORKTREE} (branch {BRANCH}; build it first if the harness asks)
- Host: {HOST} (`mac` or `linux`; Linux offline means "skipped: offline")
- Dev engine you may use: {DEV_ENGINE}
- Expected result: {EXPECTED}

Run `SILO_AGENT_ITEM={ITEM} tools/agent/live {CHECK} --host {HOST}` as one blocking command (exit 0 pass, 1 fail, 2 usage, 3 skipped) and read the `VERDICT` file in its state folder. For a run longer than one command, use `tools/agent/live start {CHECK} --host {HOST}` (prints `<state-dir> <run-id>`) and then `tools/agent/live wait <state-dir> <run-id>`, which prints the verdict and has the same exit codes. On failure read the log of the failed step, fix nothing in code, and rerun with `--from <step>` only after a cause outside the code (setup) is removed. Use only the Dev build and throwaway `e2e-*` computers. For a GUI step call `computer-use` with one bounded task.

Stop every background wait (Monitor, loops) before your last message and never end a turn to wait. Your final message is the whole report, at most 250 words: verdict, steps passed, the failing step with its first error line and log path, anything left running or created (computers still listed in the state's `computers` file).
