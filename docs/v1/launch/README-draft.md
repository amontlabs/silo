# Silo

> **DRAFT for the owner's approval. Not public text yet.** Proposed top-level README for Silo v1.
> It does not replace `README.md` until approved. Every number in it comes from
> `docs/v1/evidence/final/` and names its file. Commands are the ones the final run used
> (`docs/v1/evidence/final/final-run.txt`), with the production names: the run used a Dev build,
> so it printed `silo-dev`-style names such as `ssh main.silo-dev`.

**Git for your agent's computer.** Fork a running computer, let an agent work in the fork, look at
what changed, then keep it or throw it away. If you keep something by mistake, undo it.

Silo runs Linux and macOS virtual machines on your Mac. One of them is `main`, the computer your
agent normally works on. A fork is a full copy of it, running on its own, with its own URL. When
the agent is done, `silo diff` sorts what it changed into code, packages, settings and files,
databases and the rest; `silo fold` brings the parts you want back into `main`.

Forks get none of the credentials Silo manages unless you name them; files on disk are copied.

[The flow](#the-flow) · [Quiet forks](#quiet-forks) · [Review](#review) · [Keep and undo](#keep-and-undo) ·
[Make this main](#make-this-main) · [A URL per fork](#a-url-per-fork) · [Recipes](#recipes) ·
[MCP](#mcp-setup) · [Computer use](#computer-use) · [Agents inside a computer](#agents-inside-a-computer) ·
[Another Mac](#another-mac) · [Install](#install) · [Numbers](#numbers) · [Not there yet](#not-there-yet)

## The flow

From a project folder with a recipe in `.silo/recipes/demo.yaml` (a small web app on port 3000
and a background poller; the final run's file is `docs/v1/checks/final-proj/.silo/recipes/demo.yaml`):

```sh
silo up --recipe demo
```

```text
Making main from demo...
Starting main...
Checkpointing main...
main is running (made from demo). Warm checkpoint cb7140498337c47b5886ea3578002be6 saved. ssh main.silo reaches it.
```

Four forks, each running a command (here: write a note named after the fork's own host):

```sh
silo fork main -n 4 --name try -- bash -c 'echo kept-from-$(hostname) > ~/final-note.txt'
```

```text
try-1  exit 0  http://3000.try-1.silo.localhost:47383
try-2  exit 0  http://3000.try-2.silo.localhost:47383
try-3  exit 0  http://3000.try-3.silo.localhost:47383
try-4  exit 0  http://3000.try-4.silo.localhost:47383
4 forks of main; review with `silo diff <fork>`, keep with `silo fold <fork>`.
```

Look at what one of them changed:

```sh
silo diff try-1
```

```text
Comparing try-1 with when it was forked...
Other changed
  + /home/silo/final-note.txt
Ignored: 2 paths (caches, logs, build output)
credentials used: none
```

Keep it, check `main`, and change your mind:

```sh
silo fold try-1
silo exec main -- bash -c 'cat ~/final-note.txt'
silo undo main
silo exec main -- bash -c 'test ! -e ~/final-note.txt' && echo gone
```

```text
Keeping try-1...
Comparing try-1 with when it was forked...
Kept ea43ea10-2bc3-415c-89d8-db0c9b459e55 into fa3fb981-d9f4-4bfc-ba37-e373a46f3c52.
  files: 1 written, 0 already identical
Undo with `silo undo fa3fb981-d9f4-4bfc-ba37-e373a46f3c52`.
kept-from-try-1
Undoing the latest keep or promote of main...
Put fa3fb981-d9f4-4bfc-ba37-e373a46f3c52 back to Before keep of ea43ea10-2bc3-415c-89d8-db0c9b459e55. fa3fb981-d9f4-4bfc-ba37-e373a46f3c52 is running again, as it was.
gone
```

The long ids are the computers' ids (`main` and the fork). Throw the forks away:

```sh
for f in try-1 try-2 try-3 try-4; do silo stop $f; silo drop $f; done
```

The fork names are shortened here; the final run named them `e2e-final-demo-1` to `-4`, ran the
checks through a small wrapper around `silo exec`, and dropped with `--force`. The outputs are
copied from that run with only the names changed (`final-run.txt`, sections D1 to D6).

## Quiet forks

A fork is a copy of a computer that is already doing things: services, scheduled jobs, an agent
connected to a chat channel. Silo starts every fork quiet:

- a cold boot from the copied disk, with a new machine id and hostname;
- scheduled jobs off (cron, at, timed launchd jobs on a Mac); services that keep running, such as
  a database, stay on, because the fork should carry the dev stack;
- the programs a recipe lists under `quiet:` stopped, and the paths under `scrub:` deleted;
- no secret or GitHub access from the source unless you name it with `--secret`. GitHub access
  that is copied is read-only.

In the final run, `main` had a cron job every minute and the recipe's poller service. Its fork,
watched for 90 s after a 45 s settle, wrote no poll or cron line and ran no poller, while `main`
kept writing (`final-run.txt`, section Q1). `silo status` shows what was done:

```text
quiet: applied; stopped: final-poller, poller.py; scrubbed: /etc/silo/guest-client.json; secrets: none
```

## Review

`silo diff <fork>` compares the fork with the moment it was forked and sorts the changes into
buckets: Code, Packages, Settings & files, Database changed and Other changed. Caches, logs and build output are
counted as ignored. Credential paths, new listening ports, processes and services are listed under Not kept: a keep never brings them back.
The last line says which Silo-managed secrets the fork used, per host and method, never values.

`silo fold <fork> --request` records a review request instead of keeping; `silo requests` lists
them, and `silo requests show|keep|throw-away|open <id>` acts on one. A keep from a request is
pinned to what the request showed: if the fork changed since, the keep is refused.

## Keep and undo

`silo fold <fork>` keeps a fork's changes in the computer it came from. It saves a checkpoint
first, then replays packages by name and version, merges git, copies files, merges crontabs and
launch agents, and runs the recipe's `keep:` steps (`migrate`, `restart`, `after`). It stops
without changing anything if the fork and the target changed the same thing, or if a database
change has no migration step.

`silo undo main` puts `main` back to the checkpoint taken before the latest keep, running, as it
was. `auto_keep:` rules in `.silo/silo.yaml` can keep a fork without asking when every change
falls in buckets you list (for example only code, with the recipe's tests passing in the fork);
anything else becomes a review request.

## Make this main

When the fork is the computer you want, not just its changes:

```sh
silo fold <fork> --promote
silo undo main
```

The fork takes `main`'s name, published ports, URL, `ssh main.silo`, secrets, GitHub access,
hostname and scheduled jobs. The old `main` is kept, stopped, as `main-before-<time>`. Undo swaps
them back. Linux and macOS. In the final run a fork made with `--computer-use auto` was promoted
and the new `main` kept `main`'s own computer-use approval (`ask`), and the undo left no archive
behind (`final-run.txt`, sections P1 and P2).

## A URL per fork

Each computer's web ports open at their own local name:
`http://<port>.<computer>.silo.localhost:<proxy port>`, for example
`http://3000.try-2.silo.localhost:47383`. Four forks of an app on port 3000 open side by side,
and your own dev server on port 3000 keeps working. `silo fork -n` publishes the source's ports
and prints the URLs; `silo ports`, `silo publish` and `silo unpublish` manage them.

## Recipes

A recipe describes a clean computer in `.silo/recipes/<name>.yaml`:

```yaml
os: linux
packages:
  apt: [python3, cron, git]
setup:
  - mkdir -p /opt/app
services:
  web: python3 /opt/app/web.py
  poller: python3 /opt/app/poller.py
ports: [3000]
quiet: [poller]          # stopped in a fork
scrub: [/root/.env]      # deleted in a fork
keep:
  restart: [web]
  migrate: [make migrate]
```

`silo up --recipe <name>` builds it (cached), makes `main` from it, starts it, publishes its ports,
makes `ssh main.silo` work and saves a warm checkpoint. A fork of `main` starts from that
checkpoint when `main` has not changed since and the checkpoint is under a minute old; otherwise
it checkpoints `main` first. Changes made over a plain `ssh` session are not seen by that check.

Built in: `linux-dev`, `macos-xcode`, and two for agents that live in `main`:

- `openclaw` installs OpenClaw and runs its gateway as a service. Its `quiet:` and `scrub:` lists
  were checked in the final run: a fork had no gateway process and neither the `credentials/`
  folder nor `.env`, while `main` kept both (`final-run-part4-claw.txt`, section O3).
- `hermes` does the same for Hermes. Its process names and `.env` path are not checked yet.

Onboarding (model keys, chat logins) happens once in `main` over `ssh main.silo`.

## MCP setup

```sh
claude mcp add silo -- silo mcp
```

`silo mcp` serves the same operations as the command line over stdio: `up`, `list`, `status`,
`fork` (with `count`, `command`, `recipe`, `fromStep`), `diff`, `fold` (with `request` or
`promote`), `undo`, `drop`, `exec`, checkpoints, steps, the fork tree, review requests and the
computer-use tools. It returns the same JSON as `--json`. No tool sets or assigns secrets, and
`drop` refuses `main`. `.silo/silo.yaml` can set per-agent defaults under `agents:` (which
recipes and secrets a fork may use, whether anything is kept without asking).

## Computer use

An agent on your Mac can drive a fork's desktop:

```sh
silo fork main --gui --computer-use auto
```

`--gui` starts the fork and returns when its desktop and the computer-use driver (Cua Driver)
are ready. The MCP tools `computer_use_screenshot`, `_click`, `_type`, `_key`, `_hotkey`,
`_scroll`, `_move`, `_drag` and `_wait` then work on that fork; they refuse `main`.
`--computer-use auto` lets agents running inside the fork use computer use without a prompt; it
applies to forks only, and `main` keeps its own setting.

In the final run, an agent's tool calls took a screenshot of a Linux fork, opened a terminal,
typed a command and pressed Return; the file it wrote was then read from the Mac with `silo exec`
(`final-run.txt`, sections C1 and C2; screenshots in `docs/v1/evidence/final/raw/`). macOS forks
work the same way with `silo fork <mac> --gui`.

## Agents inside a computer

An agent that lives in a Linux `main` (OpenClaw, Hermes, or your own) can fork itself and ask
for review, from inside:

```sh
# on the Mac, once
silo guest-client install main --agent openclaw --binary ~/.silo/guest/silo
# inside main
silo --in-guest fork main --name try-1
silo --in-guest diff try-1
silo --in-guest fold try-1 --request
```

The token it gets is scoped to the forks it made. It cannot keep, promote, undo, drop or change
secrets; you see its requests with `silo requests` on the Mac. A fork does not inherit the token.

## Another Mac

With Connections on, `silo --device <name>` runs a command against another Mac's engine, for
example a Mac mini that holds your `main`:

```sh
silo --device mini fork main
silo requests --all-devices
```

`silo list --all-devices` and `silo requests --all-devices` give one list across devices; keep,
throw away and open act on the device the request is on. Commands that need this Mac's files,
SSH configuration or secrets (`up`, `exec`, `publish`, `secrets`) refuse `--device`.

## Install

Silo is a Mac app for Apple Silicon. It carries the engine, the VM runtime and the `silo`
command, and links the command into `~/.local/bin` each time the engine starts (put
`~/.local/bin` on your `PATH`). The Linux guest image downloads once before the first computer.

> **Owner:** download link and signing status to fill in at release (the next release is not
> notarized yet; see `docs/v1/release/README.md`).

To build from source, see [the source-build guide](docs/SiloUI-BUILD-FROM-SOURCE.md); the
command alone builds with `cargo build --release -p silo-cli`.

## Numbers

Measured once, on one Mac (Apple M4 Max, 64 GiB RAM, macOS 26.5), on a Dev-channel build of the
same commit in the release profile with the release runtime binaries, not the signed production
app. Each time is the median of N=3 runs, with min–max. Source: `docs/v1/evidence/final/final-run.txt`
(summary in `docs/v1/evidence/final/README.md`).

| What | The clock stops when | Median | Min–max |
|---|---|---|---|
| Linux fork of a running `main`, no usable warm checkpoint | `silo fork` exits with the fork running and a command answered in it | 8.87 s | 8.62–10.75 s |
| `silo fork main -n 4` from the warm checkpoint | all four forks running and a command answered in each | 15.57 s | 15.53–17.67 s |
| macOS fork to ready | `silo fork --gui` returns with the desktop and computer use ready | 51.69 s | 51.68–57.84 s |

Memory of one Linux fork (its VM processes, measured with `footprint` 20 s after ready): about
500 MiB, 499.0–507.1 MiB over 5 forks. Method, caveats and what was not measured:
[numbers post draft](docs/v1/launch/numbers-post.md).

## Not there yet

- **The Silo app** has no screens yet for review, keep, Make this main or pending requests; use
  `silo` or MCP.
- **Hosts:** Apple Silicon Macs only. A Linux host is the next milestone; Intel Macs are not planned.
- **macOS forks:** Apple allows two macOS VMs at once, so `silo fork -n` on a Mac is limited to
  the free slots; fork a Linux computer for more.
- **Hermes recipe:** its `quiet:` and `scrub:` entries are not checked against a running install.
- **Secrets in forks:** copying a named secret into a fork (`--secret`) and the credentials-used
  line have tests but no live run yet; the live check waits on a Keychain repair for the Dev
  engine.
- **Hosted review** (`silo review`) works against a reviewer you run; no hosted service is
  deployed.
- **GitHub Action** (`silo-action/`) is in this repository and not published.
- **Changes over plain `ssh`** are not seen by the warm-checkpoint check; a fork may start from a
  checkpoint that misses them. Use `silo exec`, or fork after the checkpoint ages out.
- **Outbound network:** there is no allowlist of hosts a computer may reach.

## License

MIT. Built on MicroSandbox (Linux VMs), Apple's Virtualization framework (macOS VMs) and Cua
Driver (computer use).
