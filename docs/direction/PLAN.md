# Direction build: plan

This plan turns [the product direction](../DIRECTION.md) into work. State, decisions and what is
left are in the [build log](BUILD_LOG.md); the rules every implementing agent follows are in
[AGENT-BRIEF.md](AGENT-BRIEF.md). Written 2026-11-10 from a read of the code (five area maps,
summarised under [Facts](#facts-the-plan-rests-on)).

## Outcome

When this is done, on the Dev build:

1. A new computer has Silo's tools inside it. An agent there runs `silo` or its MCP server and can
   list, create, fork, run commands in, diff, checkpoint, stop and drop the computers it made.
2. Anything that widens what it has (merge into its parent, a repository, a secret, a wider
   network, a port on the device) becomes a request in the Silo app. You allow or deny, and the
   agent's call gets the answer.
3. Every call that changes something is in an audit log with who made it.
4. `silo merge` replaces fold, keep and promote. A conflict asks for a choice instead of refusing.
5. The `.silo` project folder is gone. A recipe is stored by the engine and belongs to a computer.
6. A computer has a network policy: open, a list of allowed hosts, or closed.
7. macOS computers have the same page in the app as Linux ones, and no longer boot on Apple's
   network once an agent may have written their disk.
8. A personal GitHub token is a secret you assign per computer; several can exist.
9. Onboarding makes `main`, installs the agent you pick and opens it.

## Design

### 1. Who is calling: `Caller`

Every request the engine handles carries a `Caller`, set by the transport and never by the client:

| Caller | Set where | Authority |
| --- | --- | --- |
| `Owner { client }` | the local socket (`silod/src/dispatcher.rs` `prepare`) | everything, as today |
| `Device { key }` | the Connections bridge (`serve_owner`, with the key fingerprint threaded through `access_dispatch`) | the bridge's method lists, as today |
| `Computer { id }` | the channel inside a computer (section 3) | the policy below |

`Caller` is a field of `RequestContext`, which every handler already receives through
`HandlerContext`, so handler signatures do not change.

### 2. Policy, in one place

`engine/src/policy/` holds a function from method to rule, written as an exhaustive `match` on
`MethodName`, so a new method does not compile until it is classified. `methods::dispatch`
(`silod/src/methods/mod.rs`) is the single place that asks it. It replaces
`guest_client::ALLOWED` and `authorize_call`.

Rules for a `Computer` caller:

| Rule | Meaning | Examples |
| --- | --- | --- |
| `Open` | always | handshake, `device.resources`, job reads for its own jobs |
| `Tree` | the target is the caller or a computer it created, directly or through one it created; list results are cut to that set | status, tree, diff, steps, checkpoints, logs, ports list |
| `Made` | the target is a computer it created, never itself | exec, files, start, stop, delete, restore, desktop and computer use |
| `Create` | forks itself or a computer it made, or makes a new computer; access handed over must be a subset of its own, else the call becomes `Ask` | `checkpoints.fork`, `computers.create` |
| `Ask(kind)` | becomes an approval request (section 4) | merge, replace main, GitHub access, secrets, network policy wider, publish a port, SSH access |
| `OwnerOnly` | refused | settings, Connections, exports and imports, approvals decide, secret values, updates |

"Created by" is recorded by the engine when a `Computer` caller creates or forks (`created_by`),
persisted, and is the only basis for `Tree` and `Made`. Lineage alone is not enough: a fork you
made by hand may hold more access than its parent.

A narrowing call (fewer hosts, removing a grant from a computer it made) is free. Results that
would show another computer are filtered, as `Filter` does today.

### 3. The channel inside a computer

Spike S1 confirmed this shape with the real runtime ([evidence](evidence/S1.md)); no new runtime patch is needed:

- The engine listens on one loopback TCP port per computer and serves the normal engine protocol
  on it with `Caller::Computer`. The computer's network rules allow exactly that port on the
  gateway address (`host.microsandbox.internal`, which the runtime already maps to the device's
  loopback; today only port 53 is allowed). No other computer's rules allow it.
- A token minted for the computer is required in the handshake and is valid only on its own
  port, so a copy of the token in a fork or an export proves nothing.
- The `silo` binary is inside the computer (Linux: a static musl build pushed by the engine at
  start; macOS: the device's own `silo`, copied over SSH). It reads `/etc/silo/endpoint.json`.
  `silo mcp` is registered with Claude Code and Codex the way Cua Driver is.
- The `/.msb` spool bridge and its folder parsing are removed.
- The rule is `--net-rule allow@host:tcp:<P>`, passed at create; it survives stop and start. A
  restore resets rules unless they are all passed again, so restore and fork build them from the
  stored policy, and a fork swaps in its own port. An import gets its rule at its first start.
  An existing computer gets it through the engine's restore in place while it is stopped.
- The device side always sees `127.0.0.1`, so the token is what identifies the computer. The
  client reconnects after a memory restore.
- Later hardening, not in this build: a runtime patch mapping the gateway port to a Unix socket,
  so no loopback port is open on the device.

Protocol version goes to 8 with this change, once, because `guestClient.*` is retyped.

### 4. Approvals

- **Record:** id, kind, caller, method, params, an engine-written summary (from the params, never
  from the agent), the agent's own note (shown as such), state (`pending`, `allowed`, `denied`,
  `cancelled`, `expired`), who decided, and the result or job id of the call once allowed.
  One JSON file per approval under `<app data>/approvals/`, like review requests.
- **Flow:** a call ruled `Ask` returns the error `approval_required` with the approval id.
  The caller polls `approvals.get`. The CLI and MCP tools wrap this: they wait up to a few
  minutes and return the outcome, or say the request is still waiting and give its id.
  On allow, the engine runs the stored call itself as the owner, after checking it again, and
  stores the result on the approval.
- **Methods:** `approvals.list`, `approvals.get`, `approvals.decide` (owner only),
  `approvals.cancel` (the caller, its own). Event `approvals.changed`. A notice category
  `approvals` so the app can notify.
- **Merge requests** stay on today's review-request machinery (digest pin, stale detection). An
  approval of kind `merge` wraps one; allowing it runs `changes.requests.keep`.
- **App:** the Pending page lists approvals and review requests together; the menu bar panel
  shows a count; a native notification opens the request.

### 5. Audit log

A separate SQLite file (`audit.sqlite3`, so the job registry's strict schema version is left
alone). One row per call of kind command, job or stream, and per refused or asked call of any
kind: time, caller kind and id, client name, method, target computer, outcome (`ok`, `error`,
`refused`, `asked`), approval id. Parameters are not stored. Trimmed by age and row count.
`audit.list` reads it; the app shows it under Activity.

### 6. Limits

No fixed caps on how many computers an agent makes. Instead:

- `device.resources` (open to every caller): memory total and available, memory pressure, CPUs,
  free disk, and each running computer's configured memory, so an agent can judge for itself.
- A safety floor for `Computer` callers only: a start, create, fork or checkpoint fails with
  the same facts when available memory or free disk is below the floor (defaults: memory
  `max(4 GiB, 10 %)`, disk `max(10 GiB, 5 %)`, changeable in settings). Above the floor but
  below twice the floor, the call works and its result carries a warning.
- The in-guest caps (32 forks, 3 running) go away. The CLI's `fork_ram_budget` goes with `silo.yaml` (item T6).

### 7. Merge

- `changes.keep` (wire name unchanged) takes `choices`: for each conflicting item, `fork`,
  `main`, or for a repository `branch` (the fork's commits are left in main as a branch and
  nothing is merged, so Git resolves it). Without a choice for a conflict the call answers with
  the list of conflicts and their ids, where today it refuses.
- Credentials are never brought over, as today. A database is never combined: main's stays
  unless the recipe has a migrate step; the result says so.
- Fast-forward means "main has not changed, so nothing can conflict": everything applies and
  main keeps running. It does not swap computers, because the agent lives in main.
- **Replace main** is today's promote: explicit, always an approval for a `Computer` caller, and
  the request says that it restarts main.
- User-facing words: `silo merge`, `silo replace-main`, `silo undo`, `silo drop`; MCP tools
  `merge` and `replace_main`; the app says Merge and Decline. Wire names, journal files and
  checkpoint kinds keep their current spelling.
- Removed: `auto_keep`, `fold: auto-code`, `autokeep.rs`.

### 8. Recipes, and the end of `.silo`

- The engine stores named recipes (`recipes.save`, `.get`, `.list`, `.remove`) with every key:
  system, size, packages, setup, services, ports, desktop, `quiet`, `scrub`, `keep` hooks, `test`.
- A computer records the recipe it was built from (name, hash and the full text). Forks inherit
  it. Fork applies the source's `quiet` and `scrub` in the engine; merge reads the target's
  `keep` hooks in the engine.
- The CLI loses `Project::discover`, `silo.yaml`, the recipe memory beside the socket, write-back
  and auto-keep. `silo recipe add <file>` uploads a recipe; `silo up --recipe <name>` still makes
  `main`. Built-in `linux-dev` stays; `openclaw` and `hermes` go.
- The hosted reviewer is hidden from the CLI and MCP (code and service stay). Folder forks and
  `--pr` stay as device-only commands; inside a computer they are not offered.

### 9. Network policy

- Per computer: `open` (today), `allowlist`, `closed`. An allowlist is a set of host patterns
  plus presets (package registries, GitHub, model APIs). GitHub's hosts, each assigned secret's
  hosts and Silo's own channel are always added.
- `network.policy.get` and `.set`. Stored on the computer, passed as the runtime's own
  `--net-rule` rules at create, start, restore and fork, and through the gateway's init policy
  for macOS and container computers. If the runtime cannot change rules while a computer runs,
  the change applies at the next start and the result says so.
- In allowlist mode the rule that lets the computer use any public resolver is dropped, so names
  resolve only through Silo's resolver.
- A `Computer` caller widening its own policy is an approval; narrowing is free.
- From S1: hostname rules for hosts no secret covers work only with the runtime's strict mode
  off, because under strict a TLS connection that is not intercepted is refused when only a
  hostname rule admitted it. Allowlist computers are created with strict off; `restore` needs a
  small runtime patch to carry the setting, the export code must reset it, and the macOS gateway
  needs a `policy` and a `strict` field in its init request. The threat model must say what
  strict off costs: a host that shares an address with an allowed host may be reachable.
- Rule sets. Open: default egress deny, `allow@dns`, the channel rule, `allow@public`.
  Allowlist: the same without `allow@public`, plus `allow@<host>:tcp:443` per entry (and the
  `*.` form). Closed: default deny and the channel rule only.
- Not in this build: the DNS query log (needs a runtime patch).

### 10. macOS computers

- **Network:** Recovery boots get no network card. A record remembers once setup has completed
  (`used`); from then on, and for every fork and import, a boot never uses Apple's NAT: the
  network bootstrap files are written to the stopped disk first, as forks already do, or the
  boot is refused with a message. Only a first install on a pristine disk still uses NAT.
- **App:** a macOS computer is an `ApplicationComputer` with an `os` field. It opens the same
  detail page; Files, Logs, Network, Secrets and GitHub pages include it; capabilities gain an OS
  dimension.

### 11. What is in a computer

- `agents.list` and `agents.install` for both systems: Claude Code and Codex, pinned and
  checked as Claude Code is today. Create takes `agents`. The macOS template still contains
  Claude Code until its next revision (changing it would rebuild every user's template).
- **Guest components:** one list of Silo-owned pieces with versions, one receipt in the computer
  (`/var/lib/silo/components.json`), reconciled at every start. First members: the `silo` client
  and its MCP registration, the GitHub credential helper, the accessibility helper. Computer use
  and the desktop streamer keep their own paths for now.
- **Upgrade path** (rebuild on the newest image, keep the workspace and home): spike S2, then
  build if the spike holds. It needs the workspace disk moved between two stopped computers.

### 12. Tokens, logins, exports

- A personal GitHub token is a secret of kind `github-token`. Several can exist. A computer
  uses signed-in access or one token secret. The engine still hands the proxy the same token
  profile, so the runtime patch does not change. The saved single token is migrated.
- Exports: a temporary fork is scrubbed of known login files, exported and dropped.
  Shipped only if a canary check shows the login's bytes are not in the file; otherwise today's
  warning stays. `~/.claude.json` joins the known login paths. **Built as best effort (T14):** the
  canary finds the bytes in the file, because a fork's disks are layers over the computer's and the
  shared read-only layers are written whole (trimming the copy does not reach them; the root is an
  overlay whose ext4 disk has to be mounted a second time to be trimmed). So the files are removed from
  the exported computer and the wording says traces may remain. Checkpoint, macOS and container exports
  keep today's warning. Evidence: `docs/direction/evidence/T14.txt`.

### 13. Onboarding

After the first computer (named `main`): pick an agent, install it, register Silo's tools, then
Open, which starts a terminal on the computer with the agent running. The screen also gives the
SSH name for desktop apps that connect over SSH.

## Work items

Each item is one agent in its own worktree, reviewed before it merges. Items in one wave do not
touch the same files, apart from generated files, which are regenerated at merge.

| Wave | Item | What | Model |
| --- | --- | --- | --- |
| A | **S1** | Spike with the bundled `msb`: gateway port rule, changing rules on an existing computer, restore and fork and export behaviour, hostname allow rules | Sonnet |
| A | **T1** | `Caller`, the policy module, `created_by`, the audit log and `audit.list`; the existing spool bridge switched to it | Opus |
| A | **T4** | Merge choices in `changes/keep`, the `branch` choice, fast-forward report | Sonnet |
| A | **T9** | macOS computers on the detail page and the other pages (frontend and Tauri commands) | Sonnet |
| B | **T2** | Approvals: store, methods, event, notice category, run on allow | Opus |
| B | **T3** | Limits and `device.resources` | Sonnet |
| B | **T7** | macOS network: no card in Recovery, the `used` flag, offline bootstrap for forks and imports | Sonnet |
| C | **T5** | Channel: listener per computer, token, `silo` over TCP, guest components and MCP registration, spool bridge removed, protocol 8 | Opus |
| C | **T6** | Recipes in the engine, `.silo` removed, `silo merge` and the new MCP tools, reviewer hidden | Sonnet |
| C | **T10** | App: approvals on the Pending page, menu bar, notifications; audit under Activity | Sonnet |
| D | **T8** | Network policy: engine, API, CLI | Sonnet |
| D | **T11** | Tokens as secrets: engine, migration, GitHub and Secrets pages | Sonnet |
| D | **T12** | `agents.list` and `.install`, Codex, create with agents | Sonnet |
| E | **T13** | App: network policy control, onboarding agent step, Open | Sonnet |
| E | **T14** | Export scrub with the canary check | Sonnet |
| E | **S2/T15** | Upgrade path spike, then build | Sonnet |
| F | **T16** | Documents: threat model, CLI, in-guest, recipes, vocabulary, roadmap, direction | Sonnet |
| F | **V** | Live run on the Dev build, end to end, with the app driven on screen | me |

## Checks

- Every item: the focused Rust or Vitest suites for what it touched, `fmt`, `typecheck` and
  `lint` where relevant, the schema and generated requests regenerated.
- T1, T2, T5, T8: a threat-model review by me as well as the code review, since they move a
  security boundary.
- T5, T8, T7, T14, T15 need a real computer. They run on the Dev engine, one at a time, with
  `e2e-*` computers, and leave their output under `docs/direction/evidence/`.
- At the end: the whole workspace test suite, the frontend suite, and the live run.

## Facts the plan rests on

- 175 methods in `api/src/methods/registry.rs`; kind is the only per-method metadata; the schema
  file and method counts are pinned by tests and regenerated with two examples.
- A request reaches every handler through `methods::dispatch` with a `RequestContext`; the socket
  checks only the user id; the client name is self-reported.
- The in-guest bridge polls files under `/.msb`, checks a token, and calls `methods::dispatch`
  as a fixed client; its allow-list has 13 methods and no exec.
- Review requests are JSON files, fork-specific, with no event; the app polls.
- SQLite is already a dependency; the job registry accepts exactly one schema version.
- The runtime already maps guest traffic to the gateway address onto the device's loopback, and
  already has host rules, strict mode and a `--vsock` flag; Silo passes only strict mode today.
- The macOS gateway takes its policy at init only; there is no live policy update.
- Keep refuses on any conflict; there is no per-item selection; promote swaps computers by
  renaming and is the only fast-forward today.
- Recipes, fork rules, keep hooks and all of `silo.yaml` live only in the CLI.
- The app never speaks the engine protocol: every call is a Tauri command; macOS computers use
  a separate model and store; `agentTools` is computed but unused by the app.
- macOS setup uses Apple's NAT whenever setup is not complete, including forks that need a boot,
  imports and failed-setup starts; forks prepared offline already boot behind the gateway.
- One personal token exists, selected per computer; the proxy has a token profile that handles
  Git's Basic authentication.
- Exports are whole disks; a scrub needs a temporary fork.
