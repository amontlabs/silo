# Flaky tests

A test that fails and then passes without a code change is logged here and becomes a fix item. Nobody retries it in a loop.

## Format

One line per test, newest last:

- `DATE` `binary test` failed, then passed on retry (`command`)

`tools/agent/check` appends the line itself when nextest reports a test that failed and then passed on retry (a test already listed is not added again). Add a line by hand in the same format for a flaky frontend or script test.

## Closing

The orchestrator puts a fix item on the board and appends ` — fix: <item>` to the line. The child that fixes the test deletes the line in the fixing commit (git keeps the history); a later flake of the same test is added again as new.

## Entries

