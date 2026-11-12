# Flaky tests

A test that fails and then passes without a code change is logged here and becomes a fix item. Nobody retries it in a loop.

## Flow

1. `tools/agent/check` appends one line to `$SILO_BUILD_STATE/flaky.log` (default `<integration checkout>.state/flaky.log`) when nextest reports a test that failed and then passed on retry; a test already listed is not added again. It edits no tracked file, in no checkout.
2. A flaky frontend or script test that `check` cannot see: the child names it in its report; the orchestrator adds the line.
3. The orchestrator moves new lines from `flaky.log` into [Entries](#entries) below (committed in its own worktree, never in the integration checkout while a merge is pending), deletes them from the log, and puts a fix item on the board.

## Format

One line per test, newest last:

- `DATE` `binary test` failed, then passed on retry (`command`)

## Closing

The fix item's line gets ` — fix: <item>` appended. The child that fixes the test deletes the line in the fixing commit (git keeps the history); a later flake of the same test is added again as new.

## Entries

- 2026-11-12 `silo-engine audit::tests::old_rows_and_rows_beyond_the_cap_are_trimmed` failed, then passed on retry (`cargo nextest run --config-file /Users/polarzero/code/projects/silo-direction/app/SiloUI/target/agent-check/20261112-180622-84926.nextest.toml --manifest-path /Users/polarzero/code/projects/silo-direction/app/SiloUI/Cargo.toml --locked --profile check --no-fail-fast --workspace -E all()`)
- 2026-11-12 `silo-engine audit::tests::owner_and_device_rows_are_trimmed_among_themselves` failed, then passed on retry (`cargo nextest run --config-file /Users/polarzero/code/projects/silo-direction/app/SiloUI/target/agent-check/20261112-180622-84926.nextest.toml --manifest-path /Users/polarzero/code/projects/silo-direction/app/SiloUI/Cargo.toml --locked --profile check --no-fail-fast --workspace -E all()`)
- 2026-11-12 `silo-engine backup::tests::timeout_also_stops_gracefully_and_ignored_term_is_killed_after_the_grace` failed, then passed on retry (`cargo nextest run --config-file /Users/polarzero/code/projects/silo-direction/app/SiloUI/target/agent-check/20261112-182559-97319.nextest.toml --manifest-path /Users/polarzero/code/projects/silo-direction/app/SiloUI/Cargo.toml --locked --profile check --no-fail-fast -p silo-api -p silo-cli -p silo-client -p silo-engine -p silo-ui -p silod -E rdeps(silo-api) | rdeps(silo-cli) | rdeps(silo-engine) | rdeps(silod)`)
- 2026-11-12 `silo-engine backup::tests::large_computer_list_is_read_whole_and_oversized_json_fails_explicitly` failed, then passed on retry (`cargo nextest run --config-file /Users/polarzero/code/projects/silo-direction/app/SiloUI/target/agent-check/20261112-182559-97319.nextest.toml --manifest-path /Users/polarzero/code/projects/silo-direction/app/SiloUI/Cargo.toml --locked --profile check --no-fail-fast -p silo-api -p silo-cli -p silo-client -p silo-engine -p silo-ui -p silod -E rdeps(silo-api) | rdeps(silo-cli) | rdeps(silo-engine) | rdeps(silod)`)
