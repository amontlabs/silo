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
A socket outside the build's own home is refused unless `--allow-other-channel` is given;
a debug build never uses `~/.silo`, even with the flag. If nothing listens, the command
asks you to open Silo.

If a wait ends before its job does (connection lost, Ctrl-C), the job id is printed and
`silo job <id>` waits for it again. `exec` accepts at most 180 seconds (the engine's
limit) and returns the command's exit code, or 128 plus the signal number.

## Commands

Every command accepts `--json`. Progress goes to stderr; a failed job exits non-zero with
the engine's message.

| Command | What it does |
| --- | --- |
| `silo list` | computers and their state |
| `silo status <computer>` | one computer: size, checkpoints, a fork's quiet state, assigned secrets and GitHub access |
| `silo secrets list` | secret names and the computers they are assigned to (never values) |
| `silo secrets set NAME [--computer C]... [--domain D]...` | add a secret or replace its value; the value is read from stdin, never an argument |
| `silo secrets assign NAME COMPUTER` / `silo secrets unassign NAME COMPUTER` | change which computers have a secret |
| `silo create <name> [--linux\|--macos] [--cpus N --memory GiB --disk GiB]` | create a computer |
| `silo start <computer>` / `silo stop <computer>` | lifecycle |
| `silo retry-setup <computer>` | finish the setup of a macOS computer that ended `failed` before it was ready |
| `silo up [--recipe R]` | make `main` from a recipe if needed, start it, keep a warm checkpoint ([recipes](SiloUI-RECIPES.md)) |
| `silo recipe ls\|build NAME [--force]\|rm NAME` | list, build or remove recipe builds |
| `silo fork NAME --recipe R` | fork from the cached build of recipe R (built first if missing) |
| `silo fork <source> [--name N] [--checkpoint ID] [--secret NAME]... [--recipe-file PATH]` | copy a computer; the name defaults to `<source>-<6 hex>` |
| `silo diff <computer> [--json] [--max-lines N] [--no-baseline] [--baseline]` | what changed since the fork (captures the computer now, compares with the record `silo fork` took); `--baseline` measures a missing baseline first, on a throwaway idle fork of the same checkpoint (also for a fork that has been used), and a baseline measured on one fork of a computer applies to its other forks; `silo fold --dry-run` ignores review digests |
| `silo fold <fork> --promote [--into C] [--dry-run]` | make the fork main: it takes main's name, ports, secrets and GitHub access, and the old main is archived as `main-before-<time>`; `silo undo main` swaps back ([details](SiloUI-CHANGES.md#making-a-fork-main-silo-fold-fork---promote)) |
| `silo drop <computer> [--force]` | throw a computer away; `main` needs `--force` |
| `silo exec <computer> [--cwd DIR] [--timeout SECS] -- <cmd...>` | run a command; the exit code is passed through |
| `silo checkpoint <computer> [--name N] [--kind manual\|automatic\|before-keep\|warm]` | save a checkpoint |
| `silo checkpoints <computer>` | list checkpoints |
| `silo job <id>` | wait for a job that is already running |
| `silo mcp` | serve the MCP tools on stdio |

`silo status` prints, for a fork, a line such as
`quiet: applied; stopped: poller; scrubbed: /root/.env; secrets: none` (on macOS it adds the
count of parked jobs and the jobs left scheduled), then `secrets assigned:` and a `github:`
line (`read-only` or `can push`, with the repositories). `--json` carries the same data as
`quiet` (the fork's record, `null` for other computers), `secrets` (names) and `github`
(`null` when the computer has no assignment, or when the engine could not be read). The quiet
record comes from the engine's per-computer state (`quiet` on a Linux or macOS computer in
`computers.list` and the application state); it is absent on computers that are not forks.

A new secret needs at least one `--computer` and one `--domain`; for an existing secret both
default to what it has. A secret cannot be left with no computer, and a secret for any
destination (`*`) is created in the app. `set` reads all of stdin and drops one final newline:
`printf %s "$VALUE" | silo secrets set NAME --computer main --domain api.example.com`.

## Project files

`silo` looks upward from the working directory for a `.silo/` that holds `recipe.yaml` or
`silo.yaml`. Unknown keys are errors.

`.silo/recipe.yaml` (recipes can also name an OS, packages, setup commands, services and ports, and live in `.silo/recipes/`; see [recipes](SiloUI-RECIPES.md)):

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
`--recipe-file`, else the nearest `.silo/recipe.yaml` (a malformed file is an error). The fork
gets the named secrets (`github` gives read-only GitHub access) and the recipe's `quiet:` and
`scrub:` lists; with neither, the fork gets none of the credentials Silo manages.

## MCP setup

```sh
claude mcp add silo -- silo mcp
```

Tools: `up`, `recipe_list`, `list`, `status`, `fork` (with `recipe`), `drop`, `exec`, `checkpoint`, `checkpoints`, `secrets_list`. They make the
same engine calls as the commands and return the same JSON as `--json`. `drop` cannot
drop `main` through MCP. No tool sets or assigns secrets; values are never sent over MCP.

## Testing against a local GitHub server

A debug build of the engine sends its `api.github.com` requests to the URL in
`SILO_GITHUB_API_BASE` when it is a loopback `http` or `https` origin (`localhost`,
`127.0.0.1` or `[::1]`, with an optional port and no path). Anything else is ignored. A
release build never reads the variable.
