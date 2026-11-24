# The first run and the daily loop: what using Silo is like

Research of 24 November 2026, by agents, for polarzero. It writes out, as transcripts and screen
sketches, what a person does and sees from installing Silo to the first agent working in main, and
then through one day and one night: starting agents, leaving them, the morning, going back, and
answering asks. It maps each step to the plan and names the entries that make the experience worse
than it has to be.

Nothing here is a decision, and nothing under `plan/` was edited. It builds on the earlier research
of the same day: the smallest first build (`research/2026-11-24-first-build.md`), the second
opinions on its decisions (`research/2026-11-24-first-build-decisions/`), and the sketches of the
morning screen, a receipt and the Timeline in `research/2026-11-24-record-and-spend/details/record-ux.md`,
which this research does not repeat.

| File | Covers |
|---|---|
| [`details/first-run.md`](details/first-run.md) | The first run as a transcript, its minutes and its questions, sign-in inside main, and the failures as they read |
| [`details/daily-loop.md`](details/daily-loop.md) | One day and night, step by step, with seven gaps; the lock-screen ask; which alerts interrupt |
| [`details/cli.md`](details/cli.md) | The `silo` command: every command, its output on one real case, and the conventions they share |
| [`details/plan-map.md`](details/plan-map.md) | Each step against its plan entries, the ten entries that hurt the experience, and the new entries proposed |
| [`screens/`](screens/) | Static pictures of the phone ask, the phone list, the Mac's morning screen and a Mac notification |
| [`details/onboarding.md`](details/onboarding.md) | How Docker's `sbx`, OrbStack, Tart, Conductor, Claude Code, Codex and others onboard, and bring people back |
| [`details/complaints.md`](details/complaints.md) | 103 user reports, sourced and counted, on onboarding, approvals, unattended runs, phones and lost work |
| [`details/notify-review-cli.md`](details/notify-review-cli.md) | Phone approvals and push, morning review, and command-line conventions in other tools |
| [`details/macos-first-run.md`](details/macos-first-run.md) | Sizes, times and pitfalls of a macOS virtual machine's first run and of a night left running |
| [`details/fact-check.md`](details/fact-check.md) | A second agent's check of the four design files against the sources and the plan |

## In short

1. **The first agent can work in main about 31 minutes after `brew install`, at 300 Mbps, with 2 of
   those minutes yours.** Apple's restore image for macOS 27.2.1 is 25.6 GB, and it is most of the
   wait: 13 minutes at 300 Mbps, 38 at 100. The install takes about 6 minutes by other tools'
   reports, the first start about 2. Tart, the fastest way to a macOS virtual machine today,
   downloads images of 24 to 32 GB, so Silo is not slower than it. What Silo can do better is ask
   for less attention: one yes starts the download, then six questions with defaults, all in the
   first minute, and nothing more until main is ready ([`first-run.md`](details/first-run.md)).
2. **Reuse a restore image that is already on the Mac.** Tart keeps them in `~/.tart/cache/IPSWs/`
   under their SHA-256, VirtualBuddy in its own folder, and Apple's CDN gives the SHA-256 before a
   download. A match skips 25.6 GB and brings the first run to about 18 minutes.
3. **Main starts with no `git`.** On a fresh macOS, `git` is a stub that opens a dialog, which nobody
   sees over SSH. The first build has no recipe to install anything (`blank-computer`). Proposed: a
   fixed first-run step that runs the vendors' own installers over SSH (Command Line Tools,
   Homebrew, Claude Code, Codex, tmux), shown before it runs. It becomes the shipped recipe later.
4. **The first two things that break a night are not in the plan.** A run started over SSH dies
   when the terminal closes, and main stops when the Mac sleeps, which a MacBook with its lid closed
   on battery always does. Proposed: a second SSH entry that opens tmux, so `ssh main.keep` gives a
   shell that outlives the connection, and a power assertion while main runs on power, with every
   sleep and wake shown in the morning ([`daily-loop.md`](details/daily-loop.md), gaps 1 and 2).
5. **An ask nobody answered vanishes.** A hold lasts 90 seconds (`hold-limit-length`), and
   `one-queue` drops an ask whose hold ended. At night every ask ends that way, and the morning
   screen shows nothing of it. Proposed: refused asks stay on the list, grouped, with "allow next
   time" making a rule proposal, which is how Docker's `sbx` handles a blocked host; and "away", set
   by you, while which a hold refuses at once with words that tell the agent you are away, so it
   goes on with other work instead of waiting 90 seconds for nobody (gap 4).
6. **With several agents in main, the first build's only way back costs everyone's night.** Forks
   come in the second build, so bringing back a folder one agent deleted means restoring all of
   main and losing what the others did since. Proposed for the first build: `silo look` reads a
   path inside a checkpoint and `silo put-back` copies it into main. Both use what the first build
   already builds: reading checkpoints from outside, and copying files in (gap 6, [`cli.md`](details/cli.md)).
