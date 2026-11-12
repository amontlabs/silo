# Flaky tests

A test that fails and then passes without a code change is logged here and becomes a fix item. Nobody retries it in a loop.

## Format

One line per test, newest last:

- `DATE` `binary test` failed, then passed on retry (`command`)

`tools/agent/check` appends the line itself when nextest reports a test that failed and then passed on retry (a test already listed is not added again). Add a line by hand in the same format for a flaky frontend or script test.

## Closing

The orchestrator puts a fix item on the board and appends ` — fix: <item>` to the line. The child that fixes the test deletes the line in the fixing commit (git keeps the history); a later flake of the same test is added again as new.

## Entries

- 2026-11-12 `silo-engine audit::tests::old_rows_and_rows_beyond_the_cap_are_trimmed` failed, then passed on retry (`cargo nextest run --config-file /Users/polarzero/code/projects/silo-direction/app/SiloUI/target/agent-check/20261112-180622-84926.nextest.toml --manifest-path /Users/polarzero/code/projects/silo-direction/app/SiloUI/Cargo.toml --locked --profile check --no-fail-fast --workspace -E all()`)
- 2026-11-12 `silo-engine audit::tests::owner_and_device_rows_are_trimmed_among_themselves` failed, then passed on retry (`cargo nextest run --config-file /Users/polarzero/code/projects/silo-direction/app/SiloUI/target/agent-check/20261112-180622-84926.nextest.toml --manifest-path /Users/polarzero/code/projects/silo-direction/app/SiloUI/Cargo.toml --locked --profile check --no-fail-fast --workspace -E all()`)
