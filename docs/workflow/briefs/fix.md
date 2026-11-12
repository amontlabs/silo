# Fix task: {ITEM}

Same worktree and branch as before: {WORKTREE}, branch {BRANCH}. Rules: [CHILD.md](../CHILD.md).

{HANDOVER: only when this is a fresh session; what was built, what state the tree is in}

Fix exactly these findings. Each has file:line and a concrete failure.

{FINDINGS}

For each: fix every instance of the problem (search for the same pattern elsewhere in the diff), or say why it is not a problem. Prove each fix with the narrowest test that fails before and passes after.

Do nothing beyond the list. Commit. Stop every background wait (Monitor, loops) first and never end a turn to wait. Your final message is the whole report, at most 150 words: commit, what changed per finding, checks run with results, including the `check` verdict line for your final tree.
