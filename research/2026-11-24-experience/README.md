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
| [`screens/`](screens/README.md) | Static pictures of the phone ask, the phone list, the Mac's morning screen and a Mac notification |
| [`details/onboarding.md`](details/onboarding.md) | How Docker's `sbx`, OrbStack, Tart, Conductor, Claude Code, Codex and others onboard, and bring people back |
| [`details/complaints.md`](details/complaints.md) | 103 complaints, sourced and counted, on onboarding, approvals, unattended runs, phones and lost work |
| [`details/notify-review-cli.md`](details/notify-review-cli.md) | Phone approvals and push, morning review, and command-line conventions in other tools |
| [`details/macos-first-run.md`](details/macos-first-run.md) | Sizes, times and pitfalls of a macOS virtual machine's first run and of a night left running |
| `details/fact-check.md` | A second agent's check of the four design files against the sources and the plan |

## In short

1. **The first agent can work in main about 31 minutes after `brew install`, at 300 Mbps, with 2 of
   those minutes yours.** Apple's restore image for macOS 27.2.1 is 25.6 GB, and it is most of the
   wait: 13 minutes at 300 Mbps, 38 at 100. The install takes about 6 minutes by other tools'
   users, the first start about 2; neither is measured on a Mac yet. Tart downloads images of 24 to
   32 GB, so Silo's download is no larger than Tart's. What Silo can do better is ask for less
   attention: one yes starts the download, then six questions with defaults, all in the first three
   minutes while the download runs, and nothing more until main is ready
   ([`first-run.md`](details/first-run.md)).
2. **Reuse a restore image that is already on the Mac.** Tart keeps them in `~/.tart/cache/IPSWs/`
   under their SHA-256, VirtualBuddy in its own folder, and Apple's CDN gives the SHA-256 before a
   download. A match skips 25.6 GB and brings the first run to about 18 minutes.
3. **Main starts with no `git`.** On a fresh macOS, `git` is a stub that opens a dialog, which nobody
   sees over SSH. The first build has no recipe to install anything (`blank-computer`). Proposed: a
   fixed first-run step that runs the vendors' own installers over SSH (Command Line Tools,
   Homebrew, Claude Code, Codex, tmux), shown before it runs. It becomes the shipped recipe later.
   Running Claude Code's installer unpinned is a first-build exception to
   `claude-code-fetched-by-recipe`: no named version, no checksum, no three-day wait.
4. **The first two things that break a night are not in the plan.** A run started over SSH dies
   when the terminal closes, and main stops when the Mac sleeps. With its lid closed, a MacBook
   sleeps unless it is on power with an external display attached. Proposed: a second SSH entry
   that opens tmux, so `ssh main.keep` attaches to a session that outlives the connection, with each
   agent in its own tmux window. It changes `reaching-a-computer`'s "one named entry per computer".
   Also proposed: a power assertion while main runs on power, a warning about the closed lid, and
   every sleep and wake shown in the morning ([`daily-loop.md`](details/daily-loop.md), gaps 1
   and 2).
5. **An ask nobody answered vanishes.** A hold lasts 90 seconds (`hold-limit-length`), and
   `one-queue` drops an ask whose hold ended. At night every ask ends that way, and the morning
   screen shows nothing of it. Proposed: requests refused as held, not answered, stay on the list,
   grouped, with "allow next time" making a rule proposal, which is how Docker's `sbx` handles a
   blocked host. Also proposed: "away", set by you. While you are away, a request a rule would hold
   is denied at once, with words such as "denied: the user is away until 07:00; not sent", so the
   agent goes on with other work instead of waiting 90 seconds for nobody (gap 4).
6. **With several agents in main, the first build's only way back costs everyone's night.** Forks
   come in the second build, so bringing back a folder one agent deleted means restoring all of
   main and losing what the others did since. Proposed for the first build: `silo look` reads a
   path inside a checkpoint and `silo put-back` copies it into main. Both use what the first build
   already builds: reading checkpoints from outside, and copying files in (gap 6, [`cli.md`](details/cli.md)).
   Putting a path back in main is a partial rewind outside Git, so it challenges
   `fork-from-the-moment` (taken). A checkpoint of now is taken first, so nothing is lost, and the
   restore of all of main that the plan allows is the worse rewind.