7. **The daily failure of an unattended run is silence, not destruction.** In the user reports, a
   Claude Code session waited on a prompt for 8.5 hours unnoticed, and an agent whose question went
   unanswered for 60 seconds "continued without an answer" (anthropics/claude-code #73125, 388 👍).
   Proposed: a run state "waiting on you", and a morning line for the questions an agent went on
   without. Both are read from the model traffic, from outside (gap 3).
8. **The lock-screen ask: Deny is one tap, Allow needs Face ID, and only "Allow once" is offered
   there.** A wrong deny costs a retry; a wrong allow lets something out. Repeats fold into one
   notification, nothing goes to the phone while you are at the Mac, and the push is withdrawn when
   the ask is answered elsewhere. Apple's notification actions can require Face ID
   (`.authenticationRequired`); Duo and 1Password make a person type a code for the same reason,
   push fatigue. Claude Code's Remote Control skips the phone while you are at the terminal.
9. **Alerts need two columns: interrupt now, and wait for the morning.** Asks while you are not
   away, a computer left stopped, disk pausing computers and a budget reached interrupt. A new host,
   a flagged deletion whose checkpoint is kept, a stuck run and an ended run wait. No entry decides
   this yet (`alert-only-on-a-change` merges alerts but does not rank them).
10. **The `silo` command answers "what needs me" with no arguments**, prints text for a person and
    JSON lines with `--json`, ends each output with the next command, names checkpoints by their
    time (`silo restore 02:40`), and exits 3 when something needs you. Every command and its output
    for one night is in [`cli.md`](details/cli.md).

## What people struggle with, ranked

From [`complaints.md`](details/complaints.md), by how often and how strongly each appears in
GitHub issues, Hacker News and posts of 2025 and 2026:

| Pain | Example | What answers it in Silo | Build |
|---|---|---|---|
| Spend nobody sees, limits that drain | Claude Code #16157, 697 👍, 1,499 comments | spend and budgets at the boundary | third |
| Agents deleting files, no way back | `rm -rf ~/`, `.git` deleted by Codex | checkpoints, restore, put back | first |
| Per-command approvals | "users accept 93% of them anyway" (Anthropic) | full access inside, holds only at the boundary | first, holds third |
| Runs stuck in silence | 8.5 hours on an unanswered prompt | "waiting on you", the morning list | second |
| Undo that misses what Git misses | Codex `/undo` request, 505 👍 | the whole disk at a checkpoint | first |
| Morning review is archaeology | "what did this thing decide while I wasn't watching" | receipts, the morning screen | second |
| Phone approval missing or flaky | five home-made ntfy tools for one job | asks in Silo's own apps | later |
| Credentials inside sandboxes | sign in again at every restart | sign in once in main; secrets held outside | first, secrets second |
| Irreversible actions elsewhere | force pushes, `terraform destroy` | holds on named requests | third |
| Heavy machines, file-sync bugs | a 10 GB VM bundle, stale shared files | nothing shared from your Mac; idle memory given back | first |

Spend is the first pain and comes in the third build. `facts-before-reports` (in force) rightly
keeps Silo from showing it from what agent programs say of themselves. [`plan-map.md`](details/plan-map.md)
names two ways to bring part of it earlier without a report.

## What to rule on

Every recommendation, with the entry it touches, is in [`plan-map.md`](details/plan-map.md). The
ones that change the first build, in the order they matter:

1. `silo look` and `silo put-back` in the first build (new features in `going-back`).
2. Main set up for agents at the first run (new decision in `agent-programs`; touches
   `blank-computer`).
3. A run outlives its terminal, and the Mac is kept awake while main runs (one new decision, one new
   feature).
4. The first build says main's network is open with no boundary, in the first run and in
   `silo status` (touches `nothing-to-set-up`, `safe-fresh-install`).
5. A restore image already on the Mac is reused (edit to `computer-image`).

The ones for later builds: refused asks stay and "away" (edits to `one-queue`, new decision
`while-you-are-away`), which alerts interrupt (new decision), "waiting on you" (edits to
`how-each-run-is-going` and `now-place`), and a first phone app that does three things only (edit
to `phone-app`).

## What was not verified

- No step was timed on a Mac. The install and first-start times are other tools' users' reports;
  the download times are computed from the image size.
- Whether Claude Code's login stays over SSH in main is Mac test 2 of the first-build decisions.
  The first run's last screen depends on it.
- Whether Apple's provisioning accepts a user name equal to the Mac's (FB23241619) is unverified
  either way.
- Reddit could not be read by the agents; stories from it appear only where an article quotes them.
- The proposed phone and Mac screens are pictures, not tested with anyone.
