# Silo roadmap

This is the work planned after the current release, in order. It starts from what Silo does not
do today, as stated in the [threat model](THREAT-MODEL.md) and measured by the [isolation evals](evals/README.md), and says which
milestone closes or narrows each gap.

Last updated 2026-11-11. When a milestone lands, its section here is shortened to a line under
[Done](#done) and the threat model and evals are updated in the same change.

**Order.** *Next* is work that needs no funding and starts now. *Funded* is work that starts once
a grant or a paying pilot covers it, because it costs money or more time than can be given
unpaid.

## Summary

| Milestone | Closes or narrows | Order |
| --- | --- | --- |
| [1. Cua Driver for computer use](#1-cua-driver-for-computer-use) | ChatGPT-for-Linux download, two host shares in every Linux computer | In progress |
| [2. Egress policy per computer](#2-egress-policy-per-computer) | Open egress (narrowed by the policy), DNS as an unlogged channel | Built; DNS log next |
| [3. Reflected-secret redaction](#3-reflected-secret-redaction) | Allowed host echoing a secret back | Next |
| [4. Socket client identity and audit log](#4-socket-client-identity-and-audit-log) | Any same-user process drives the engine; the audit log exists, client identity does not | Audit log built; identity next |
| [5. Review of the `/.msb` runtime share](#5-review-of-the-msb-runtime-share) | Read-write host share with no security review (Silo no longer uses it for requests) | Next |
| [6. Linux host daemon](#6-linux-host-daemon) | Engine embeddable on macOS only | Next |
| [7. Protocol reference and examples](#7-protocol-reference-and-examples) | Embedding needs reading the source | Next |
| [8. Project governance and docs cleanup](#8-project-governance-and-docs-cleanup) | No contribution or decision process | Next |
| [9. Isolation evals in CI](#9-isolation-evals-in-ci) | Evals run by hand on one Mac | Funded |
| [10. External security review](#10-external-security-review) | Every boundary checked only by its author | Funded |
| [11. Signed and notarized releases](#11-signed-and-notarized-releases) | Unsigned app and engine, Gatekeeper override | Funded |
| [12. Published crates and bindings](#12-published-crates-and-bindings) | Embedding outside Rust means generating types yourself | Funded |
| [13. After the direction build](#13-after-the-direction-build) | Open items left by the agent-inside-the-computer work | Next |

## 1. Cua Driver for computer use

**Status: in progress.**

Linux computers today download ChatGPT for Linux from OpenAI (about 450 MB) and LCU, and mount
both into the guest read-only at `/opt/silo/chatgpt` and `/opt/silo/lcu`. Silo is replacing them
with [Cua Driver](https://github.com/trycua/cua) (MIT), pinned by version and SHA-256 and built
into the Linux guest image. Agents reach it as a local MCP server (`cua-driver mcp`), registered for
Claude Code and Codex, with its telemetry and update check switched off.

Done when:

- New Linux computers have computer use from the image alone: no download from OpenAI, no host
  share for computer use. Computers on the previous image get the same pinned driver once.
- Guest image v5 is published with Cua Driver and its licence notice; the ChatGPT-only packages are
  gone from it.
- macOS computers move from LCU to Cua Driver in a follow-up change.
- The threat model drops the two read-only shares, and the `host-files` eval stops testing a
  ChatGPT mount.

## 2. Egress policy per computer

**Status.** Built: a computer has a network policy of three modes, stored with it, set through
`network.policy.set`, `silo network policy` and a control in the app, and applied to Linux virtual
machines, macOS computers and containers. The `open-egress` eval in allowlist mode and the DNS
query log are still open.

**Gap before.** A computer could open any connection to any public address, TCP or UDP, and DNS
queries went out unfiltered through the host's resolvers. The `open-egress` eval posts a file from
`/workspace` to an arbitrary public host and succeeds.

**What exists.**

- Three modes: *open* (today's behaviour, the default for existing computers), *allowlist* (named
  hosts, from the owner's list and from presets, on TCP 443) and *closed* (nothing out).
- The hosts a computer needs are added for it and shown: GitHub's hosts when GitHub is granted and
  each assigned secret's allowed hosts.
- The same policy for macOS computers and containers through `silo-macos-gateway`'s start-up
  request, which carries the rules and a strict flag.
- Saved with the computer; a checkpoint restore keeps it, a fork inherits it, an export drops it
  and an import starts closed.
- A computer's agent may narrow its own policy and those of the computers it made without asking,
  and asks you to widen one.
- A small runtime patch lets `msb restore` take `--net-strict`, so a restored or forked allowlist
  computer keeps its hostname rules working.

**Still to do.** A DNS query log per computer, readable through the API and shown in Activity, with
a filter for names that are not on the list; the `open-egress` eval passing on a computer in
allowlist mode; running the allowlist on a Linux host and on a live macOS or container gateway.

**Still open after it.** An allowlist does not stop sending data to an allowed host (a gist, an
issue, a package upload). DNS queries to names that are not on the list still resolve, and a host
that shares an address with an allowed host may be reachable while strict is off. All of this is in
the threat model.

## 3. Reflected-secret redaction

**Gap today.** If a secret's allowed host returns request headers in its response (debug echo
endpoints, some error pages), the guest reads the real value. The `secret-reflected` eval
reproduces it.

**Plan.** In the runtime's proxy, on connections where a secret was substituted, replace the real
value with its placeholder in the response headers and body before they reach the guest. This is
a change to Silo's MicroSandbox patch, offered upstream.

Done when the `secret-reflected` eval passes and the threat model moves the case from non-goals to
a boundary with its limit: a value the server transforms (encoded, split, hashed) is not caught.

## 4. Socket client identity and audit log

**Status.** The audit log is built: every call that can change something, and every refused or
held call, is a row with the caller (owner, device or computer), the method and the computer,
readable through `audit.list` and shown under Activity. A computer's identity is its channel.
What is left is the identity of programs on the device.

**Gap today.** `silod` checks only that a client runs as the same user. Any process of that user
can create, export or delete computers, run commands as root in them, and change GitHub and secret
assignments. The `same-user-client` eval does this from an unsigned script. The client name in
the audit log is self-declared.

**Plan.**

- On macOS, identify each client by its code signature (the connection's audit token checked
  against a code requirement). On Linux, by `SO_PEERCRED` (PID, UID) and the executable it maps to.
- A list of allowed clients, managed by the user. Unknown clients are refused or limited to
  read-only methods, depending on a setting.
- Client identity in the audit log rows that exist today.

Done when an unsigned script is refused by default, the audit log names a verified client, and the
`same-user-client` eval passes.

**Still open after it.** Malware running as the same user can still tamper with an allowed client
or the user's files; same-user malware stays a non-goal. This milestone stops casual and
accidental use of the socket and makes every action attributable.

## 5. Review of the `/.msb` runtime share

**Gap today.** Every Linux computer sees MicroSandbox's own runtime directory read-write at
`/.msb`. MicroSandbox writes scripts and certificates there and keeps its checkpoint state and a
heartbeat file in it. Silo no longer puts requests or answers in the share and reads nothing a
guest wrote there for its own use. The eval could not reach a host canary through it, but the share
has had no security review.

**Plan.** Read the passthrough file system and every host read from that directory. Make the
share read-only from the guest where the runtime allows it, and move the heartbeat to a separate,
minimal channel otherwise. Fix upstream where the change belongs to MicroSandbox. Add an eval
scenario that writes there and checks what the host does with it.

Done when the threat model either drops the share from the guest's writable surface or documents
what the guest can write and why the host is safe reading it, backed by the new scenario. The
external review (milestone 10) covers it again.

## 6. Linux host daemon

**Gap today.** `silo-engine` builds and passes its tests on Linux in CI, but `silod`, the daemon
apps embed, ships for Apple Silicon Macs only.

**Plan.**

- Split the macOS-only parts of `silod` (Objective-C bindings, VM windows, launchd and Service
  Management, code-signature checks) behind platform features, as already done in `silo-engine`.
- A systemd user unit in place of the LaunchAgent, and XDG directories for state and the socket.
- Restore the Linux packaging and verification workflows removed in `be300f4f`, adapted to the
  engine archive.
- Release `silo-engine-<version>-linux-x64.tar.gz` and `-linux-arm64.tar.gz` beside the macOS
  archive.
- A CI job that starts a real Linux computer on a KVM-enabled runner.
- Linux computers only on a Linux host; macOS computers stay Mac-only.

Done when both Linux archives are on an engine release, the KVM job is green, and the threat model
has a Linux host section (it needs its own boundary review: KVM, the socket, Keychain replaced by a
Secret Service or file store).

**Progress (Nov 2026).** `silod` builds, passes its tests and serves on Linux
([remote hosts](hosts/PLAN.md) phase 2): the Apple code is behind `cfg`, a plain main-thread
loop replaces AppKit, the manifest sits beside the binary, data, cache and logs follow XDG, the
socket checks `SO_PEERCRED`, credentials go to a file store, and the daemon needs no GTK. CI
runs the silod, CLI and client tests on Linux. Phase 3 added `silod --service`, which installs a
systemd user unit, the `linux-x64` and `linux-arm64` archives in the engine release workflow
(built natively on `ubuntu-24.04` and `ubuntu-24.04-arm`), and `.github/workflows/linux-host.yml`,
which installs the Dev archive on a KVM runner and creates, starts, runs a command in, forks and
drops a Linux computer through the `silo` command. Phase 4 added `silo host init` (VM and account
checks, the service, Connections on, `--authorize` for a controller's key, the address), `silo host
status` and `revoke`, and `silo devices add` by address (with `list`, `remove` and `key`); phase 5
routed `exec`, `fork -- <command>` and the computer-use tools to another device. The KVM job is
green, including its two-account run: one account prepared with `silo host init --authorize`, the
other adding it by address over the runner's sshd and creating, starting, running a command in and
dropping a computer on it through Connections. The threat model's
[Linux hosts](THREAT-MODEL.md#linux-hosts) section covers KVM, the socket, the file store, the
bridge, the journal and lingering. Still open: an engine release that carries the Linux archives.

## 7. Protocol reference and examples

A reference for every method, error, job and event, and the versioning policy (how protocol
versions are negotiated, what counts as a breaking change). An `examples/` directory with a Rust
program that creates, starts, runs a command in, and stops a computer, run in CI against the
schema.

## 8. Project governance and docs cleanup

`CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue and pull request templates, and `GOVERNANCE.md`
(who decides, how a second maintainer is added, how security reports are handled). Old reviews,
verification logs and plans move to `docs/archive/`, and `docs/README.md` becomes a short map of the
current documents.

## 9. Isolation evals in CI

**Gap today.** The evals run by hand on one Mac, and only the results of that run are published.
GitHub's hosted macOS runners are virtual machines that cannot start nested VMs, so they cannot
run a Silo computer.

**Plan.** Run the full suite, including GitHub scope and cross-user socket scenarios, on a
dedicated Apple Silicon Mac (owned or rented) for every release, and publish the results with the release. Once the Linux daemon
(milestone 6) exists, run the Linux-computer scenarios on every pull request on a KVM runner at no
cost. Add scenarios for the egress policy, socket identity and `/.msb` as those milestones land.

Done when every release links its eval results and a failing boundary blocks the release.

## 10. External security review

An independent review by a specialist firm of the boundaries in the threat model, in this order: the network and secret
proxy patches to MicroSandbox, the GitHub token minting and selection, the `/.msb` share and
virtiofs, the engine socket, and import of exported computers. Findings and fixes are published
with the report.

## 11. Signed and notarized releases

Cost: Apple Developer Program membership, €99 a year.

Sign the Silo app and `silod` with a Developer ID and notarize them, so first launch no longer
needs **Open Anyway**. Sign engine archives so embedding apps can verify them: Apple signing for the
macOS archive, and a published signature (for example Sigstore) for Linux archives.

## 12. Published crates and bindings

Publish `silo-api` and `silo-client` on crates.io under the versioning policy from milestone 7.
Generate and publish Python and TypeScript clients from the JSON schema.

## 13. After the direction build

**Status.** The agent now lives in a computer with Silo's tools, asks for what widens its access,
merges back, and works under a per-computer network policy (see [the direction](DIRECTION.md#what-is-built)).
These are the next steps it left, in rough order.

- **Rebuild on the newest image**, keeping the workspace and home: `silo rebuild`, Linux first
  (a macOS rebuild later). The design is settled; homes over about 45 % of the root disk are
  refused, and the old computer stays stopped as `<name>-undo`.
- **Live checks on a real device** of what was built from unit tests and the Dev engine alone: the
  app screens (Pending, onboarding, network policy, macOS computers, tools), Open for each agent,
  macOS forks and imports behind the gateway, the allowlist on a Linux host, and a Codex install in
  a real computer.
- **Fix:** declining a merge approval leaves the fork's review request open; a failed create-time
  agent install shows a generic message; a creating computer may not accept a network policy
  change; the network page's link does not scroll to the section.
- **Close the loopback port**: map the gateway port to a Unix socket in a private folder so no port
  is open on the device, and detect a port another program holds when the engine starts.
- **Recipes:** stop a fork of a recipe's build from carrying the recipe's full text to a computer
  that may not read it; decide whether every computer should see every recipe's name.
- **Limits:** hold the memory of approved starts too; measure disk at the storage location the
  owner chose, not only at app data.
- **Computers' views:** cut the event stream, notifications and the job list per computer so an
  agent need not poll.
- **Exports:** flatten a fork's disk layers in the runtime, so a login removed from an export is
  gone from the file and not only from the imported computer.
- **Tokens:** let the old single-token methods find tokens by any name, and let a fork copy token
  access.
- **Merge:** check an ignored `.env` for credentials the way Git history is checked.
- **First install of a macOS computer** still uses Apple's network; closing the window needs the
  bootstrap files inside the template.
- **Not scheduled from this work:** the DNS query log (milestone 2), reflected-secret redaction
  (milestone 3), one helper process per macOS computer, and signed releases (milestone 11).

## Considered, not scheduled

These gaps are in the threat model and stay there. Each would need its own design first:

- **A second layer under the VM.** The runtime runs as the user's own process without the App
  Sandbox, so a hypervisor or virtio escape gives the user's account. Running it sandboxed or as a
  separate user is a large change to how the runtime is packaged.
- **Repository scoping in personal-token mode.** A personal token carries all its permissions by
  design; GitHub App mode is the scoped path.
- **Network policy for macOS computers during a first install.** A first install and the
  setup of a copy of a template use Apple's NAT until setup finishes and can reach the Mac and
  the LAN in that window; the disk then holds only Apple's image and Silo's scripts. A computer
  an agent may have used (every fork and import, and any computer whose setup has completed)
  never boots on Apple's NAT, and Recovery has no network card. Closing the window for a first
  install needs the bootstrap files inside the template, which rebuilds every user's template.
- **Bandwidth and connection limits** per computer.

## Ideas to revisit

Product ideas that are not gaps in the threat model. Each needs its own design first.

- **Exact rebuilds with Nix.** Forks, checkpoints and shared computers are exact copies, but
  rebuilding a recipe from scratch is equivalent, not identical, because apt, brew, npm and other
  installs download whatever is current at build time. An optional recipe list
  (`nix: [nodejs_22, jq]`) could install from a Nix package catalogue revision recorded in a lock
  file next to the recipe, so a rebuild later or on another machine gets the same tools on Linux
  and macOS computers; an update moves the lock forward. Users would not write Nix. Ruled out for
  now: Nix as the default list, mapping apt or brew names to Nix names, and a host `/nix/store`
  shared between computers (it breaks disk-only forks and whole-computer export).

## Done

- 2026-11: The agent inside a computer: callers and a policy, approvals, the audit log, a channel
  and the `silo` tools in each computer, merge and replace main, recipes in the engine, a network
  policy per computer, macOS computers on the same page, personal tokens as secrets, agent install
  and onboarding, and export without logins (best effort). See [the direction](DIRECTION.md#what-is-built).
- 2026-11: `silo-engine` builds and passes its tests on Linux in CI.
- 2026-11: Every crate declares its MIT licence and repository.
