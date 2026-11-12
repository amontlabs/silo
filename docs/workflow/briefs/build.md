# Build task: {ITEM}

You are building one item. Read [CHILD.md](../CHILD.md) first; it holds the rules.

- Item: {ITEM}: {GOAL}
- Worktree: {WORKTREE}
- Base branch: {BASE} (start from it; later items may stack on your commits)
- Design and docs to read before coding: {DOCS}
- Files you will touch: {FILES}
- Files other children are editing now (keep changes there small and additive): {SHARED_FILES}
- Done means: {DONE_CRITERIA}
- Proof to run: {CHECKS} (use `tools/agent/check`; narrower commands when faster)
- Live check, if any: {LIVE_CHECK} (Dev engine: {DEV_ENGINE})
- Changeset: {CHANGESET: none | patch | minor}

Commit as you go. Do not push. Report in at most 400 words in the format of CHILD.md.
