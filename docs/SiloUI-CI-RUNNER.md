# CI runner: Silo as the machine GitHub Actions jobs run on

`silo-runner` (Go, `app/SiloUI/runner`) makes a Mac running Silo, or a [Linux host](#linux-hosts)
running `silod`, the machine that runs the jobs of a private GitHub repository or organization. Every job gets a fresh Silo computer forked from a
recipe; the computer is deleted when the job succeeds and stopped and kept when it fails (the last
`keepFailed.count` of them, 3 on Linux and 1 on a Mac by default).

GitHub's runner program runs inside the computer. `silo-runner` owns everything GitHub-facing (the
runner scale set protocol, job messages, runner registration) and drives Silo only by running the
`silo` command line with `--json`. The engine never learns GitHub's protocol.

## What it is built on

| Need | Choice | Why |
| --- | --- | --- |
| Scale set protocol, listener, JIT configuration | [`github.com/actions/scaleset`](https://github.com/actions/scaleset) v0.5.0 (GitHub's own Go client) | Supported by GitHub; its `listener.Run` handles sessions, polling and acknowledgement |
| Runner program | GitHub's [`actions/runner`](https://github.com/actions/runner), pinned by version and SHA-256 in each recipe | The only supported way to run workflow jobs |
| Ideas | [`ysya/runscaler`](https://github.com/ysya/runscaler) (MIT): readiness on "Listening for Jobs", capacity before the JIT config, deregister before delete, orphan reconcile, disk guard, two-VM macOS limit | Borrowed ideas only; no dependency |
| Driving Silo | the `silo` command line | It already handles the engine protocol, job polling and `--device` routing |

Gap: `scaleset` v0.5.0's `go.mod` asks for Go 1.27.3, so `runner/go.mod` pins its own Go version, 1.27.3;
the repository's other Go code uses 1.25.

The Linux host support adds no dependency: the service is a systemd user unit written by the helper
itself, the way `silod --service register` writes the engine's.

## One label per recipe

GitHub gives a job to any idle runner whose labels match, and sends no branch, pull request or SHA
beforehand. So a scale set is one label and one recipe: `runs-on: silo-linux` means a computer made
from the `silo-linux` recipe. Put different toolchains in different recipes and give each a label.

## Set up

1. **Silo on the Mac**, the engine running, `silo` on `PATH`.
2. **A private repository** (or an organization whose runner group does not allow public
   repositories).
3. **Credentials**, either:
   - a **GitHub App** (preferred) installed on the repository with *Administration: read and write*
     (repository) or *Self-hosted runners: read and write* (organization), plus *Metadata: read*. Note
     the client id and the installation id, and save the private key as a `0600` file; or
   - a **personal access token** with the same permissions, exported in an environment variable whose
     name you put in the configuration (`tokenEnv`). The token is never written in the file.
4. **Configuration** (YAML or JSON, unknown keys are errors): start from
   [`examples/config.yaml`](../app/SiloUI/runner/examples/config.yaml).
5. **Recipes**: `silo-runner install-recipes -config config.yaml` writes the shipped `silo-linux` and
   `silo-macos` recipes ([`runner/recipes/`](../app/SiloUI/runner/recipes/)) into
   `<projectDir>/.silo/recipes/` without replacing one that is already there. Add the tools your jobs
   need to your copy (see [recipes](SiloUI-RECIPES.md)). The helper runs `silo` in `projectDir`, so
   project recipes are found there.
6. **Check, then run** (build with `go build -o silo-runner .` in `app/SiloUI/runner`):

```sh
silo-runner check -config config.yaml    # validates, and refuses a public repository
silo-runner run   -config config.yaml
```

### Configuration

| Key | Meaning | Default |
| --- | --- | --- |
| `github.configUrl` | `https://github.com/OWNER/REPO` or `https://github.com/ORG` | required |
| `github.runnerGroup` | runner group of the scale sets | `default` |
| `github.auth.app` | `clientId`, `installationId`, `privateKeyFile` | |
| `github.auth.tokenEnv` | environment variable holding a personal access token | |
| `silo.path` / `projectDir` / `socket` / `device` | the `silo` command, where its recipes are, `--socket`, `--device` | `silo`, `~/.silo-runner/project` |
| `stateDir` | lock file and the record of the computers the helper made (`computers.json`) | `~/.silo-runner/state` |
| `limits.minFreeDiskGiB` / `diskPath` | no new computer below this much free disk | 20 GiB, home |
| `limits.readyTimeout` / `startAttempts` | wait for "Listening for Jobs"; computers tried before backing off | 3m, 3 |
| `limits.idleTimeout` | an idle runner no job needs is removed after this | 10m |
| `limits.listenerStall` | the watchdog restarts a session with no poll for this long | 5m |
| `linuxBudget` | `memoryGiB`, `count`: Linux computers on this host. Each computer counts with the `memory:` of its recipe file (the engine's default, 4 GiB on Linux and 8 on macOS, when the file has none) | on a Mac, none (0 or left out: no bound beyond `maxRunners`). On a [Linux host](#linux-hosts), a field left out comes from the host: three quarters of its memory, one computer per CPU |
| `runnerRefresh` | `checkEvery` (24h), `afterDays` (3), `disabled` | |
| `scaleSets[]` | `label`, `recipe`, `os` (`linux`/`macos`), `maxRunners`, `keepFailed: {count, days}` | `maxRunners` 1, `keepFailed.count` 3 (Linux) or 1 (macOS), `keepFailed.days` 7 |

## What happens to a job

1. GitHub reports an available job; the helper acquires it and, if capacity allows, starts a runner
   for it. Jobs are tracked by runner request id, so a redelivered message starts nothing twice.
2. **Capacity first.** Every decision to start a computer begins with a fresh `silo list --all --json`.
   Capacity is a function of that list and of the computers the helper has asked for that the list does
   not show running yet: every record that is forking, or live and not shown running, plus the recipe
   rebuilds that are running. On a Mac every running,
   starting or stopping macOS VM counts, whatever started it (a recipe build, your own computer,
   another runner): Apple allows two. Linux uses `linuxBudget` and the memory in each recipe file. The
   free-disk guard comes next. Nothing is forked when either says no; the next poll or tick asks again.
3. The helper writes the computer's record (`ci-<label>-<id>`, phase `forking`) and then runs
   `silo fork ci-<label>-<id> --recipe <recipe>`. The name is new every time and is never taken from a
   computer in the list or in the record. The fork answers with the computer's id; from then on every
   call (`silo start`, `silo exec`, `silo stop`, `silo drop`) names the computer by that id.
4. `silo start <id>`, then a one-time JIT configuration from GitHub for a runner **named exactly as the
   computer**, so a job's page names the computer it ran on. The runner is started with
   `silo exec <id> --stream --stdin`. The JIT configuration is the command's standard input: it
   travels to the engine in the request and is in no command line on the Mac or in the computer. The
   recipe's `silo-run-jit` script reads it into `ACTIONS_RUNNER_INPUT_JITCONFIG` and starts GitHub's
   `run.sh`.
5. The computer counts as ready only after the runner prints `Listening for Jobs`. If it does not
   within `readyTimeout`, or the runner ends first, the computer is finished with the reason "the
   runner did not start": the runner is deregistered at GitHub, the computer deleted and a new one
   tried, up to `startAttempts`; then the helper waits longer before trying again (1 minute, doubling
   up to 16 minutes, reset by the first runner that comes up).
6. When the job completes, **the runner is deregistered first**: the helper looks the runner up by name
   at GitHub (no runner there counts as deregistered) and removes it. A runner that GitHub says still
   has a job (`JobStillRunningError`) is left alone and its computer keeps running. Once the runner is
   gone, the computer is deleted if the job succeeded (or was cancelled or skipped), and **stopped and
   kept** otherwise, under the runner's name. No checkpoint is taken of a computer a job ran in. A
   computer is never deleted before its runner is deregistered or GitHub says it is gone.
7. A delete is only believed once a fresh `silo list` no longer shows the computer's id. `silo drop`
   does not stop a running computer and the engine will not delete one, so the helper stops it first.

### Keep on failure

`keepFailed.count` and `keepFailed.days` bound the kept computers per label: the oldest are deleted
when there are more than `count`, and any older than `days`. `count` is 3 on Linux and 1 on a Mac
unless you set it (a stopped computer holds no virtual machine slot, so the Mac's limit of two running
computers is not the reason; its disk is, and a Mac computer's is large). **`count: 0` keeps nothing**:
a failed job's computer is deleted like a successful one's. Only a computer that is already kept (its
runner deregistered and the computer stopped) is ever counted or deleted by these limits, never one
whose clean-up is still in progress. Kept computers are cleaned up only while `silo-runner` runs; stop
it and they stay until it runs again. Inspect one with `silo start <name>` and `silo exec`, then
`silo drop <name>`. A kept computer can hold the job's secrets
([threat model](THREAT-MODEL.md#ci-runner-silo-runner)).

### How the helper keeps track

- **One record per computer, in `<stateDir>/computers.json`.** It is written by name before
  `silo fork`, gets the engine's computer id when the fork answers, and is removed only when a fresh,
  successful `silo list` no longer shows that id. Each record has a phase: `forking` (name only),
  `live` (id known; whether its runner is starting, idle or busy is kept in memory and found again
  from GitHub after a restart), `finishing` (the job ended, the start failed, the runner exited or
  sat idle; the reason or the job's result is recorded) and `kept` (deregistered and stopped). Every
  write flushes the file and then its directory to disk before it counts.
- **Ours means this id.** The helper acts on a computer only if its id is in a record (or, for a
  `forking` record left by a run that stopped, if a computer has exactly the recorded name). A
  computer with a `ci-*` name that is not in the record, a computer renamed since, and one that took
  the name of a computer that is gone are never touched. A computer is "gone" only when a fresh `silo list`
  that succeeded lacks its id; a failed list, a failed delete or a "no such computer" answer change
  nothing.
- **One loop.** A single goroutine receives every event (a scale set message, a runner command
  ending, the result of a fork, start, deregistration, stop, delete or list, a 30 second tick,
  shutdown) and is the only thing that changes records or the file. Slow calls run beside it and come
  back as events, at most one per computer. After every event the helper looks at every record and at
  the latest list and starts the next step of whatever has none in flight. A step that fails is
  retried by the first tick after 30 seconds: a GitHub outage or a failed delete never strands
  capacity until a restart. Starting up is not a special case; it is the same loop reading the same file.
- **Starting up**, under a lock file (a second helper on the same state directory is refused). A
  `live` record with no job request and no job result recorded has no runner command to
  watch, so the helper deregisters the runner: if GitHub refuses because the runner has a job, the
  computer is adopted as busy and keeps running; otherwise the computer is deleted as an orphan. A
  `live` record whose runner had a job is adopted as busy at once (next item). A `forking` record
  is bound to the computer with exactly that name and deleted, or forgotten if the first list shows
  no computer with the name. `finishing` records restart at deregistering the runner, which is safe to repeat (a runner already
  gone counts as removed); `kept` ones stay stopped. A computer is never deleted when GitHub cannot be asked.
- **A job in flight across a restart is kept, with its result unknown.** A runner that had a job when
  the helper stopped is adopted as busy and looked up at GitHub every 30 seconds; when GitHub no
  longer has its runner, the job has ended without the helper seeing how, so the computer is stopped
  and kept with the result `unknown`, unless the job's own result reaches the helper first (it then
  decides). This holds when the job ended while the helper was down. The same happens to a runner
  whose command ended without a result, after two minutes of waiting for the result to arrive.
- **A result that arrives while a computer is finishing counts**, until the computer is stopped or
  deleted: a job result replaces one of the helper's own reasons (idle, did not start, unknown), and a
  keeping result is not replaced by a success. If GitHub still has the runner when the result
  arrives, the computer keeps running with that result and is cleaned up with it once GitHub
  releases the runner: a success is deleted and a failure is kept as a failure, never as `unknown`.
- **A fork that fails is settled by the next list, unless the name was taken.** If the engine
  refuses the fork because a computer already has the name (`Computer names must be unique.`), it
  made nothing: the record is forgotten at once and no computer is deleted. For any other error the
  engine may have made the computer (a timeout, a killed command, unreadable output), so the record
  stays until a `silo list` that started after the error answers. A computer with exactly the
  recorded name is then deleted by its id, and if no computer has the name the record is forgotten.
  The name is `ci-<label>-<random>`, chosen by the helper and not in the list or the records, so a
  computer with it is the one the fork made.
  What remains: if the helper itself is killed while a fork is running and the engine finishes making the
  computer only after the first list of the next start, the record is forgotten and the computer stays in
  `silo list` as `ci-...`; it is yours to delete.
- **Capacity counts what the list may not show.** Every computer the helper has forking or live that
  the latest list does not show running counts as running, and so does a recipe rebuild while it
  runs, because the list can be stale and a computer started a moment ago may not appear yet. The
  exception is a live record whose computer a list started after its fork or start shows stopped,
  and that is not starting: after a Mac restarts, a busy record adopted from the file with an
  offline runner holds no slot. A
  rebuild's own virtual machine, once the list shows it, is counted twice; that errs towards
  starting fewer computers.
- **Watchdog**: if no poll returns within `limits.listenerStall`, the session is closed and a new one
  opened.
- **Shutdown** (Ctrl-C or SIGTERM) deregisters and deletes idle runners and computers still being
  started, and waits for the work in flight; computers with a job keep running and are picked up by
  the next start.

## Private repositories only

`silo-runner` refuses to start (and `check` fails) when the repository is public, when an organization's
runner group has `allows_public_repositories` on, or when it cannot tell. GitHub's own guidance is that
self-hosted runners "should almost never be used for public repositories"
([secure use](https://docs.github.com/en/actions/reference/security/secure-use)): a pull request from
anyone can run a workflow on your Mac. A VM limits the damage; it does not make this safe.

## GitHub's secrets are not protected by Silo's placeholders

GitHub delivers its own secrets and `GITHUB_TOKEN` **straight into the machine** the job runs on. Silo's
placeholder secrets, which keep a real value out of a computer, do not apply to them: the runner program
has the value, and so does the job. Only secrets that **Silo manages** (`silo secrets`, assigned to the
computer) can be placeholders. Give a job only secrets you would give a build server, and keep the
repository private.

## Linux hosts

A Linux machine that runs `silod` ([Linux host](SiloUI-ENGINE.md#silod-on-a-linux-host), x86_64 or
aarch64) can be the runner machine. Its computers are Linux computers over KVM, so only Linux scale
sets run there (a `macos` scale set is refused). **One helper per machine, one label per machine and
recipe**: two machines that should both serve `runs-on: silo-linux` need two labels (for example
`silo-linux-a` and `silo-linux-b`), because GitHub picks any idle runner of a label and each helper
owns the computers of its own machine. One helper for several machines is later work.

### Install

`silo-runner` is in the Linux engine archive, beside `silod` and `silo`
(`silo-engine-<version>-linux-x64.tar.gz` or `-linux-arm64.tar.gz`; see
[Engine releases](SiloUI-RELEASES.md#engine-releases)). Install the engine as the
[engine guide](SiloUI-ENGINE.md#silod-on-a-linux-host) says (`silo host init`), as a dedicated
account that is not root and is in the `kvm` group. Then, as the same account:

```sh
mkdir -p ~/.config/silo-runner && chmod 700 ~/.config/silo-runner
```

Write `~/.config/silo-runner/config.yaml`, starting from
[`examples/config-linux.yaml`](../app/SiloUI/runner/examples/config-linux.yaml), and keep the GitHub
credential in files only this account can read:

```sh
R=~/.local/opt/silo-engine/silo-runner
$R install-recipes -config ~/.config/silo-runner/config.yaml     # writes silo-linux into projectDir
chmod 600 ~/.config/silo-runner/app.private-key.pem              # a GitHub App key, or instead:
printf 'SILO_RUNNER_TOKEN=%s\n' "$TOKEN" > ~/.config/silo-runner/env && chmod 600 ~/.config/silo-runner/env
$R check -offline -config ~/.config/silo-runner/config.yaml     # this machine only: /dev/kvm, the budget
$R check -config ~/.config/silo-runner/config.yaml              # and GitHub: refuses a public repository
$R service register -config ~/.config/silo-runner/config.yaml
```

The `silo-linux` recipe picks the runner for the computer's architecture while it builds
(`uname -m`: `arm64` on an aarch64 host or a Mac, `x64` on an x86_64 host) and checks the download
against that architecture's SHA-256; both are pinned in the recipe
(`RUNNER_SHA256_ARM64`, `RUNNER_SHA256_X64`), and the version refresh moves both from the same
release's published digests (a release missing either digest changes nothing). The first job builds
the recipe, which takes several minutes.

### Service

`silo-runner service register` writes `~/.config/systemd/user/silo-runner.service`
(`$XDG_CONFIG_HOME` is honored), then runs `systemctl --user daemon-reload`, `enable` and `restart`
on it. It follows `silod --service register`, and like it never replaces a unit file it did not
write (its first line marks it) and never asks for privileges. The unit:

- is a **user** unit, so it runs as the account whose user manager runs it: the account of the
  engine's socket (the engine accepts only clients of its own account). It has no `User=`, and
  `silo-runner` refuses to run as root, with or without systemd;
- has `After=` and `Wants=` the engine's unit (`org.silo.preview.engine.service`; pass
  `-engine-unit org.silo.dev.engine.service` for a Silo Dev engine), so the helper starts after the
  engine and starting it starts a stopped engine. Stopping or restarting the engine does not stop
  the helper; it retries until the engine answers;
- runs `silo-runner run -config <file>` with the canonical paths, `Restart=on-failure`,
  `RestartSec=10` and no start rate limit, `UMask=0077`, `KillMode=mixed` and `TimeoutStopSec=120`:
  a stop sends SIGTERM to the helper alone, which then deregisters idle runners and waits for the
  work in flight (the [shutdown](#how-the-helper-keeps-track) rules); whatever is left after the
  timeout is killed. Computers with a job keep running and are picked up by the next start;
- reads `~/.config/silo-runner/env` if it exists (`EnvironmentFile=-%E/silo-runner/env`), the place
  for `SILO_RUNNER_TOKEN=` when `github.auth.tokenEnv` is used. A GitHub App needs no environment.

`silo-runner service unregister` runs `disable --now` and removes the file. As for the engine, turn
lingering on so the services start at boot and survive the last logout (an administrator runs
`sudo loginctl enable-linger "$(id -un)"`; `register` prints this when lingering is off).

**Why a unit written by the helper and not a documented unit file.** The unit needs the canonical
paths of the binary and the configuration, quoted for systemd, the engine's unit name for the
channel, and a refusal to replace someone else's file; a copied file would be edited wrongly
(a stale path after an upgrade, `%` and `$` in a path). The code is about 150 lines, mirrors
`silod --service register`, and has unit tests for the text and the systemctl calls. After an
upgrade that installs a new engine directory, run the new `silo-runner service register` (it
rewrites the unit and restarts the helper), as for `silod`.

### Logs

The helper logs to standard error, which the user journal keeps:

```sh
journalctl --user -u silo-runner.service -f
journalctl --user -u org.silo.preview.engine.service        # the engine's
```

`silo-runner run -debug` is not set by the unit; add `-debug` to a copy of `ExecStart` through
`systemctl --user edit silo-runner.service` when you need it. The journal is readable by the
host's administrators ([threat model](THREAT-MODEL.md#ci-runner-silo-runner)); the helper logs
computer names, job and runner ids, GitHub error messages and recipe versions, never the GitHub
credential or a JIT configuration.

### Capacity and /dev/kvm

At start (and in `check`, with `-offline` for this part alone) the helper on a Linux host:

- requires `/dev/kvm` to open for reading and writing as this account, and otherwise stops with
  what to do (`/dev/kvm does not exist`, or add the account to the `kvm` group);
- gives each `linuxBudget` field you left out the host's value: **memory is three quarters of the
  machine's total memory** (`MemTotal` in `/proc/meminfo`; the rest is the host's own, the page cache
  and each virtual machine's overhead) and **count is the number of CPUs**. It uses the machine's
  size and not what is free right now, so a restart with computers running computes the same budget.
  The log line says which fields came from the host. Set `linuxBudget` to choose your own;
- refuses a budget smaller than one computer of your largest recipe (the recipe's `memory:`, 8 GiB for
  the shipped `silo-linux`), because no job could ever start. Give the recipe less memory, or the
  host more;
- refuses a `macos` scale set.

Why the default is derived and not required: an unbounded Linux host lets a burst of jobs use all of
its memory, and leaving the number to the owner means the safe setting depends on remembering to
write it. With `silo.device` set, the helper drives an engine on another machine, and none of these
host checks apply.

### What this phase did not run

The jobs on a Linux host have not run against GitHub: registering runners needs an admin token and
the owner's Linux machine was offline. The archive, the unit, `/dev/kvm`, the budget and the journal
are tested on a KVM runner (`scripts/runner-host-e2e.sh`, in the Linux host workflow, with no
GitHub). The end-to-end check against a private repository is
[`forks-ci/checks/r2_linux_runner_check.sh`](forks-ci/checks/r2_linux_runner_check.sh).

## Runner version

Recipes install GitHub's runner with a pinned version and SHA-256, and the scale sets turn self-update
off (`RunnerSetting.DisableUpdate`). GitHub requires such a runner to be updated within 30 days of a
new release ([self-hosted runners](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)).
At startup and every `runnerRefresh.checkEvery`, the helper compares each recipe's `RUNNER_VERSION`
with the latest `actions/runner` release. Once the release is `afterDays` old it rewrites the pin (the
version and the release's published SHA-256) in `<projectDir>/.silo/recipes/<recipe>.yaml` and runs
`silo recipe build`. New computers wait during the rebuild; a failed rebuild restores the old recipe and
logs the date GitHub will stop the runner. If you edit a recipe's download, keep the `RUNNER_VERSION=`,
`RUNNER_SHA256=` and `actions-runner-<os>-<arch>-${RUNNER_VERSION}.tar.gz` lines the helper reads.

## Example

[`examples/ci.yml`](../app/SiloUI/runner/examples/ci.yml) is a plain job on `runs-on: silo-linux`
(checkout, build, test). If it fails, its computer is kept under the runner name shown in the job's
"Set up job" step; inspect it with `silo start <name>` and `silo exec <name> -- <command>`.

## Not in this phase

Outbound rules per label; per-job network logs; one helper for several machines; the macOS archive
does not carry `silo-runner` (a Mac builds it with `go build`). The live check against a private test repository is
[`forks-ci/checks/r1_runner_check.sh`](forks-ci/checks/r1_runner_check.sh) (Mac, or Linux with `R1_OS=linux`
against a Dev engine) and, for a Linux host, [`forks-ci/checks/r2_linux_runner_check.sh`](forks-ci/checks/r2_linux_runner_check.sh).
