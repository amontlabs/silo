# The first build, step by step

An agent's plan, 24 November 2026, for the 17 features of the first build
(`research/2026-11-24-first-build.md`). It cuts them into six steps. After each step you can do
something you could not do before, or a risk is settled. It gives no time and no date, as
`AGENTS.md` asks; each step ends on a check, not on a day.

The steps follow `order.json` as this PR leaves it. A step starts when the features it builds are
taken and the decisions they need are taken, and when the Mac tests named for it have passed.

## The steps at a glance

| Step | Builds | What you can do after it | Mac tests first |
|---|---|---|---|
| 0. Mac answers | Nothing | Nothing new; six decisions can be ruled with numbers | #359's `quick`, then `00`, `01`, `10`, `04`, `02` |
| 1. The engine, empty | `access-model-checked`, `engine-program-and-clients`, `silo-command` | `silo status` answers from a running engine that survives a restart | `F_FULLFSYNC` cost |
| 2. Main runs | `computers-main-on-a-mac`, `engine-matches-wanted`, `agent-programs-run-unmodified`, `agent-programs-reach-every-computer`, `computers-stopped-and-paused` | `ssh main` into a macOS computer with Claude Code and Codex in it | First run, sign-in over SSH, gvproxy, launchd |
| 3. A night | `agents-work-through-the-night`, `engine-outlived-by-computers` | Start agents at night, close the terminal, find them working in the morning | Sleep and power assertions |
| 4. The way back | `checkpoints-taken-on-their-own`, `thinning-old-checkpoints`, `restore-to-a-checkpoint` | Put main back to any checkpoint of the last 24 hours | APFS clone stalls, clone of a running disk |
| 5. The record | `record-of-a-computer`, `file-changes-in-the-record`, `a-path-from-a-checkpoint`, `files-imported-from-your-mac` | `silo changes`, `silo look`, `silo put-back` | What macOS writes by itself, reader on a large disk |

Step 4 does not need step 3, and can be built beside it. Step 5 needs step 4.

## Step 0: what only a Mac can answer

Your Mac updates to macOS 27, then runs #359's kit in the order its README gives. The results go
in the notes of the decisions they settle, as #347 did for the Linux runs.

| Result | Decides |
|---|---|
| Provisioning with your own user name works or fails | `mac-silo-needs`, `agent-programs-moving-in` |
| Claude Code stays signed in over SSH, after a restart and after a restore | Whether step 3's night works at all |
| How long a write in main waits while a clone is taken, after 10, 100 and 1,000 checkpoints | `checkpoint-per-system`, and whether step 4 needs a mitigation from #352 (file 07) |
| A clone of a running disk is a moment in time, in which attachment mode | `checkpoint-per-system`: pause or not |
| `F_FULLFSYNC` cost, grouped and not | `engine-saved-state`, the engine's latency target |
| gvproxy with a macOS guest, with its token and without its map to 127.0.0.1 | `network-stack`, `network-before-rules` |

Risk: the Swift helper of the kit was never compiled. Expect a fix on its first run.

## Step 1: the engine, empty

The engine program in Zig, its saved state and its client, with the access model first, as
`model-of-access-first` asks. No computer yet. `silo` talks to it over a Unix socket.

Done when:

- the access model passes its test on every sequence of calls up to its bound;
- the core runs under its simulator (`core-simulator`) with crashes, and a repeated call does
  nothing twice;
- killing the engine with `kill -9` in the middle of a call and starting it again gives the same
  state, on a Mac, with `F_FULLFSYNC`;
- `silo status` and `silo status --json` print it.

Risks:

- Zig's `fsync` is not durable on a Mac (#352, file 02). A test that passes on Linux proves
  nothing here, so this step's crash test runs on a Mac.
- The four entries `numbered-events`, `call-ids`, `formats-carry-versions` and `engine-saved-state`
  assume things of each other. If they are ruled apart, step 1 is built twice.

## Step 2: main runs

The Swift helper that drives Apple's framework, one per computer. The first run: the check of the
Mac, the download, the install, the provisioned first start, the list of what goes in main, the
SSH entries. The network on gvproxy, or on Apple's NAT if step 0 said so, shown as open.

