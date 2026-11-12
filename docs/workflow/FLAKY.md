# Flaky tests

A test that fails and then passes without a code change is logged here and becomes a fix item. Nobody retries it in a loop.

## Format

One row per test, newest last. Open entries have an empty `Closed` column.

| Test | First seen | Failure (one line) | Evidence | Fix item | Closed |
|---|---|---|---|---|---|

- **Test**: full nextest or Vitest name.
- **First seen**: date and commit.
- **Failure**: the first failing line, copied.
- **Evidence**: path of the log or JUnit file (outside Git) or the command that showed it.
- **Fix item**: board item or commit that fixes it.
- **Closed**: date and commit of the fix, or `not reproducible` with the number of clean runs.

## Adding

The child or orchestrator who saw the failure adds the row in the same commit as their work, or `check` appends it from nextest's JUnit output when a test fails then passes. Then the orchestrator puts a fix item on the board.

## Closing

The child that fixes it fills `Closed` in the fixing commit. An entry with no fix item after a merge batch is a problem for the orchestrator to schedule.

## Entries

