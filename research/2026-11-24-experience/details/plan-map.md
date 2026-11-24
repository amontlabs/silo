# The experience mapped to the plan

Research of 24 November 2026, by an agent, for polarzero. Every step of the first run and the daily
loop, with the plan entries it rests on, and what this research proposes for each. Nothing under
`plan/` was edited. "Taken" means polarzero gave the entry a verdict, so a recommendation against
it is a challenge. "Open" means it has an answer and no verdict, so a recommendation is a proposal.
"New" is an entry the plan does not have. The states were read from `plan/` at `1b8f1917a`.

## Step by step

| Step | Entries it rests on | State | What this research adds |
|---|---|---|---|
| Install, checks | `first-run-failure` | open | Fits as written. The checks print in four lines ([`first-run.md`](first-run.md)) |
| Download | `computer-image`, `first-run-time-macos` | open | Reuse a restore image another tool already downloaded, checked against Apple's SHA-256; show size and time before the yes |
| Questions | `first-run-wait`, `first-run-offers-to-bring-in`, `moving-in-on-macos` | open | At most eight questions, all in the first minute, each with a default |
| Main set up for agents | `blank-computer`, `main-from-the-mac-or-a-recipe`, `unmodified-agent-programs` | open | New first-build step: the vendors' installers run over SSH, shown first |
| Keep the Mac awake | none | new | A power assertion while main runs on power; sleeps and wakes shown in the morning |
| Way in | `reaching-a-computer`, `computer-reached-over-ssh` | open | A second SSH entry that opens tmux, so a run outlives the terminal |
| Sign in | `logins`, `sign-in-from-a-computer`, `where-the-token-lands` | open | Codex's sign-in needs port 1455 or a ChatGPT setting; the first run says which |
| Network on the first day | `nothing-to-set-up`, `safe-fresh-install`, `network-rules-from-use` | open, open, taken | The first build says the network is open with no boundary; later, open plus proposed rules is the first-day default |
| Starting agents | `full-access-inside`, `several-agents-in-main` | taken | Fits |
| Before bed | `now-place`, `automatic-checkpoints` | open, taken | A checkpoint by hand with a note is never thinned by age |
| An agent waiting on you | `how-each-run-is-going`, `now-place` | open | A sixth state, "waiting on you", and the questions an agent went on without |
| A hold at night | `hold-limit`, `hold-limit-length`, `hold-sentence`, `one-queue` | taken, open, open, open | Refused asks stay on the list; "away" refuses at once with words that say so |
| Reaching your phone | `ask-reaches-you`, `phone-app`, `forwarding-to-a-phone` | open | Mac notifications first; the smallest phone app answers asks and sets you away |
| Which alerts interrupt | `alert-only-on-a-change`, `new-host-alert-is-a-rule`, `low-disk` | open | New decision: two columns, interrupt and morning |
| Morning screen | `first-screen`, `one-queue`, `app-as-text`, `silo-command-reads-the-app` | taken, open, open, taken | "Since you last looked"; sleeps and restarts; refused asks; runs waiting on you |
| What changed | `file-changes-in-the-record`, `record-queried-with-ordinary-tools` | open | Text grouped by folder by default, JSON with `--json`; macOS's own writes folded |
| Undo one agent's damage | `putting-back-one-run`, `kinds-of-other-actions`, `fork-from-the-moment` | open, open, taken | New first-build feature: put back named paths from a checkpoint, and look inside one |
| Undo everything | `restore-to-a-checkpoint`, `restore-saves-first`, `thinning-checkpoints` | open | The restore prompt counts what it leaves; the checkpoint saved first is kept until set aside |
| Main stopped in the night | `computer-stops-by-itself`, `engine-matches-wanted` | open | Fits; the morning line names the runs that did not come back and the command that resumes each |
| Spend | `spend-per-conversation`, `model-traffic-read` | open | Nothing in the first build; see "Order" below |

## What hurts the experience as the plan stands

Ranked by how much of the daily loop each one breaks.

### 1. An unanswered ask leaves no trace you will see (`one-queue`, `hold-limit-length`, open)

At night every hold ends unanswered after 90 seconds, and `one-queue` removes the ask from the
list. In the morning, nothing on the first screen says an agent was blocked. The agent may have
found another way, which is the case a hold exists to catch. Proposed in [`daily-loop.md`](daily-loop.md),
gap 4: refused asks stay, grouped, with "allow next time" making a rule proposal; and "away", set
by you, so that a hold refuses at once with words that say why. Docker's `sbx` already works the
first way.

### 2. The first build's only way back costs every other agent's night (`restore-to-a-checkpoint`, open, with `several-agents-in-main`, taken)

Several agents in main is the normal case, and forks are in the second build. A whole restore is
the only way to bring back a folder one agent deleted, and it throws away what the others did since.
Proposed: `silo look` and `silo put-back` in the first build, from what it already builds
([`daily-loop.md`](daily-loop.md), gap 6; [`cli.md`](cli.md)).

### 3. A run dies when its terminal closes, and main stops when the Mac sleeps (no entry)

Both are the first two things that go wrong for a person who leaves agents overnight, and the
plan has neither. Proposed: a second SSH entry that opens tmux, and a power assertion while main
runs ([`daily-loop.md`](daily-loop.md), gaps 1 and 2). The second also gives the morning one line
that explains a quiet night.

