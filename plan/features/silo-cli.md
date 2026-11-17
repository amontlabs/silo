## From the old code

The old code had it built and driven end to end, about 7,900 lines, and its approach can be reused.

Lessons:
- Keep one operations layer that both the CLI and MCP call, returning the same JSON, so the two never drift.
- Exit codes are a contract agents script against; changing one breaks callers silently.
- Never touch the production link from a Dev build; names come from one channel module.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CLI.md#commands`
- `app/SiloUI/cli/src/cli.rs`
- `app/SiloUI/cli/src/ops.rs`
- `app/SiloUI/cli/src/render.rs`
- `app/SiloUI/cli/tests/commands.rs`
- `docs/SiloUI-CLI.md#install`
- `app/SiloUI/host/src/channel.rs`
- `docs/SiloUI-CLI.md#engine-socket`
- `app/SiloUI/cli/src/engine.rs`
- `app/SiloUI/cli/src/endpoint.rs`

Calls: `silo`.
