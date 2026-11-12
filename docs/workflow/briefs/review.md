# Review: {ITEM}

Read-only. Change no files.

- Worktree: {WORKTREE}
- Diff: run `{DIFF_CMD}` (for example `git diff {BASE}...{BRANCH}`)
- What the change is meant to do: {INTENT}
- Risk areas: {RISKS}
- Not in scope: {OUT_OF_SCOPE}

## Round 1 (full)

Report correctness and security problems only, most severe first. For each problem class, list **every instance** in one pass, not just the first. Each finding has:

- `file:line`
- the concrete failure (input or state, then the wrong result)
- the fix in one line

No style notes, no "consider", no guesses without a failure. If you find nothing that matters, say "No findings." Do not run the full test suites; read the code and run only a narrow test if it settles a doubt.

## Round 2 (follow-up; use instead of Round 1 for later rounds)

Fixes landed in {FIX_COMMITS}. Check only that each listed finding is fixed everywhere it occurs and what the fix touches ({FIX_DIFF_CMD}). Do not re-review the rest. Same finding format. Previous findings:

{PREVIOUS_FINDINGS}
