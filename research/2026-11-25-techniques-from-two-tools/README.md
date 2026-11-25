# Techniques from two open-source tools, and a closed network with no network stack

Research of 25 November 2026, by an agent, for polarzero. Two open-source tools (Apache-2.0) run
coding agents in virtual machines on a Mac. One gives each agent a macOS or Linux guest on Apple's
Virtualization framework, as the first build does. The other runs each task in a Linux VM and
holds model spend at a gateway on the Mac. This page reads their code and documents against #348
(the first build), #370 (its specs) and #374 (spend budgets), and keeps what Silo can use. It
names neither tool: what they are next to Silo is not for this repository.

One technique changes a decision of the first build, so it was measured on Linux: a computer with
a network card that leads nowhere, whose only way out is a proxy on the Mac reached over vsock.
The rest are smaller and come with what they change, by entry.

Nothing here is a decision. The plan changed only by agents' notes, listed at the end.

## In short

- **A closed network needs no network stack.** The guest gets a network card whose other end, on
  the Mac, answers ARP for one router address and drops every other frame. There is no route and no
  name server, and whatever root in the guest configures on the card reaches nothing. The only way out is a proxy at
  `127.0.0.1:3128` in the guest, relayed over vsock to a proxy on the Mac. Measured on Linux, with
  a Unix socket standing in for vsock: Claude Code worked through it, every one of its
  connections was in the proxy's log by host name, and without the proxy settings it reached
  nothing. #370 says a closed network is "not possible" before gvproxy is patched; it is.
- **The guest's clock falls behind while the Mac sleeps.** One tool found a guest an hour and a
  half behind after a night, which breaks certificates and sign-ins. It sets the guest's clock
  from the Mac at start, every 5 minutes and at each wake. Silo's boundary signs certificates on
  the fly, so a guest behind the Mac refuses Silo's own certificates.
- **Putting back a path as root needs care in a computer an agent controls.** The tool's undo
  never follows a link, reaches every entry from one open folder, unlocks what the agent locked,
  and moves the agent's version aside rather than deleting it. #370's put-back runs `sudo tar -x`
  and `mv` in main.
- **Deleting a checkpoint can free nothing for up to a day.** A local Time Machine snapshot on the
  Mac keeps deleted data, and the private size APFS reports for a clone reads 0 while one exists.
  The tool counts space by clone ids and blocks instead.
- **Budgets, from the gateway tool:** one call at a time near the limit bounds the overshoot to
  one call; a count of calls holds where money cannot be counted; a ledger that cannot be read
  refuses; a window measured on a clock that can jump either forgets spend or deletes it; a
  figure the computer wrote is never summed; and batch routes are refused, not counted.
- Four smaller points: which files to show first in the morning, two logs for connections, a
  macOS guest's password readable by root, and the gateway's undercounts that #374 already avoids.

## 1. A closed network with no network stack

### The design

```
 in the computer                                   on the Mac
 ┌──────────────────────────────┐                  ┌──────────────────────────────────────┐
 │ program ── HTTPS_PROXY ──▶   │      vsock       │                                      │
 │   127.0.0.1:3128 (relay) ────┼──────────────────┼─▶ proxy: CONNECT host:port, rules,  │
 │                              │                  │   name looked up here, record ──▶ out │
 │ network card ────────────────┼── socket pair ───┼─▶ answers ARP for 10.254.0.1,       │
 │   10.254.0.2, no route out   │                  │   drops every other frame            │
 └──────────────────────────────┘                  └──────────────────────────────────────┘
```

- **The card.** `VZFileHandleNetworkDeviceAttachment` over a Unix datagram socket pair. The Mac's
  end reads every frame, answers an ARP request for the router's address with a made-up MAC, and
  drops the rest. The tool found that a macOS 27 guest needs that one answer: without it macOS
  keeps the link "not reachable", sets no default route, never makes its proxy settings global,
  and URLSession programs report -1009, offline. With it, the whole system uses the proxy. A Linux
  guest gets no card at all.