### 4. Main starts with no `git` (`blank-computer`, open, with recipes in the third build)

A fresh macOS has a `git` that opens a dialog nobody sees over SSH. The first build has no recipe
to install anything. Proposed: a fixed first-run step that runs the vendors' installers over SSH,
shown first, which becomes the shipped recipe later ([`first-run.md`](first-run.md)).

### 5. The phone comes last (`order-of-work`, open; `phone-app` has no needs)

`order-of-work` says "the phone app come[s] later". `merges-and-deploys-held` says you answer from
the phone, and `ask-reaches-you` says only Silo's own apps answer. Until the phone app exists, a
held merge is answered at the Mac within 90 seconds or not at all. Proposed: a Mac notification
with Allow and Deny right after `held-requests-and-asks`, and a phone app whose first version does
three things only: the list, an answer, and away.

### 6. Every alert can interrupt (`alert-only-on-a-change`, open)

The entry merges alerts and repeats a lasting one only once. It says nothing about which ones may
wake you. With `new-host-alert-is-a-rule` switched on in an open main, the first week brings dozens
of new hosts. Proposed: a new decision with two columns, interrupt and morning, and nothing pushed
to the phone while you are at the Mac ([`daily-loop.md`](daily-loop.md), gap 5).

### 7. A run waiting on you is "idle" (`how-each-run-is-going`, open)

The user reports say the daily failure is a run blocked in silence, and one that guessed when its
question timed out. Both are visible from the traffic. Proposed: the state "waiting on you", and
the questions a run went on without.

### 8. The first run has no limit on its questions (`first-run-wait`, open)

Each later feature will want a say at the first run. Proposed: at most eight, each with a default,
all before the download ends.

### 9. "Nothing is open" cannot be said by the first build (`nothing-to-set-up`, `safe-fresh-install`, open)

The first build runs main on Apple's NAT, so main reaches everything. The first run and
`silo status` have to say the network is open with no boundary, or they promise one that is not
there.

### 10. Spend, the first pain users name, comes third (`order-of-work`, open)

The busiest threads in both vendors' trackers are about spend and limits (Claude Code #16157, 697
👍 and 1,499 comments; [`complaints.md`](complaints.md), theme 3). The first build shows none of it,
and `facts-before-reports` (in force) rules out showing it from what the agent program writes about
itself, since the boundary will see it. That is the right principle; the cost is that the most
asked-for number arrives with `model-traffic-read`, after 77 open decisions. Two things lower the
cost without a report: the plan-limit headers Silo reads at the boundary are few and need no
conversation tracking (`plan-limit-reached`), so they could come before the rest of the traffic
features; and `budget-on-a-computer` needs only spend per computer, not per conversation.

## Decisions in force that this design follows, and what they cost

These stay as they are. Each has a cost in the daily loop, written here so that it is weighed and
not met by surprise.

- **`device-not-shared`** (taken). Your editor works on main's files over SSH (VS Code Remote,
  Zed, Cursor), not on files on your Mac. It removes the file-sync bugs users report with shared
  folders ([`complaints.md`](complaints.md), theme 1), and it means every edit you make by hand is
  in main too.
- **`asks-are-native`** (taken). An ask is never a prompt in the agent's terminal, so it cannot be
  answered where the agent is. It is answered in Silo, which in the first build is the command line.
- **`hold-limit`** (taken). A hold always ends, so a night of holds ends as refusals. The "away"
  proposal makes that end immediate and readable.
- **`full-access-inside`** (taken). Nothing asks the agent for approval inside, which is what the
  third pain of the user reports asks for: per-command prompts are accepted 93% of the time anyway
  ([`complaints.md`](complaints.md), theme 2).

## New entries, in one list

| Part | Kind | Id (placeholder) | Says |
|---|---|---|---|
| agent-programs | decision | `run-outlives-the-terminal` | Main ships tmux, and Silo writes a second SSH entry that opens it |
| computers | feature | `mac-kept-awake` | While main runs on power, the Mac does not sleep on its own; sleeps and wakes are facts in the record |
| going-back | feature | `paths-put-back` | Named paths are copied from a checkpoint into the running computer, after a checkpoint of now |
| going-back | feature | `looking-inside-a-checkpoint` | A checkpoint's folders and files read from outside, read only (record-ux 4.11) |
| boundary | decision | `while-you-are-away` | While you say you are away, a hold refuses at once, with words that say so |
| views | decision | `alerts-that-interrupt` | Two columns, interrupt and morning, and nothing to the phone while you are at the Mac |
| views | decision | `since-you-last-looked` | One line of counts since the first screen was last shown (record-ux 4.1) |
| views | feature | `asks-as-mac-notifications` | An ask with Allow and Deny in a macOS notification |
| agent-programs | decision | `first-run-sets-up-main` | For the first build, the vendors' installers run over SSH, shown first |

Edits proposed to open entries: `one-queue` (refused asks stay), `first-run-wait` (eight
questions), `how-each-run-is-going` and `now-place` (waiting on you), `thinning-checkpoints` (a
checkpoint taken by hand with a note, and the one saved before a restore, are kept until set
aside), `computer-image` (reuse a restore image already on the Mac), `phone-app` (its first
version).
