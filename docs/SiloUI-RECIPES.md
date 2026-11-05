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
ports: [8080]          # published for each fork
quiet: [poller]        # fork rules (see SiloUI-CLI.md); not part of the build
scrub: [/root/.env]
keep:
  after: [make test]
```

Package names are single words (a version suffix such as `name@1.2` or `name==1.0` is
fine); they are passed to the package manager as arguments, never as shell text. `setup`
commands are shell text. Services start at boot through an XDG autostart entry on Linux and
a LaunchAgent on macOS.

Built-in recipes, minimal on purpose:

| Name | What it is |
| --- | --- |
| `linux-dev` | Linux with `build-essential`, `jq` and `ripgrep` |
| `macos-xcode` | macOS that must already have Xcode: the build runs `xcode-select -p` and `xcodebuild -version`, and fails if they do not |

## Build and cache

`silo recipe build <name>` (or the first `silo fork --recipe <name>` or `silo up`) asks the
engine (`recipes.build`) to create a computer named `silo-build-<name>`, run the packages,
the setup commands and the services, check that each package manager lists its packages and
each service entry exists, save a checkpoint, and stop it. A failed step drops the builder
computer and leaves no cache entry; a rebuild that fails keeps the previous build.

The cache key is a hash of the normalized recipe plus the guest image version. Normalizing
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
(default `linux-dev`) when it does not exist, starts it if it is stopped, and saves a
checkpoint of kind `warm`. Each `up` replaces the previous warm checkpoint (one that a fork
still depends on stays until a later `up`); retention never removes it.

A fork of `main` with no `--checkpoint` starts from the warm checkpoint when `main` runs and
nothing outside the system's log, cache and temporary folders has been written since: `up`
leaves a marker file in `main` and the fork checks for files newer than it. When in doubt it
takes a new checkpoint as before.

## Fork baselines

A fork's idle baseline (`silo diff --baseline`) is cached under the recipe's hash when the
fork came from a recipe, so every fork of one recipe shares a baseline and rebuilding the
recipe's checkpoint does not discard it.
