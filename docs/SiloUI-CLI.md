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
| `silo list [--all]` | computers and their state; `--all` also shows the hidden throwaways (`silo-baseline-*`, `silo-build-*`) |
| `silo status <computer>` | one computer: size, checkpoints, its `generation` (moves when the engine sees a change), a fork's quiet state, assigned secrets and GitHub access |
| `silo secrets list` | secret names and the computers they are assigned to (never values) |
| `silo secrets set NAME [--computer C]... [--domain D]...` | add a secret or replace its value; the value is read from stdin, never an argument |
| `silo secrets assign NAME COMPUTER` / `silo secrets unassign NAME COMPUTER` | change which computers have a secret |
| `silo create <name> [--linux\|--macos] [--cpus N --memory GiB --disk GiB]` | create a computer |
| `silo start <computer>` / `silo stop <computer>` | lifecycle |
| `silo retry-setup <computer>` | finish the setup of a macOS computer that ended `failed` before it was ready |
| `silo up [--recipe R]` | make `main` from a recipe if needed, start it, publish the recipe's ports on it, make `ssh main.silo` work (`main.silo-dev` from a development build), keep a warm checkpoint ([recipes](SiloUI-RECIPES.md)) |
| `silo ports <computer>` | list the published ports with their host port and URL |
| `silo publish <computer> <port> [--http]` / `silo unpublish <computer> <port>` | publish or stop publishing a guest port; `--http` serves it as a website with a URL per computer ([URL proxy](SiloUI-URL-PROXY.md)) |
| `silo ssh-setup <computer>` | make `ssh <computer>.silo` reach a computer. A Linux computer gets SSH access turned on and an entry in `~/.silo/ssh-hosts.conf` (`~/.silo-dev/` for a development build), which `~/.ssh/config` includes; a macOS computer's saved entry also answers to the name. After `silo fold --promote` or its undo the name is written again for the computer that now has it (a macOS entry follows on its own) |
| `silo recipe ls\|build NAME [--force]\|rm NAME` | list, build or remove recipe builds |
| `silo fork NAME --recipe R` | fork from the cached build of recipe R (built first if missing) |
| `silo fork <source> [--name N] [--checkpoint ID] [--secret NAME]... [--recipe-file PATH]` | copy a computer; the name defaults to `<source>-<6 hex>` |
| `silo fork <source> -n N [--name PREFIX]` | N forks of the same checkpoint, made at once and started; each fork's name and URLs are printed ([details](#several-forks-and-commands)) |
| `silo fork <source> [-n N] -- <cmd...>` | fork, start, run the command in the fork (output streams, the exit code is passed through) and leave the fork for review |
| `silo fork ./dir` | copy a host folder's git repository into a fresh Linux fork ([details](#folder-forks)) |
| `silo fold <fork> --request` | record a review request for the fork and print its id; nothing is kept |
| `silo diff <computer> [--json] [--max-lines N] [--no-baseline] [--baseline]` | what changed since the fork (captures the computer now, compares with the record `silo fork` took); `--baseline` measures a missing baseline first, on a throwaway idle fork of the same checkpoint (also for a fork that has been used), and a baseline measured on one fork of a computer applies to its other forks; `silo fold --dry-run` ignores review digests |
| `silo fold <fork> --promote [--into C] [--dry-run]` | make the fork main: it takes main's name, ports, secrets and GitHub access, and the old main is archived as `main-before-<time>`; `silo undo main` swaps back ([details](SiloUI-CHANGES.md#making-a-fork-main-silo-fold-fork---promote)) |
| `silo undo <computer\|fork\|./dir>` | put a computer back to before its latest keep, or a folder back to before its latest fold |
| `silo drop <computer> [--force]` | throw a computer away (a hidden throwaway too, by name or id); `main` needs `--force`. A stopped `silo-baseline-*` copy more than an hour old is dropped by the next fork or `silo diff --baseline` |
| `silo exec <computer> [--cwd DIR] [--timeout SECS] -- <cmd...>` | run a command; the exit code is passed through |
| `silo checkpoint <computer> [--name N] [--kind manual\|automatic\|before-keep\|warm\|step]` | save a checkpoint |
| `silo checkpoints <computer>` | list checkpoints |
| `silo steps <computer>` | the commands run in the computer through Silo, numbered, with exit code and the checkpoint each left ([steps](SiloUI-CHANGES.md#steps-and-the-fork-tree)) |
| `silo fork <computer> --from-step N` | fork from the checkpoint taken after step N |
| `silo tree` | the computers as a tree: what each was forked from, its state and pending review |

A fork publishes the ports its source publishes (a recipe fork, the recipe's `ports`), each on a host port of its own. The engine does not copy published ports when it forks a computer, so `silo fork` does it with the same `publish` calls. The MCP server has `ports`, `publish`, `unpublish` and `ssh_name` tools.
| `silo job <id>` | wait for a job that is already running |
| `silo mcp` | serve the MCP tools on stdio |

## Several forks and commands

`-n N` (1 to 32) makes N forks of one checkpoint concurrently: the warm checkpoint from
`silo up` when the engine can tell `main` has not changed since (and it is under a minute old; see [recipes](SiloUI-RECIPES.md#silo-up)), else one new checkpoint (`silo-fork-<id>`, kept,
because `silo diff --baseline` forks from it) that all N share. Names are `<source>-<6 hex>`,
or `<prefix>-1` to `<prefix>-N` with `--name`. Each fork is started and its URLs
(`network.urls`) are printed. Without `-n` or a command, `silo fork` makes one stopped fork
as before.

Limits are checked before anything is made:

- macOS: Apple allows two macOS VMs at once, so N may not exceed the free slots (2 minus the
  macOS computers running, the source included). Fork a Linux computer for more.
- `fork_ram_budget` in `.silo/silo.yaml` (for example `8G`): the running forks (every running
  computer except `main`) plus N more must fit. The engine does not report how much memory a
  running computer uses, so each counts as the memory it was created with. The command refuses
  and says how many would fit.

`-- <cmd...>` runs the command in each fork once it is started, with no time limit (it uses the
engine's streaming exec). With one fork the output goes to stdout and stderr as it arrives and the
exit code is the command's own (128 plus the signal when it was killed). With `-n N` each line is
prefixed with `[fork]`, a per-fork summary follows, and the exit code is the first non-zero one.
The forks stay for review: `silo diff <fork>`, then `silo fold <fork>` or `silo drop <fork>`.

## Folder forks

`silo fork ./repo` needs the top folder of a git repository with a commit. It makes a fork
of the `linux-dev` recipe (built if needed; `--name`, `--secret` and `silo.yaml` limits apply),
starts it, sends `git bundle --all` through `files.upload` (into a `/workspace/silo-in-<id>` folder,
a root the transfer API allows, removed after the clone) and clones it to `~/<folder>` in the
fork. Only commits are copied; uncommitted changes stay on the host (the command says so). The
CLI remembers the host path, the commit the fork started from and the branch, beside the engine
socket (`cli-folders.json`). Words after `--` run in the repository inside the fork.

`silo fold <fork>` on such a fork keeps into the folder instead of a computer:

1. The host folder must be on a branch with no uncommitted changes to tracked files; otherwise
   the fold is refused and changes nothing. Uncommitted work in the fork is committed there
   first (`silo: uncommitted changes in <fork>`).
2. The fork's commits since the starting commit come back as a bundle (`files.download` from a
   `/workspace/silo-out-<id>` folder in the fork, removed after) and are fetched into
   `refs/silo/<fork>/head` in the folder. The fold is refused, before anything is changed,
   when the commits add or change a `.gitattributes` file (it could select filter or merge
   programs the folder's git configuration defines), or add files where the folder has files git
   does not track (untracked or ignored; they are listed).
3. A backup ref `refs/silo/backup/<UTC time>` marks the folder's HEAD, then the branch is
   fast-forwarded, or merged when the folder moved on. A merge that would conflict is refused
   (the files are listed) before the tree is touched; the backup ref is made only for a fold that
   goes ahead.

`--dry-run` stops after step 2 and adds only the `refs/silo/<fork>/head` ref. `silo undo <fork>`
or `silo undo ./repo` resets the branch to the backup ref; it refuses when the folder has
uncommitted changes to tracked files, has new commits since the fold, or has untracked or ignored
files where the reset would write files. Every host git command of a fold or undo runs with
`core.hooksPath=/dev/null`, `core.fsmonitor=false` and `protocol.ext.allow=never`, and merges with
`--no-verify`, so no hook or program named by the folder's configuration runs because of a fork's
commits (the folder's own commit hooks do not run for the merge commit either); its identity and
other settings still apply. Everything done on the
host stays inside the repository (its `.git/silo` folder holds the bundles in transit, removed
afterwards). A folder fork cannot be made main or kept `--into` a computer.

## Review requests

`silo fold <fork> --request` takes a fresh `silo diff` of the fork (for a folder fork, its commits
and uncommitted files) and calls `changes.request`, which stores a fold request in the engine
(`fold-requests/<id>.json` under the app data folder): `id` (`fr-` and 8 hex), `forkId`, `target`
(the computer id; `folder:<path>` for a folder fork), `createdAt`, `reviewDigest`, `summary` and
`status: open`. Asking again for the same fork and target updates that open request. The id is
printed. CI or cron call `silo fork main -- <cmd>` then `silo fold --request`.

- `silo requests [--status open|kept|thrown-away|all]` lists requests (default open) with the
  fork, its state, the computer it would be kept into and the count of changes per bucket.
- `silo requests show <id>` prints the same buckets as `silo diff` for a fresh look at the fork,
  then `Credentials used: ...` (`none` when the fork used no secret; a `Blocked:` line follows
  when the proxy refused something). It says when the fork changed since the request, which makes
  a keep refuse.
- `silo requests keep <id> [--data-ok] [--recipe-file PATH]` keeps the fork pinned to the
  request's digest and exits 2 when refused (the request stays open). A folder fork is kept with
  `silo fold`.
- `silo requests throw-away <id>` drops the fork and closes the request; `main` and the
  request's target are refused.
- `silo requests open <id>` starts the fork if it is stopped and prints its name, SSH name and
  URLs.

Each takes `--json`. The MCP tools are `requests_list`, `request_show`, `request_keep`,
`request_throw_away` and `request_open`, on the same engine methods.

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
step_checkpoints: true           # checkpoint a computer after each step (default false)
step_checkpoints_keep: 20        # step checkpoints kept per computer (default 20; 0 keeps all)
warm_max_age: 60s               # how old main's warm checkpoint may be for a fork to use it (and only if main is unchanged)
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

Tools: `up`, `recipe_list`, `list`, `status`, `steps_list`, `tree`, `fork` (with `recipe`, `count`, `command`, `fromStep`, or a repository path as `source`), `fold` (with `promote` or `request`), `undo`, `drop`, `exec`, `checkpoint`, `checkpoints`, `secrets_list`. They make the
same engine calls as the commands and return the same JSON as `--json`. `drop` cannot
drop `main` through MCP. No tool sets or assigns secrets; values are never sent over MCP.

## Testing against a local GitHub server

A debug build of the engine sends its `api.github.com` requests to the URL in
`SILO_GITHUB_API_BASE` when it is a loopback `http` or `https` origin (`localhost`,
`127.0.0.1` or `[::1]`, with an optional port and no path). Anything else is ignored. A
release build never reads the variable.
