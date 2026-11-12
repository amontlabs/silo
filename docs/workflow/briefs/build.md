# Build task: {ITEM}

You are building one item. Read [CHILD.md](../CHILD.md) first; it holds the rules.

- Item: {ITEM}: {GOAL}
- Worktree: {WORKTREE}
- Build state directory: {STATE_DIR} (`export SILO_BUILD_STATE={STATE_DIR}`; `check` logs flaky tests there)
- Base branch: {BASE} (start from it; later items may stack on your commits)
- Design and docs to read before coding: {DOCS}
- Files you will touch: {FILES}
- Files other children are editing now (keep changes there small and additive): {SHARED_FILES}
- Done means: {DONE_CRITERIA}
- Proof to run: {CHECKS} (use `tools/agent/check`; narrower commands when faster)
- Live check, if any: {LIVE_CHECK} (Dev engine: {DEV_ENGINE})
- Changeset: {CHANGESET: none | patch | minor}

Commit as you go. Do not push. Stop every background wait (Monitor, loops) before your last message, and never end a turn to wait. Your final message is the whole report, at most 400 words, in the format of CHILD.md, including the `check` verdict line for your final tree.