7. **The daily failure of an unattended run is silence, not destruction.** Four complaints
   describe runs blocked in silence, and in one issue an agent whose question went unanswered for
   60 seconds "continued without an answer" (anthropics/claude-code #73125, 388 👍). Proposed: a run
   state "waiting on you", and a morning line for the questions an agent went on without. Both are
   read from the model traffic, from outside. `what-silo-does-not-build` (open) lets you answer an
   agent's question from Silo, which is the natural action on that line (gap 3).
8. **The lock-screen ask: Deny is one tap, Allow needs Face ID, and only "Allow once" is offered
   there.** A wrong deny costs a retry; a wrong allow lets something out. Repeats fold into one
   notification, nothing goes to the phone while you are at the Mac, and the push is withdrawn when
   the ask is answered elsewhere. Apple's notification actions can require Face ID
   (`.authenticationRequired`). Duo Verified Push makes a person type a code when it sees a risk,
   against push fatigue. On the Mac's notification, Allow asks for Touch ID or the Mac's password.
   Claude Code's Remote Control skips the phone while you are at the terminal.
9. **Some alerts should interrupt you, and the rest wait for the morning list.** Asks while you
   are not away, a computer left stopped, disk pausing computers and a budget reached interrupt. A new host,
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
| Runs stuck in silence | "No response after 60s — continued without an answer" (#73125) | "waiting on you", the morning list | later |
| Undo that misses what Git misses | Codex `/undo` request, 505 👍 | the whole disk at a checkpoint | first |
| Morning review is archaeology | "what did this thing decide while I wasn't watching" | receipts, the morning screen | later |
| Phone approval missing or flaky | five home-made ntfy tools for one job | asks in Silo's own apps | later |
| Credentials inside sandboxes | sign in again at every restart | sign in once in main; secrets held outside | first, secrets second |
| Irreversible actions elsewhere | force pushes, `terraform destroy` | holds on named requests | third |
| Heavy machines, file-sync bugs | a 10 GB VM bundle, stale shared files | nothing shared from your Mac; idle memory given back | first |

Spend is among the top pains, with the busiest threads in both vendors' trackers, and comes in the
third build. `facts-before-reports` (in force) rightly keeps Silo from showing it from what agent
programs say of themselves. [`plan-map.md`](details/plan-map.md), item 10, proposes two edits to
`needs` that would bring part of it earlier without a report.

## What to rule on

Every recommendation, with the entry it touches, is in [`plan-map.md`](details/plan-map.md). The
ones that change the first build, in the order they matter:

1. `silo look` and `silo put-back` in the first build (new features in `going-back`; a challenge
   to `fork-from-the-moment`, taken).
2. Main set up for agents at the first run (new decision in `agent-programs`; touches
   `blank-computer`).
3. A run outlives its terminal, and the Mac is kept awake while main runs (one new decision, one new
   feature; the decision changes `reaching-a-computer`).
4. The first build says main's network is open with no boundary, in the first run and in
   `silo status` (touches `nothing-to-set-up`, `safe-fresh-install`). If the first build runs main
   on Apple's NAT, as first-build.md proposes as a fallback when gvproxy is slow (Mac test 5), main
   reaches everything.
5. A restore image already on the Mac is reused (edit to `computer-image`).

The ones for later builds: requests refused as held, not answered, stay on the list, and "away"
(edits to `one-queue`, new decision
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
- Whether Apple's framework holds the Mac awake while a virtual machine runs: no source says that
  it does. It is to check with `pmset -g assertions` on a Mac.
- Reddit could not be read by the agents; stories from it appear only where an article quotes them.
- The proposed phone and Mac screens are pictures, not tested with anyone.