- **The way out.** A daemon in the guest listens on `127.0.0.1:3128` and carries each connection
  over vsock to the Mac (`AF_VSOCK` works in a macOS guest; the tool's daemon is Swift and uses it
  on both systems). The proxy on the Mac takes `CONNECT host:port` and plain `http://` requests,
  checks the host and port against the rules, looks the name up on the Mac, and refuses an address
  on the Mac or the local network, NAT64 and 6to4 forms included.
- **Which programs follow it.** The proxy is offered three ways: the guest's system proxy
  settings (URLSession programs, `softwareupdate`, Python's `urllib`), `HTTP_PROXY` and
  `HTTPS_PROXY` in every program's environment (curl, git, SwiftPM, npm, pip), and a `ProxyCommand`
  in `/etc/ssh/ssh_config.d/` for ssh and git over SSH. Node's `fetch` needs
  `NODE_USE_ENV_PROXY=1`. A program that follows none of them has no network, and fails at once
  because there is no name to look up.
- **The way in.** The tool reaches its guest over vsock only. Silo's way in (#370, step 2) is ssh
  through `silo proxy`, which the helper connects to main's address; over vsock it needs a small
  relay in main from a vsock port to `127.0.0.1:22`, or sshd started by launchd on that port.

### Measured on Linux

`closed-network/run.sh`. A network namespace with only its loopback stands in for the computer:
no route, no name server, and an `/etc/hosts` with nothing but `localhost`. `relay.py` listens on
`127.0.0.1:3128` inside and carries each connection to a Unix socket, which stands in for vsock:
a channel between the two sides that is not a network. `proxy.py` outside allows
`api.anthropic.com`, `github.com`, `registry.npmjs.org`, `pypi.org` and `files.pythonhosted.org`,
logs every connection, and sends `api.anthropic.com` to #374's stand-in provider. Each case ran
once without the proxy settings and once with them. Claude Code is 2.1.42 (the one installed in
this container, older than the 2.1.331 of #374); Node is 22.23.

| Case | Without the proxy settings | With them |
|---|---|---|
| A name lookup (`getent hosts`) | fails at once | (not needed: the proxy looks names up) |
| A packet to `8.8.8.8:53`, `curl --noproxy '*' https://1.1.1.1/` | "Network is unreachable", at once | (no way around the proxy) |
| `curl https://api.anthropic.com/v1/models` | "Could not resolve host", at once | 200, through the proxy |
| `curl https://example.org/`, not allowed | fails at once | the proxy answers 403, and logs the refusal |
| `git ls-remote https://github.com/...` | "Could not resolve host", at once | through the proxy (502: this test reaches nothing beyond the stand-in) |
| `npm view left-pad` | `EAI_AGAIN`, in 0.2 s | through the proxy (`E502`) |
| `pip download six` | no version found, in 0.4 s | through the proxy |
| Python `urllib` | name resolution fails, at once | 200 |
| Node 22 `fetch` | `EAI_AGAIN` | `EAI_AGAIN` with the variables alone; 200 with `NODE_USE_ENV_PROXY=1` |
| Claude Code, `claude -p` | retried for 193 s, then "API Error: Can't reach the API server — check your internet or DNS (EAI_AGAIN)" | worked: its model call was answered and billed by the stand-in, and it ended at `--max-turns 1` in 0.8 s |

The proxy's log of the whole run has 16 connections. Twelve went to `api.anthropic.com`: three
from curl, `urllib` and `fetch`, and nine from Claude Code (its model call, and its posts to
`/api/eval/...` and `/api/event_logging/v2/batch` on the same host), 350 KB up and 51 KB down in
all. One refusal (`example.org`), and one connection each to `github.com`, `registry.npmjs.org`
and `pypi.org`, allowed and answered 502 because this test reaches nothing else. Nothing reached
the stand-in provider without going through the proxy: in the runs without the proxy settings, the
log has no line and the stand-in billed nothing.

Two things to know from it. Without the proxy, Claude Code does not fail at once: it retries for
about three minutes, so a program that ignores the settings looks like a slow network before it
fails. And Node's own `fetch` ignores `HTTPS_PROXY` unless `NODE_USE_ENV_PROXY=1` is set, so main's
environment sets both.

### What it changes

- **`network-stack` (open).** Its answer says every packet goes through Silo's stack and never
  Apple's NAT. A dead-end card with a proxy keeps that promise with no stack: a packet that does not
  go to the proxy goes nowhere, and the proxy is Silo's. What it gives up is a program that ignores
  proxy settings, UDP (QUIC, DNS, games), ICMP and raw TCP to other ports: those get nothing, where
  Silo's stack would carry them through rules. Added to the entry's notes as a third option for the
  first build, recommended over Apple's NAT.
- **`network-before-rules` (#348) and #370's step 2.** The table "Apple's NAT (first) / patched
  gvproxy" gains a column, and its line "Closed: not possible" no longer holds: closed is this card
  with the proxy refusing every host. Open with rules is this card with a list. "Open, not
  checked" is still Apple's NAT, for a person who wants a program that ignores proxies to work.
- **#373, option A's step 2.** #373 puts main on Silo's stack from the first day, because "a host
  or a call is a fact only when every packet passes the boundary", and so makes #359's test 07
  (gvproxy under a macOS computer) blocking. The dead-end card gives the same property without
  gvproxy. The boundary of #347 and #374 then takes its connections from the proxy (a `CONNECT`
  names the host before TLS starts, and the boundary checks that the TLS name matches it) instead
  of from a stack. Test 07 stops blocking option A; it is still needed for the full stack later.
- **`name-lookups` (#356).** In this design the computer makes no name lookup at all: the proxy
  looks names up on the Mac. Lookups as a way to send data out are closed.
- **What the record gets.** Every connection, by host name and port, with bytes each way, from the
  proxy. A connection a program made without the proxy does not exist, so the record misses none.

### Not measured

- Nothing here ran on a Mac. The ARP answer, `AF_VSOCK` in a macOS guest, and which macOS tools
  follow the system proxy are the tool's measurements on macOS 27 guests, read in its code and
  documents. A Mac kit test for it is proposed below.
- Codex, Gemini CLI and the other programs of #367 were not run here (not installed in this
  container). #367 and #369 found that all fall back to HTTP/1.1 and that 8 of 11 trust the system
  store; whether each follows `HTTPS_PROXY` is the next check.
- Throughput through the relay, against #366's gvproxy numbers.

### A Mac kit test to add (#359)

Boot main with the dead-end card and the relay; in main, run `curl`, `git clone`, `npm install`,
`pip install`, `brew install`, `xcodebuild -downloadPlatform iOS`, Claude Code and Codex, each with
the system proxy, the variables, both, and neither; check that each is in the proxy's log or
fails, that `ping`, `dig` and `nc` to the Mac's address fail, and that root in main cannot add a
route that reaches anything.

## 2. The guest's clock after the Mac sleeps

The tool found a guest an hour and a half behind after the Mac slept: a guest with no network
time keeps no time across the host's sleep. Certificates and sign-in tokens then fail. Its
supervisor sets the guest's clock from the Mac's when the guest is ready, every 5 minutes, and at
once after a wake, which it detects by comparing a clock that stops during sleep with one that
does not (the same pair #370 uses for the record: `mach_absolute_time` and
`mach_continuous_time`).

For Silo this is sharper than for the tool. The boundary ends TLS and signs a certificate for each
host on the fly (#347). A certificate whose start time is the Mac's "now" is "not yet valid" to a
guest an hour behind, so every HTTPS connection in main fails after a sleep on battery, and an
agent's run fails with it.

What it changes: `mac-kept-awake` (#348) and #370's step 3 add a clock step to the helper: on wake
and every few minutes, set main's clock from the Mac (with a guest agent, or `sntp` against a time
server the boundary answers). And the boundary dates the certificates it signs a day back, as
inspecting proxies do.

## 3. Putting back a path as root, in a computer an agent controls

#370's put-back (`putting-back-a-path`, #348) writes the paths into the running main with
`sudo tar -x` into `<parent>/.silo-put-back-<id>/`, then moves the current path aside and renames
the copy into place. Everything it touches is in a folder an agent may still control. The tool's
undo, on its own host, guards against each of these, and found them with tests that play the
agent:

- **A link where a folder was.** An agent replaces `<parent>` with a link to `/etc/sudoers.d`
  between the listing and the copy, and root writes there. The undo opens the project folder once
  and reaches every entry from it by name (`openat` with no-follow), so a folder swapped for a link
  is refused, not followed. Silo's step needs the same: a small program in main that does the swap
  through open folders, not `tar` and `mv` through paths.
- **Files the agent locked.** `chflags uchg`, a folder made read-only or unreadable, an ACL such as
  `everyone deny delete`: the move aside fails. The undo opens them up first.
- **A FIFO or socket where a file was.** Reading it blocks the put-back. The undo moves it aside
  without opening it.
- **Something still writing.** An agent left running rewrites the path just after. The tool asks
  to stop the agent first; #370 accepts a lost write and keeps it in the checkpoint of step 3.

The tool also keeps what it replaces (moved into its session folder, never deleted). #370's step 3
checkpoint does the same for Silo.

## 4. Space on APFS: what deleting a checkpoint frees

The tool measures what its clones hold and found two traps, both on the Mac's own APFS volume
where #370 keeps checkpoints as clones of `disk.img`:

- **A local Time Machine snapshot keeps deleted data for up to a day** (sooner when space runs
  short). Until macOS removes it, removing a checkpoint may free nothing.
- **The private size APFS reports (`ATTR_CMNEXT_PRIVATESIZE`) reads 0** for every older file while
  such a snapshot exists. The tool counts from clone ids (`ATTR_CMNEXT_CLONEID`) and blocks instead:
  files of one clone id share their blocks, and a write to a clone gives it a new id.

What it changes: `low-disk` and `space-the-engine-keeps` (#348). The level where Silo pauses
computers must not count on space freed by removing checkpoints in the same minute. Disk shows
"frees once macOS removes its local snapshots" when one exists (`tmutil listlocalsnapshots /`).
#365's free-space floor and reserve file are the defence that does not depend on it.

## 5. Smaller points

- **Which files to show first in the morning.** The tool flags files an agent changed that run
  later, outside the box: git hooks and `core.hooksPath`, `core.fsmonitor` and filters, agents'
  own settings and instructions (`.mcp.json`, `.claude/`, `.codex/`, `opencode.json`, `.cursor/`,
  `CLAUDE.md`, `AGENTS.md`), CI workflows, editor tasks, `.envrc`, build scripts, package
  manifests, Xcode projects and schemes, new executables, and links that point outside. Names are
  matched without regard to case, as the Mac's disk matches them. #370's `silo changes` groups by
  place; this list is a good first group in each repository, for #371's morning screen. Its own
  advice: flags are a reading aid, not a boundary.
- **Two logs for connections.** Refused connections go to one file and allowed ones to another, so
  a computer that sends refused requests as fast as it can cannot push an allowed connection out of
  the record. A long connection (an agent's own to its provider) is written when it opens, about
  once a minute with its bytes so far, and when it ends. For #370's `formats.md`: the chain holds
  both, but a size limit or a thinning must never drop allowed facts to keep refused ones.
- **A macOS guest's account password is in `/etc/kcpassword`** when it logs in by itself, lightly
  obscured and readable by root. Every computer made from one install has the same password, so
  root in one learns all of them. For `fork-identity`: forks of main share it by nature; a fresh
  computer made from the same install image should get a new password at first boot.
- **A gateway's undercounts, which #374 avoids.** The gateway tool prices every Anthropic cache
  write at 1.25 times the input rate (a 1-hour write costs 2 times), does not read
  `usage.iterations`, web searches or long-context rates, and prices an unknown model at a high
  default. #374 reads all of these and refuses an unpriced model. Nothing to change; it confirms
  #374's choices against a tool in use.

## 6. Budgets, from the gateway tool

The gateway tool asks the question #374 asks: how a budget holds against an agent that can spend.
#374 measured more than it did. These are the parts #374 has not tried.

- **One call at a time near the limit.** By default the tool lets one call per task be in flight,
  so the most it can overshoot is one call. #374 found "calls under way finish" goes 35% to 56% over
  with 8 subagents, because 8 calls are under way at the limit. Serializing all the time would slow
  every subagent. A middle way for `what-a-budget-promises`: admit calls in parallel while what is
  left is more than the calls under way could spend at their typical output, and one at a time
  after. Not measured; #374's `run/overshoot.sh` can measure it as a fifth policy.
- **A count of calls where money cannot be counted.** The tool's last line of defence counts an
  event it causes itself (a task start), not dollars a reply reports, because no gap in reading
  usage can undercount it. For Silo: a limit on the number of model calls per computer per night,
  which holds for plan traffic, for Chat Completions streams without `include_usage`, and for a
  host Silo cannot count. A note on `program-that-cannot-be-read` and `budget-as-a-rate`.
- **A ledger that cannot be read refuses.** The tool's rule: when its record of spend cannot be
  read at all, a call is refused; a damaged line makes the total a lower bound and says so; an
  absent ledger is zero, because nothing was spent. A budget's "I don't know" means no.
- **A window on a clock that can jump.** The tool found two failures in a spend window over the
  last 24 hours. A clock moved forward makes all spend look old, so the window admits everything;
  and when the same clock prunes old entries from the file, the jump deletes them for good. A
  second, subtler one: correcting the time of each entry with the same correction as the query
  made the two drift apart with uptime until the window saw nothing. It now measures both on one
  monotonic clock. For `budget-as-a-rate` on a Mac that sleeps: measure windows on the Mac's
  continuous time (`mach_continuous_time`, which counts sleep), never on the wall clock, and never
  prune the record by a wall-clock cutoff.
- **Never sum a figure the computer wrote.** The tool once summed a cost found in a task's log,
  where the agent's own output is copied too; a forged line claiming $999,999 refused every task
  until it aged out. Its spend now comes only from what its gateway counted, and an agent's printed
  cost is shown marked as the agent's. For `how-spend-is-shown`: a figure an agent printed is the
  agent's claim, with the agent as its author, and is never added to Silo's count.
- **Batch routes are refused while a budget is set.** The tool answers `/v1/messages/batches` with
  403 rather than try to count it. #374 did not build batches or `background: true` responses. The
  simplest answer under a budget, until they are counted: refuse them in the provider's shape, as
  #374 refuses an unpriced model.
- **A retry gets a fresh budget.** The tool's retries of a failed task each get a new task budget,
  so three retries of a $2 task can spend $8. Silo's budgets are on a computer and a person, not on
  a run (#374), so a restarted agent shares what is left. Nothing to change.

## What this changes in the plan

Notes added, each "Agent, 25 November 2026":

| Entry | The note |
|---|---|
| `network-stack` | The dead-end card with a proxy as a third option for the first build, with what it gives up and the measurement |
| `low-disk` | Deleting may free nothing while a local Time Machine snapshot exists, and how to count what a checkpoint holds |
| `fork-identity` | The shared account password of computers from one install |
| `what-a-budget-promises` | One call at a time near the limit, as a fifth policy for #374 to measure |
| `budget-as-a-rate` | Windows on the Mac's continuous time; a ledger that cannot be read refuses |
| `program-that-cannot-be-read` | A count of calls as the backstop; batch routes refused under a budget |
| `how-spend-is-shown` | A figure the computer wrote is the agent's claim, never summed |

For entries that exist only in drafts, the change is said above and here:
`network-before-rules` and `mac-kept-awake` (#348) and #370's steps 2 and 3 (sections 1 and 2);
`putting-back-a-path` (#348) and #370's put-back (section 3); `space-the-engine-keeps` (#348,
section 4); #373's step 2 (section 1).

## Sources

- The two tools' repositories, read at their commits of 25 and 8 November 2026: the guest
  network, relay and proxy, the session snapshot, report and undo, the disk-usage code, the threat
  models, and the gateway's admission, metering, pricing and ledger code with its design notes.
- #347, #348, #356, #359, #365, #366, #367, #369, #370, #373 and #374 of this repository.
- Apple: `VZFileHandleNetworkDeviceAttachment`, `VZVirtioSocketDevice`, `getattrlist(2)`
  (`ATTR_CMNEXT_PRIVATESIZE`, `ATTR_CMNEXT_CLONEID`).
