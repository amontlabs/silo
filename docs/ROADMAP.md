# Silo roadmap

This is the work planned after the current release, in order, with a size and a cost for each
milestone. It starts from what Silo does not do today, as stated in the threat model
(`docs/THREAT-MODEL.md`) and measured by the isolation evals (`docs/evals/`), and says which
milestone closes or narrows each gap.

Last updated 2026-11-03. Silo is maintained by one developer; sizes are that person's working
time. When a milestone lands, its section here is shortened to a line under [Done](#done) and the
threat model and evals are updated in the same change.

## How to read sizes and costs

| Size | Working time |
| --- | --- |
| S | up to 1 week |
| M | 2 to 3 weeks |
| L | 4 to 6 weeks |

**Cost** is cash that has to be spent beyond that time: services, hardware, fees and outside
work. Amounts in euros. Where a figure is an estimate rather than a published price, it says so.
Development time is valued separately in grant budgets, not here.

**Order.** *Next* is work that needs no funding and starts now. *Funded* is work that starts once
a grant or a paying pilot covers it, because it costs money or more time than can be given
unpaid.

## Summary

| Milestone | Closes or narrows | Size | Cost | Order |
| --- | --- | --- | --- | --- |
| [1. Cua Driver for computer use](#1-cua-driver-for-computer-use) | ChatGPT-for-Linux download, two host shares in every Linux computer | M | none | In progress |
| [2. Egress policy per computer](#2-egress-policy-per-computer) | Open egress, DNS as an unlogged channel | L | none | Next |
| [3. Reflected-secret redaction](#3-reflected-secret-redaction) | Allowed host echoing a secret back | S | none | Next |
| [4. Socket client identity and audit log](#4-socket-client-identity-and-audit-log) | Any same-user process drives the engine, no record of who did what | M | none | Next |
| [5. Review of the `/.msb` runtime share](#5-review-of-the-msb-runtime-share) | Read-write host share with no security review | S | none | Next |
| [6. Linux host daemon](#6-linux-host-daemon) | Engine embeddable on macOS only | L | none | Next |
| [7. Protocol reference and examples](#7-protocol-reference-and-examples) | Embedding needs reading the source | S | none | Next |
| [8. Project governance and docs cleanup](#8-project-governance-and-docs-cleanup) | No contribution or decision process | S | none | Next |
| [9. Isolation evals in CI](#9-isolation-evals-in-ci) | Evals run by hand on one Mac | M | about €1,000 a year (estimate) | Funded |
| [10. External security review](#10-external-security-review) | Every boundary checked only by its author | M (support) | €15,000 to €40,000 (estimate) | Funded |
| [11. Signed and notarized releases](#11-signed-and-notarized-releases) | Unsigned app and engine, Gatekeeper override | S | €99 a year | Funded |
| [12. Published crates and bindings](#12-published-crates-and-bindings) | Embedding outside Rust means generating types yourself | M | none | Funded |

Total unfunded work (milestones 2 to 8): about 14 to 19 weeks. Funded work (9 to 12): about 7 to
10 weeks plus the costs above.

## 1. Cua Driver for computer use

**Status: in progress.** Size M. Cost none.

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

Size L. Cost none.

**Gap today.** A computer can open any connection to any public address, TCP or UDP, and DNS
queries go out unfiltered through the host's resolvers. The `open-egress` eval posts a file from
`/workspace` to an arbitrary public host and succeeds.

**Plan.** MicroSandbox, the runtime under Linux computers and the macOS gateway, already supports
hostname rules and strict enforcement; Silo passes none today. This milestone exposes a policy per
computer in the API and the app:

- Three modes: *public* (today's behaviour, the default for existing computers), *allowlist*
  (named hosts and ports only) and *deny* (no outbound traffic beyond DNS to the gateway).
- The hosts a computer needs are added for it and shown: GitHub hosts when GitHub is granted,
  each assigned secret's allowed hosts, package registries the user picks.
- A DNS query log per computer, readable through the API and shown in Activity.
- The same policy for macOS computers through `silo-macos-gateway`.
- Saved with exports and checkpoints, applied on import and restore.

Done when the policy is in the API and protocol, tested in Rust, the `open-egress` eval passes on a
computer in allowlist mode, and the threat model's network section describes it.

**Still open after it.** An allowlist does not stop sending data to an allowed host (a gist, an
issue, a package upload). DNS queries to allowed names remain a narrow channel. Both stay in the
threat model.

## 3. Reflected-secret redaction

Size S. Cost none.

**Gap today.** If a secret's allowed host returns request headers in its response (debug echo
endpoints, some error pages), the guest reads the real value. The `secret-reflected` eval
reproduces it.

**Plan.** In the runtime's proxy, on connections where a secret was substituted, replace the real
value with its placeholder in the response headers and body before they reach the guest. This is
a change to Silo's MicroSandbox patch, offered upstream.

Done when the `secret-reflected` eval passes and the threat model moves the case from non-goals to
a boundary with its limit: a value the server transforms (encoded, split, hashed) is not caught.

## 4. Socket client identity and audit log

Size M. Cost none.

**Gap today.** `silod` checks only that a client runs as the same user. Any process of that user
can create, export or delete computers, run commands as root in them, and change GitHub and secret
assignments. The `same-user-client` eval does this from an unsigned script. Nothing records which
client asked for what.

**Plan.**

- On macOS, identify each client by its code signature (the connection's audit token checked
  against a code requirement). On Linux, by `SO_PEERCRED` (PID, UID) and the executable it maps to.
- A list of allowed clients, managed by the user. Unknown clients are refused or limited to
  read-only methods, depending on a setting.
- An append-only audit log of client identity, method and computer for every state-changing
  request, readable through the API and exportable with logs.

Done when an unsigned script is refused by default, every state-changing call appears in the audit
log, and the `same-user-client` eval passes.

**Still open after it.** Malware running as the same user can still tamper with an allowed client
or the user's files; same-user malware stays a non-goal. This milestone stops casual and
accidental use of the socket and makes every action attributable.

## 5. Review of the `/.msb` runtime share

Size S. Cost none.

**Gap today.** Every Linux computer sees MicroSandbox's own runtime directory read-write at
`/.msb`. The host writes scripts and certificates there and reads a heartbeat file the guest
writes. The eval could not reach a host canary through it, but the share has had no security
review.

**Plan.** Read the passthrough file system and every host read from that directory. Make the
share read-only from the guest where the runtime allows it, and move the heartbeat to a separate,
minimal channel otherwise. Fix upstream where the change belongs to MicroSandbox. Add an eval
scenario that writes there and checks what the host does with it.

Done when the threat model either drops the share from the guest's writable surface or documents
what the guest can write and why the host is safe reading it, backed by the new scenario. The
external review (milestone 10) covers it again.

## 6. Linux host daemon

Size L. Cost none.

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

## 7. Protocol reference and examples

Size S. Cost none.

A reference for every method, error, job and event, and the versioning policy (how protocol
versions are negotiated, what counts as a breaking change). An `examples/` directory with a Rust
program that creates, starts, runs a command in, and stops a computer, run in CI against the
schema.

## 8. Project governance and docs cleanup

Size S. Cost none.

`CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue and pull request templates, and `GOVERNANCE.md`
(who decides, how a second maintainer is added, how security reports are handled). Old reviews,
verification logs and plans move to `docs/archive/`, and `docs/README.md` becomes a short map of the
current documents.

## 9. Isolation evals in CI

Size M. Cost about €1,000 a year (estimate: a rented Apple Silicon Mac for release runs, around
€80 a month).

**Gap today.** The evals run by hand on one Mac, and only the results of that run are published.
GitHub's hosted macOS runners are virtual machines that cannot start nested VMs, so they cannot
run a Silo computer.

**Plan.** Run the full suite, including GitHub scope and cross-user socket scenarios, on a
dedicated Mac for every release, and publish the results with the release. Once the Linux daemon
(milestone 6) exists, run the Linux-computer scenarios on every pull request on a KVM runner at no
cost. Add scenarios for the egress policy, socket identity and `/.msb` as those milestones land.

Done when every release links its eval results and a failing boundary blocks the release.

## 10. External security review

Size M of support work (scoping, answering questions, fixing findings). Cost €15,000 to €40,000
(estimate for a two-to-four-week review by a specialist firm; to be replaced by real quotes).

An independent review of the boundaries in the threat model, in this order: the network and secret
proxy patches to MicroSandbox, the GitHub token minting and selection, the `/.msb` share and
virtiofs, the engine socket, and import of exported computers. Findings and fixes are published
with the report.

## 11. Signed and notarized releases

Size S. Cost €99 a year (Apple Developer Program).

Sign the Silo app and `silod` with a Developer ID and notarize them, so first launch no longer
needs **Open Anyway**. Sign engine archives so embedding apps can verify them: Apple signing for the
macOS archive, and a published signature (for example Sigstore) for Linux archives.

## 12. Published crates and bindings

Size M. Cost none.

Publish `silo-api` and `silo-client` on crates.io under the versioning policy from milestone 7.
Generate and publish Python and TypeScript clients from the JSON schema.

## Considered, not scheduled

These gaps are in the threat model and stay there. Each would need its own design first:

- **A second layer under the VM.** The runtime runs as the user's own process without the App
  Sandbox, so a hypervisor or virtio escape gives the user's account. Running it sandboxed or as a
  separate user is a large change to how the runtime is packaged.
- **Repository scoping in personal-token mode.** A personal token carries all its permissions by
  design; GitHub App mode is the scoped path.
- **Network policy for macOS computers before setup completes.** They use Apple's NAT until setup
  finishes and can reach the Mac and the LAN in that window.
- **Bandwidth and connection limits** per computer.

## Done

- 2026-11: `silo-engine` builds and passes its tests on Linux in CI.
- 2026-11: Every crate declares its MIT licence and repository.
