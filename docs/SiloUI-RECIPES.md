# Recipes and `silo up`

A recipe is a text spec for a clean computer. Silo builds it once into a hidden computer
with a checkpoint, then forks from that checkpoint, so a fork from a recipe starts from a
known state without repeating the install. Recipes are text and can be committed; the
built computer stays on your Mac.

## Files

`.silo/recipe.yaml` is the project's recipe (it also holds the fork and keep settings).
Named recipes live in `.silo/recipes/<name>.yaml`; a project file with the name of a
built-in recipe replaces it. Unknown keys are errors that name the file and line.

```yaml
os: linux              # linux (default) or macos
cpus: 4                # optional; defaults: Linux 2 / 4 GiB / 10 GiB, macOS 4 / 8 GiB / 64 GiB
memory: 8              # GiB
disk: 20               # GiB
packages:              # installed in this order: apt, brew, pip, pipx, npm, cargo
  apt: [build-essential, jq]     # Linux only
  brew: [jq]                     # macOS only
  pip: [httpie]
  pipx: [ruff]
  npm: [typescript]
  cargo: [ripgrep]
setup:                 # run once, in order: as root on Linux, as the account on macOS
  - git clone https://example.com/app /opt/app
services:              # started at boot
  web: python3 -m http.server 8080
ports: [8080]          # published (as websites) on main by `silo up` and on each recipe fork
desktop: false         # optional; Linux only. No desktop in this computer or its forks (default: true)
quiet: [poller]        # fork rules (see SiloUI-CLI.md); not part of the build
scrub: [/root/.env]
keep:
  after: [make test]
test: make test        # run in a fork by auto_keep rules with tests: pass
files: [/etc/app.conf] # settings files kept into the computer (silo fold appends them)
```

Package names are single words (a version suffix such as `name@1.2` or `name==1.0` is
fine); they are passed to the package manager as arguments, never as shell text. `setup`
commands are shell text. Services start at boot through an XDG autostart entry on Linux
(`/etc/xdg/autostart/<name>.desktop`) and a LaunchAgent labelled `<name>` on macOS. Each runs a
launcher file named `<name>`; for a plain command (a program and arguments, no shell syntax)
the launcher `exec`s it, so the process runs under its own name. A `quiet:` entry that names
the service stops its entry or label and also the name the command runs under: the script after
an interpreter, by its full file name, when it names a file with an extension (`python3
/opt/app/poller.py` is `poller.py`; `node server` and `python3 -m app` name none), else the
program (`openclaw gateway` is `openclaw`). A launcher (`env`, `sudo`, `nohup`, `nice`, `exec`,
`timeout`, `java`, `npm`, `npx`, `yarn`, `pnpm`, `uv`, `docker`, `make`, `bun`, `deno`, `bash`,
`sh`) names none either. When no name can be derived, `silo fork` warns: add the program or
script to `quiet:` yourself. The fork's `silo status` lists both names.

On macOS, a recipe that needs Homebrew (brew packages, or pip, pipx, npm or cargo when their
tool is missing) installs it first as the account with the official non-interactive
installer.

Built-in recipes, minimal on purpose:

