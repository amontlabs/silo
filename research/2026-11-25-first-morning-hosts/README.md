# The first morning's new hosts

An agent's research, 25 November 2026, for polarzero. It closes #382's gap 4: on the first night
every host is new, so the first morning opens on a pile of "host seen for the first time" alerts,
and the paste site an agent sent a log to is one of them. #382 estimated about twenty and proposed
two fixes: hosts the first run's installers reached count as seen, and the rest merge into one
line. Neither was measured. This page measures both, and finds a third fix that matters more.

## What was found

1. **The first morning holds about 57 first-seen hosts, not twenty.** 54 to 60 over 20 runs. The
   night is #382's (Claude Code on a Node app, Codex on an API, a log sent to paste.rs), rebuilt
   from connections that were measured. Following links in documentation brings most of them. A
   second night of the same work, with the first night's hosts seen, holds about 11.
2. **Counting the installers' hosts as seen removes only 6.** The first run's installers reach 12
   hosts; 6 of them come back during the night. The hosts each agent program's vendor documents
   remove 9 more. #350's shipped sets of registries, download hosts and documentation sites bring
   the morning to about 21.
3. **Ordering the merged line by the bytes sent to each host puts the paste site first, every
   time.** In all 20 runs and all four ways of counting hosts as seen. In alphabetical order it is
   9th to 35th.
4. **A host that was sent more than it sent back stands apart.** Each of the 49 hosts measured
   here (the setup downloads and a documentation walk) received more than three times what
   it was sent. The paste upload sent 1,750 times what it received.
5. **A host counted as seen still carries data out.** After the first run, github.com,
   api.github.com, ghcr.io and storage.googleapis.com are seen, and anyone with an account can
   upload to each. A log sent to a gist raises no first-seen alert in any of the four ways.

So the first morning should show one line for the night's first-seen hosts, with its count, the
hosts ordered by bytes sent, and a first-seen host that was sent more than it sent back on a line
of its own:

```text
Sent to a host seen for the first time: paste.rs · 2.1 MB sent · 1 kB back       02:47
Hosts seen for the first time: 49  (way B; by bytes sent: console.prisma.io, stripe.com, …)   silo network
```

| Way of counting hosts as seen | First-seen hosts (mean over 20 runs) | Registrable domains | paste.rs by bytes sent | paste.rs alphabetically |
|---|---|---|---|---|
| A. every host is new (`new-host-raises-an-alert` as it reads) | 56.7 (54 to 60) | 43.3 | 1st | 29th to 35th |
| B. the first run's installers' hosts are seen | 50.7 (48 to 54) | 40.9 | 1st | 25th to 31st |
| C. B, plus the hosts each agent program's vendor documents | 42.1 (39 to 45) | 33.3 | 1st | 20th to 25th |
| D. C, plus #350's registries, download and documentation sets | 20.9 (17 to 25) | 18.5 | 1st | 9th to 14th |

Recommended: B and C, the merged line ordered by bytes sent, and the line of its own. Not D's
documentation set: #350 chose it from popular sites, its walk is popular sites by construction, and
it includes shared hosts.

## What it changes in the plan

Notes were added, as "Agent, 25 November 2026". No verdict, answer or list was changed.

| Entry | What the note says |
|---|---|
| `new-host-alert-is-a-rule` (open) | The numbers above; the merged line ordered by bytes sent; a first-seen host sent more than it sent back on its own line; shipping the rule narrowed by each program's documented hosts is polarzero's call; seen hosts that accept uploads |
| `hosts-already-seen` (open) | The first run's 12 hosts count as seen: their alerts come on the first run's screen, before any agent ran, so `new-host-raises-an-alert` holds as it reads |
| `alert-only-on-a-change` (open) | "Close together" for one night's first-seen hosts: merge until you next look |

## For polarzero to rule

- **Ship the new-host rule narrowed by each agent program's documented hosts** (way C). It
  changes the text `new-host-alert-is-a-rule` proposes ("switched on until you narrow it"), since
  Silo would narrow it first. It removes 9 alerts from the first morning, all of them hosts the
  vendor says its program reaches. Recommended.
- **A first-seen host that was sent more than it sent back gets an alert line of its own.** A new
  kind of line beside the merged one. Recommended.

## The research at a glance

The first morning holds about 57 first-seen hosts, not twenty; counting the installers' hosts as
seen removes 6, and ordering the merged line by bytes sent puts the paste site first in every run.

## How it was measured

| File | What it is |
|---|---|
| [`rig/hostlog.py`](rig/hostlog.py) | A proxy standing in for the boundary's connection log: host, port, times and bytes each way of each connection |
| [`data/setup.jsonl`](data/setup.jsonl) | The first run's installer downloads, fetched through it on 25 November 2026: Claude Code 2.1.332's installer files for macOS arm64 from downloads.claude.ai, Homebrew's installer, repository and API, tmux and its four dependencies from Homebrew's bottles, Codex 0.172.0's cask, and the provider check |
| [`rig/fetch_walk.py`](rig/fetch_walk.py), [`data/docs-walk-seed0.jsonl`](data/docs-walk-seed0.jsonl) | #350's 20 documentation pages and two followed links each (seed 0), fetched for their bytes each way |
| [`firstnight.py`](firstnight.py) | Builds the night and counts the first morning under each way; [`results.txt`](results.txt) is its output |

The night is built from: the setup log above; #350's `npm install` of a 21-dependency web app with
`playwright install`, and its `pip install` of 20 packages with models and a browser (both logged
connection by connection in `research/2026-11-24-gap-fixes/prototypes/holds/data/`); #350's
documentation walk over 20 seeds; Codex's four hosts at start (#371, measured); Claude Code's
hosts as its network page lists them for a Max sign-in (code.claude.com/docs/en/network-config,
read 25 November 2026); a `git push`; and the paste upload of #373's scenario.

## What was not verified

- The installers were not run. The classifier of this session refused to run downloaded
  installers, so their downloads were fetched by hand from the URLs the installers and Homebrew's
  API name. An installer that reaches another host at run time (Homebrew's analytics, for one) is
  missed.
- Nothing ran on a Mac. Apple's two software update hosts for the Command Line Tools are taken from
  Apple's catalogue, not measured. macOS's own traffic in main through the system proxy (updates,
  time, certificate checks, iCloud) is not counted at all, and likely adds hosts to the first
  morning. A Mac kit test could count the hosts main's macOS reaches in its first day with no agent.
- Claude Code's hosts at start are its documentation's, not a measurement; no Claude Code ran here.
- Bytes sent for the npm, pip and documentation connections are the median of the measured setup
  connections (864 bytes), since #350's logs hold no bytes. Bytes each way were measured for the
  setup and for one documentation walk.
- The paste upload was not sent anywhere.
- Ordering by bytes sent ranks a legitimate first upload first too: a first `git push` to a new
  host, or a first deploy. The night here has neither.

## Sources

- #382, `research/2026-11-25-first-run-option-a/` (gap 4, the first screen).
- #350, `research/2026-11-24-gap-fixes/8-holds-and-new-hosts.md` (G1) and its data.
- #371, `research/2026-11-24-morning-review/README.md` (Codex's four hosts at start).
- Claude Code's network requirements: https://code.claude.com/docs/en/network-config (read).
- The Claude Code installer, https://claude.ai/install.sh, and Homebrew's,
  https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh (read, not run).
