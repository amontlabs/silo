# The `silo` command line and MCP server

`silo` drives computers and forks through the running Silo engine. It lives in
`app/SiloUI/cli` (crate `silo-cli`, binary `silo`) and talks to `engine.sock` with
`silo-client`, using the typed request constructors from `silo-api`.

## Choices

| Need | Choice |
| --- | --- |
| Argument parsing | `clap` (derive) |
| YAML | `serde_yaml_ng` (maintained fork of `serde_yaml`); `deny_unknown_fields` gives unknown-key errors with a line number |
| MCP | `rmcp`, the official Rust SDK (`#[tool]` macros, stdio transport) |
| Engine access | existing `silo-client` and `silo-api`; no new protocol code |

Gaps: none known. Jobs are awaited by polling `operations.job.get`, as the app does.

## Engine socket

In order: `--socket PATH`, `SILO_SOCKET`, then the build channel's own
`~/.silo/engine.sock` (a debug build uses `~/.silo-dev/engine.sock`, like the app).
If nothing listens, the command asks you to open Silo.

## Commands

Every command accepts `--json`. Progress goes to stderr; a failed job exits non-zero with
the engine's message.

| Command | What it does |
| --- | --- |
| `silo list` | computers and their state |
| `silo status <computer>` | one computer |
| `silo create <name> [--linux\|--macos] [--cpus N --memory GiB --disk GiB]` | create a computer |
| `silo start <computer>` / `silo stop <computer>` | lifecycle |
| `silo fork <source> [--name N] [--checkpoint ID] [--secret NAME]... [--recipe-file PATH]` | copy a computer; the name defaults to `<source>-<6 hex>` |
| `silo drop <computer> [--force]` | throw a computer away; `main` needs `--force` |
| `silo exec <computer> [--cwd DIR] [--timeout SECS] -- <cmd...>` | run a command; the exit code is passed through |
| `silo checkpoint <computer> [--name N] [--kind manual\|automatic\|before-keep]` | save a checkpoint |
| `silo checkpoints <computer>` | list checkpoints |
| `silo mcp` | serve the MCP tools on stdio |

## Project files

`silo` looks upward from the working directory for a `.silo/` that holds `recipe.yaml` or
`silo.yaml`. Unknown keys are errors.

`.silo/recipe.yaml`:

```yaml
quiet: [poller]          # processes stopped in a fork
scrub: [/root/.env]      # paths removed from a fork
keep:
  restart: [web]         # services restarted after a keep
  migrate: [make migrate]
  after: [make test]
```

`.silo/silo.yaml`:

```yaml
recipes: [linux-dev]
fork_secrets: [OPENAI_API_KEY]   # what --secret may name
fold: ask                        # or auto-code
fork_ram_budget: 8G
auto_keep:
  - when: { only: [code], tests: pass }
agents:
  openclaw: { fold: ask, fork_secrets: [] }
```

When `silo.yaml` exists, `--secret` accepts only names in `fork_secrets`. `silo fork` reads
`quiet:` and `scrub:` from `--recipe-file`. The engine's fork parameters do not carry
secrets or the quiet list yet; `ops::apply_fork_extras` is where they are attached once
they do.

## MCP setup

```sh
claude mcp add silo -- silo mcp
```

Tools: `list`, `status`, `fork`, `drop`, `exec`, `checkpoint`, `checkpoints`. They make the
same engine calls as the commands and return the same JSON as `--json`. `drop` cannot
drop `main` through MCP.