| Name | What it is |
| --- | --- |
| `linux-dev` | Linux with `build-essential`, `jq` and `ripgrep` |
| `openclaw`, `hermes` | Linux with the agent installed and its gateway started as a service; `quiet:` and `scrub:` set for a fork ([resident agents](SiloUI-CLI.md#resident-agents)) |
| `macos-xcode` | macOS that must already have Xcode: the build runs `xcode-select -p` and `xcodebuild -version`, and fails if they do not |

## Build and cache

`silo recipe build <name>` (or the first `silo fork --recipe <name>` or `silo up`) asks the
engine (`recipes.build`) to create a computer named `silo-build-<name>-<8 hex>`, run the packages,
the setup commands and the services, check that each package manager lists its packages and
each service entry exists, save a checkpoint, and stop it. A failed step drops the builder
computer and leaves no cache entry. Builds of one recipe run one at a time in the engine (a
second waits and then finds the first build in the cache), each with its own builder name. A
rebuild keeps the previous builder and cache entry until the new build succeeds, and a failed
rebuild keeps them; builders an interrupted build left behind are dropped by the next build
of the same recipe.

The cache key is a hash of the normalized recipe plus the image version (the guest image on
Linux; the macOS build of the newest template and the setup fingerprint on macOS), so a new
image gives a new cache entry and a new baseline key. Normalizing
ignores package order, repeated packages, repeated ports, service order, surrounding
whitespace and resources written at their default; setup order stays, because it is the run
order. Changing the recipe or the image version builds again; `--force` rebuilds anyway.
The builder is left out of `silo list`. On macOS the two-VM limit applies: a macOS build
needs a free slot.

| Command | What it does |
| --- | --- |
| `silo recipe ls` | built-in and project recipes, and which are built |
| `silo recipe build <name> [--force]` | build now |
| `silo recipe rm <name>` | remove the build and its builder computer |
| `silo fork <name> --recipe <r>` | fork named `<name>` from the cached build of `<r>`; the recipe's `quiet:` and `scrub:` apply, and `recipes:` in `silo.yaml` limits which `<r>` are allowed |

The engine methods are `recipes.build` (a job), `recipes.list` and `recipes.remove`; the
MCP tools `recipe_list`, `up` and `fork` (with `recipe`) use the same calls.

## `silo up`

`silo up [--recipe <r>]` checks that the engine answers, makes `main` from the recipe
(default `linux-dev`) when it does not exist, starts it if it is stopped, publishes the
recipe's ports on it and, with `ssh`, makes `ssh main.silo` work. It saves no checkpoint. A
warm checkpoint that an earlier version saved is removed by the next `up` (one that a fork still
depends on stays until a later `up`).

A fork starts from the computer as it is when the fork is made. A fork with no `--checkpoint`
takes a "Fork point" of the source first: on Linux its disks only, flushed from the running
guest while it is briefly paused, never its memory, because a fork always cold-boots from the
disks. On macOS a fork point is a disk copy too. `silo fork main -n N` takes one such capture and
all N forks start from it. Name a checkpoint (`--checkpoint`) to fork from an earlier state
instead.

The fork reports which checkpoint it used, for example
`Forked try (stopped): main as of 14:02:31 (new checkpoint)`; `--json` carries `checkpoint`
(`id`, `kind`, `createdAt`, `ageSeconds`, `reused`) and `forkedAsOf`. The MCP `fork` tool
behaves the same.

## The desktop

A Linux computer has a desktop by default (the guest image ships it, and computer use needs
it). `desktop: false` in a recipe builds a computer that starts none, and every fork of it
starts none either: the choice is part of the computer, and forks inherit it. It saves the
desktop's memory and boot time in each fork. A Linux recipe with `desktop: false` cannot also have
`services:`, because Linux services start with the desktop session. A macOS recipe cannot say
`desktop: false`: a macOS computer is its own display and always has one, so computer use there
is unaffected.

`silo fork --gui`, the computer-use tools (CLI and MCP) and `silo status` say
`<name> has no desktop; turn it on with silo desktop on <name>` for a computer whose desktop is
off. Turn the desktop on or off later with `silo desktop on|off <computer>` on a running
computer (the app's **Desktop** switch in the computer's settings does the same). `on` also
starts the desktop now; `off` stops it. Forks made afterwards inherit the new setting.

## Fork baselines

A fork's idle baseline (`silo diff --baseline`) is cached under the recipe's hash when the
fork came from a recipe, so every fork of one recipe shares a baseline and rebuilding the
recipe's checkpoint does not discard it.

For a fork from a recipe, `silo diff --baseline` measures a missing baseline on a throwaway
fork of the recipe's cached checkpoint.
