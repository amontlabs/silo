# Flaky tests

A test that fails and then passes without a code change is logged here and becomes a fix item. Nobody retries it in a loop.

## Flow

1. `tools/agent/check` appends one line to `$SILO_BUILD_STATE/flaky.log` (default `<integration checkout>.state/flaky.log`) when nextest reports a test that failed and then passed on retry; a test already listed is not added again. It edits no tracked file, in no checkout.
2. A flaky frontend or script test that `check` cannot see: the child names it in its report; the orchestrator adds the line.
3. The orchestrator moves new lines from `flaky.log` into [Entries](#entries) below (committed in the integration checkout between merges; merge-batch needs a clean tree), deletes them from the log, and puts a fix item on the board.

## Format

One line per test, newest last:

- `DATE` `binary test` failed, then passed on retry (`command`)

## Closing

The fix item's line gets ` — fix: <item>` appended. The child that fixes the test deletes the line in the fixing commit (git keeps the history); a later flake of the same test is added again as new.

## Entries