Done when:

- a Mac with nothing of Silo on it reaches `ssh main` with Claude Code and Codex installed, with
  you answering only the first run's questions;
- a Mac on macOS 26 is refused before anything is downloaded;
- `silo stop` ends main within its limit, and `silo start` brings it back;
- after a restart of the Mac and your login, main comes back as you left it;
- from inside main, nothing reaches the engine's socket, your Mac's 127.0.0.1 or gvproxy's API.

Risks:

- Provisioning refuses your own user name (FB23241619). Fallback in #352 (file 10): a setup user.
- Apple allows two macOS computers at once, shared with Tart, UTM and any other app. A place can
  leak after a stop (FB22967193). Silo must report a refused start as a fact, not retry forever.
- The download is 25.6 GB. A first run cut off must go on where it stopped
  (`first-run-failure`).

## Step 3: a night

`ssh main.keep` and tmux; the power assertion; sleeps and wakes written down; a helper per computer
so an engine crash leaves main running.

Done when:

- an agent started in `main.keep` is still at work after the terminal is closed and the Wi-Fi
  drops for a minute;
- with the Mac on power and the lid open, an idle night passes with no sleep;
- killing the engine during the night leaves main running, and the engine finds it again when it
  starts.

Risk: a closed MacBook sleeps whatever Silo does, unless an external display is attached. Silo can
only say so.

## Step 4: the way back

Checkpoints by APFS clone every few minutes and at each start and stop; thinning that keeps every
checkpoint of the last 24 hours; the reserve file; restore that stops main, saves, puts the disk
back and starts it.

Done when:

- the longest wait of a write in main during a checkpoint stays under its target (#352 proposes
  100 ms, which is polarzero's number to set), measured after a week of checkpoints;
- a restore to a checkpoint taken during `npm install` boots, and `claude --resume` shows the
  conversation up to that moment;
- filling the volume does not stop the engine: it stops checkpoints, alerts, and can still restore;
- the save made before a restore is never thinned while it is the only copy of what the restore
  left.

Risks:

- Clone stalls grow with the pieces checkpoints leave in the image (#347, #352). If the APFS test of
  step 0 is bad, the fallbacks in #352 (file 07) are fewer checkpoints, a layer per boot, or copying
  the image to unfragment it at night. Each changes `automatic-checkpoints`' promise of "nobody has
  to remember" in a way polarzero should rule on.
- Restore brings back an old login file: Codex documents that as a reason to sign in again. The
  first build says so plainly.

## Step 5: the record

The reader of main's APFS disk, Silo's own, run apart from the engine with no right to write the
record; the comparison of each checkpoint with the one its disk came from; file facts in the record
with a gap when a disk cannot be read; `silo changes`, `silo look`, `silo put-back`; files imported
from your Mac.

Done when:

- the reader is fuzzed, as `fuzz-untrusted-parsers` asks, and refuses any disk another program
  could read differently;
- on main after a night, `silo changes` matches a full walk of both checkpoints, and costs what
  changed, not what exists;
- an agent that turns FileVault on inside main produces a gap, never "nothing changed";
- `silo put-back` brings back a deleted folder while another agent keeps working, and the record
  says who put back what;
- by default `silo changes` hides what macOS writes by itself, from the list step 0's idle night
  measured.

Risks:

- The 250-line prototype of #352 (file 11) has not run on a disk of millions of files. The
  comparison may need the blocks the Mac saw written (`F_LOG2PHYS_EXT`), which is a Mac test too.
- A half-written APFS checkpoint in a clone of a running disk is normal; the reader falls back to
  the one before, and how often that happens is step 0's test 17.

## What the first build does not promise

Said in the first run and in `silo status`, so nobody reads more into it:

- Checkpoints are not backups: they live on the same volume as main.
- What an agent does outside main, through a token or an API, is not undone.
- Main's network is open, and nothing is checked at its edge yet.
- After a restore you may have to sign in again, and the conversation of the hours undone is only in
  the checkpoint saved before it.
