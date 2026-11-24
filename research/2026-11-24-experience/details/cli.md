# The `silo` command: every command, and what it prints

Research of 24 November 2026, by an agent, for polarzero. A proposal for the command set of the
`silo` command, with the output of each command written out for one real case. Nothing here is a
decision, and nothing under `plan/` was edited. Plan entries are named by id; `plan show <id>`
prints each one.

The words are the plan's, some still proposed in open decisions: computer, main, fork, checkpoint,
restore, drop, bin, pause, stop, record, fact, report, opinion, rule, precaution, alert, ask, hold,
run, receipt. Of these, main, bin, record, fact, report, opinion, rule, precaution and run are in
`words.json`; the others are proposed in `meanings-for-computers`, `meanings-for-the-boundary`,
`meanings-for-what-silo-does` and `what-a-receipt-is`. Where a command needs a word the plan does
not have, the line says so.

## How every command behaves

These conventions come from what works in `git`, `gh`, `tailscale`, `fly` and `orb`, and from
[clig.dev](https://clig.dev/). The sources are in [`notify-review-cli.md`](notify-review-cli.md).

- **Text for a person by default, JSON lines with `--json`.** `record-queried-with-ordinary-tools`
  already says the record prints as JSON lines. Every command takes `--json`, and its JSON is one
  object per line, so `jq` and `grep` read it. Text output may change between versions; JSON keys
  only grow.
- **Each output ends with the next command, when there is an obvious one.** One line, starting with
  `next:`. Never more than two commands on it.
- **Times are local, with the day when it is not today.** `02:47`, `Mon 22:58`. JSON carries the
  full time with its offset (record-ux sketch (e)).
- **A checkpoint is named by its time.** `silo restore main 02:40` picks the checkpoint taken at
  02:40, or says which two are closest when none is. Ids such as `ck-0412` work too, and JSON
  carries them.
- **`main` is the default computer.** Every command that takes a computer uses main when none is
  given, as `git` uses the current branch.
- **Facts, reports and opinions are marked the same way everywhere**, with the convention of
  record-ux Part 2: no mark for a fact, `◌` for a report, `◇` for an opinion, `«»` around words an
  agent or a host wrote. A legend line ends any output that uses a mark.
- **Destructive commands say what they end, then ask.** `restore`, `stop`, `drop`, `put-back` and
  emptying the bin print one sentence of what they end and wait for `y`. `--yes` skips the
  question, for scripts. Protective commands (`pause`, `checkpoint`, `deny`) never ask.
- **Exit codes a script can use.** 0 done; 1 failed; 2 wrong usage; 3 for `silo` alone when
  something needs you, so `silo || notify` works in a shell prompt or a cron job.
- **No spinner without a number.** Long steps show what is done out of what, and the time left
  when it can be computed (the first run's download, a restore).
- **Errors say what to do.** Each error names the cause in one line and the command that fixes it
  in the next. No stack trace unless `--debug`.

## The commands

The first build column follows the smallest first build (`research/2026-11-24-first-build.md`, draft PR #348) and
this research's own proposals (in bold where they add to it).

| Command | What it does | First build | Plan entries |
|---|---|---|---|
| `silo` | What needs you, and what happened since you last looked. On a Mac with no main, starts the first run | yes | `first-screen`, `one-queue`, `app-as-text`; "since you last looked" is new (record-ux 4.1) |
| `silo setup` | The first run, or what is left of it | yes | `agent-programs-first-run`, `first-run-failure`, `first-run-wait` |
| `silo status [computer]` | Each computer: state, memory, disk, network, last checkpoint, way in | yes | `computers-main-on-a-mac`, `ways-out-shown` |
| `silo start / stop / pause / resume [computer \| --all]` | Run state of a computer | yes | `computers-stopped-and-paused`, `pause-every-computer`, `what-stop-ends` |
| `silo checkpoints [computer]` | The checkpoints, newest first, thinned ones folded | yes | `checkpoints-taken-on-their-own`, `thinning-checkpoints` |
| `silo checkpoint [computer] [-m note]` | Take one now | yes | `automatic-checkpoints` (by hand is `what-you-may-do`) |
| `silo changes [computer] --since <time> [--until <time>]` | What changed on the disk, grouped by folder, as facts | yes | `file-changes-in-the-record`, `disk-place` |
| **`silo look <time>:<path>`** | Look inside a checkpoint, read only: list a folder, print a file | **proposed** | new; record-ux 4.11 |
| **`silo put-back <path>… --from <time>`** | Copy paths from a checkpoint back into the running computer | **proposed** | new; close to `putting-back-one-run`, without the run |
| `silo restore [computer] <time>` | Put the whole computer back to a checkpoint, saving where you were first | yes | `restore-to-a-checkpoint`, `restore-saves-first`, `what-a-restore-leaves` |
| `silo import <path on your Mac> [computer:path]` | Copy a file or a folder in, once | yes | `files-imported-from-your-mac` |
| `silo record [computer] [--line …] --json` | The record, as JSON lines | yes, file lines only | `record-queried-with-ordinary-tools` |
| `silo doctor` | Check the Mac, the engine, the SSH entries and main's way in; print what to fix | **proposed** | `first-run-failure`, `computer-reached-over-ssh` |
| `silo away [until <time>]` / `silo back` | Say you are away, so that nothing waits for you | **proposed**, later builds | new; see [`daily-loop.md`](daily-loop.md) |
| `silo runs` | Now: each run alive, one line each | later | `now-place`, `now-in-the-app` |
| `silo receipt <run>` | The receipt of a run, task or fork | later | `receipts-of-work`, `what-a-receipt-holds` |
| `silo stop-run <run>` | End one run and what descends from it | second build | `stopping-a-run` |
| `silo fork [computer] [--from <time>] [name]` | Make a fork | second build | `forking-a-computer` |
| `silo drop <computer>` / `silo bin` / `silo bring-back <computer>` | Drop to the bin, list it, bring one back | second build | `dropping-into-the-bin`, `time-in-the-bin` |
| `silo asks` / `silo allow <ask>` / `silo deny <ask>` | The asks waiting, and your answer | later | `held-requests-and-asks`, `one-queue` |
| `silo alerts` / `silo set-aside <alert>` | Alerts, and setting one aside | later | `alerts-on-a-change`, `alert-only-on-a-change` |
| `silo rules [computer]` / `silo rule add …` | Rules as text | third build | `allow-hold-or-deny`, `rules-are-data`, `rule-states-its-worst-case` |
| `silo budget [computer] [$N/day]` / `silo spend` | Budgets and spend | third build | `budget-on-a-computer`, `spend-per-conversation` |
| `silo secret add <name>` | Enter a secret in Silo's own prompt | second build | `secrets-held-outside`, `secret-entry` |

`look` and `put-back` are the two additions that change the first build most. The reason is in
[`daily-loop.md`](daily-loop.md): with several agents in main, which `several-agents-in-main` makes
the normal case, a whole restore throws away every other agent's night. `put-back` needs no
opinion about which run did what, only the path you name and the checkpoint you pick.

## The output of each command

The case: polarzero's Mac, an M3 Pro with 36 GB. Main runs macOS 27.2. On Monday at 22:58 three
agents were left working; it is Tuesday 06:58. The agents work in the repositories copied in at the
first run, `~/work/app` and `~/work/client-b`. In the first build all three agents are in main,
and Silo knows no runs, so the first build's output shows only computers, checkpoints and file
changes. The output of later builds follows, for the same night. There, claude #1 and claude #2 are
in main, and codex works in the fork `try-bun`.

### `silo`, first build

```text
$ silo
main · running · 8 h since you last looked (Mon 22:58)

Since Mon 22:58
  48 checkpoints taken · 7,412 files changed · 3,412 deleted · disk +2.1 GB
  Your Mac did not sleep. Main did not restart.

Changed outside your repositories (~/work/app, ~/work/client-b)
  deleted   ~/work/shared-fixtures/              3,412 files  −1.2 GB   02:40 → 02:50
  changed   ~/.gitconfig                         +1 line                02:20 → 02:30
  changed   ~/.claude/CLAUDE.md                  +1 line                04:10 → 04:20
  new       ~/Library/LaunchAgents/homebrew.mxcl.postgresql@17.plist    03:40 → 03:50

next: silo changes --since 22:58 · silo look 02:40:~/work/shared-fixtures
```

What it rests on:

- Every line is a fact: checkpoints, file changes found by comparing two checkpoints from outside
  (`file-changes-from-outside`), and the Mac's sleep and main's starts, which the engine sees.
- "Since you last looked" is the time `silo` last printed this screen, kept by the engine. No plan
  entry has it yet (record-ux 4.1).
- "Changed outside your repositories" filters on paths set once at the first run: the
  repositories your agents work in, here `~/work/app` and `~/work/client-b`. A change outside them
  is the one most worth seeing first, as both the first-build case (a folder deleted outside the
  repository) and the incidents survey (14 of 41 incidents destroyed files on the agent's own
  machine) say. It is a filter on paths, a fact, not an opinion.
- The change times are windows between two checkpoints, never one moment, as record-ux 4.14
  proposes.
- macOS writes files by itself overnight (Spotlight, caches, logs): 812 of the 7,412 on this
  night. They are never listed here; `silo changes` shows them in a group of their own. Which paths
  macOS writes by itself is a list shipped with Silo and measured on a main left idle for a night,
  as file 12 of the first-build decisions asks.

### `silo`, with later builds

```text
$ silo
Since you last looked (Mon 22:58) · 8 h · 3 runs · 214 turns · $31.00

NEEDS YOU
  1  main     deleted ~/work/shared-fixtures/ · 3,412 files · 1.2 GB · 02:47
              outside ~/work/app · rule outside-the-repo · ◇ claude #2
              the checkpoint of 02:46 is kept for this alert
              silo put-back ~/work/shared-fixtures --from 02:46
  2  try-bun  ◇ codex stuck since 04:58 · no model call for 2 h · ◌ `bun test --watch`
              silo stop-run codex
  3  main     2 requests refused as held, not answered · github.com push ×2 · 01:12
              silo asks --refused
  4  try-bun  new host bun.sh · first seen 00:30 · ◇ codex
              silo record try-bun --line host
DONE
     main     claude #1  6 h 54 · $14.20 · 38 files · ◌ last full `npm test` exit 1 (3 failed)
                         ◇ its last words «All tests pass»: mismatch
     main     claude #2  5 h 16 · $9.10 · 61 files · 1 deletion outside the repo

next: silo put-back ~/work/shared-fixtures --from 02:46 · silo receipt claude#1
◌ told by the probe, unverified · ◇ opinion · «» words of an agent
```

This is sketch (a) of record-ux in the command line, with one item added: the requests refused as
held, not answered, while you slept (item 3), which `one-queue` takes off the list today.
[`daily-loop.md`](daily-loop.md) argues for keeping them. The checkpoint of 02:46 exists because
by then Silo takes a checkpoint at each turn of an agent (`checkpoint-at-each-turn`, open), not
only on the tens. The opinions (`◇`) come with the classifier, in a later build.

With nothing waiting, `silo` prints the since line, then `Nothing needs you.`, then DONE.

### `silo status`

```text
$ silo status
main     running   macOS 27.2   6 cores · 16 GB · disk 96 GB (41 GB its own)
         network open, no boundary yet · ssh main · last checkpoint 06:50, 48 taken since Mon 22:58
Mac      36 GB memory, 11.8 GB free · 188 GB free on Macintosh HD · on power, kept awake
         a second macOS computer can run (Apple allows two)
```

"network open, no boundary yet" is what the first build must say, since it has no rules. If the
first build runs main on Apple's NAT, as first-build.md proposes as a fallback when gvproxy is slow
(Mac test 5), nothing at all stands between main and the internet. Saying it in the status line
keeps the open network from reading as a limited one.

### `silo checkpoints`

```text
$ silo checkpoints
main · 30 checkpoints · 9.4 GB that only checkpoints hold

  06:50  06:40  06:30  06:20  06:10  06:00  05:50 … every 10 min since 05:00     (12)
  04:00  03:00  02:00  01:00  00:00  Mon 23:00 … Mon 14:00          one per hour (15)
  02:40  kept: the last one before ~/work/shared-fixtures was deleted
  Mon 22:58  before you left · taken by you · "3 agents tonight"
  Mon 13:42  the first, before any agent ran

next: silo changes --since 02:40 --until 02:50 · silo look 02:40:~/work
```

The folded lines follow `thinning-checkpoints` (every one for 2 hours, then one per hour). The
10-minute interval is this research's choice; the plan gives none, and the first-build research
says "every few minutes". The kept 02:40 is record-ux recommendation 1. A checkpoint you take by
hand is never thinned: `thinning-checkpoints` thins only automatic ones. The one Silo saves before
a restore is another matter (see `silo restore` below).

### `silo changes`

```text
$ silo changes --since 22:58
main · Mon 22:58 → Tue 06:50 · 47 checkpoints compared · 7,412 files

outside your repositories (~/work/app, ~/work/client-b)
  deleted   ~/work/shared-fixtures/                      3,412 files  −1.2 GB    02:40 → 02:50
  changed   ~/.gitconfig                                 +1 −0                   02:20 → 02:30
  changed   ~/.claude/CLAUDE.md                          +1 −0                   04:10 → 04:20
  new       ~/Library/LaunchAgents/homebrew.mxcl.postgresql@17.plist           03:40 → 03:50
  new       /opt/homebrew/  postgresql@17, watchman      2 packages, 380 MB

~/work/app                                               96 files  +1,412 −388
  rebuildable   node_modules/  2,904 files · 312 MB        (hidden: --all)
  src/auth/     12 files changed · 2 new
  test/fixtures/  41 deleted
  … 41 more files                                         silo changes ~/work/app

macOS itself   812 files in caches, logs and Spotlight      (hidden: --all)

next: silo look 02:40:~/work/shared-fixtures · silo put-back ~/work/shared-fixtures --from 02:40
```

Every line here is a fact: a path that differs between two checkpoints. "rebuildable" is not an
opinion here, unlike in `how-a-path-comes-back`: it is the folder name and its lock file next to it,
read from the ecosystem data of `ecosystem-data`, and it only folds lines; `--all` unfolds them.
The `/opt/homebrew` line groups by Homebrew's own folder names, also a fact about paths.

With `--json`, each changed path is one line:

```text
{"id":"f-88213","kind":"fact","line":"file","computer":"main","change":"deleted","path":"/Users/ana/work/shared-fixtures/","files":3412,"bytes":-1288490188,"between":["ck-2391","ck-2392"],"between_times":["2026-11-24T02:40:00+01:00","2026-11-24T02:50:00+01:00"]}
```

### `silo look` (proposed)

```text
$ silo look 02:40:~/work/shared-fixtures
main at 02:40 (checkpoint ck-2391) · read only
  ~/work/shared-fixtures/    3,412 files · 1.2 GB
    payments/   1,904 files     last changed Mon 17:20
    auth/         880 files     last changed Mon 17:20
    README.md     2 KB          last changed Fri 11:02
    … 3 more

Now: ~/work/shared-fixtures/ does not exist in main.
next: silo put-back ~/work/shared-fixtures --from 02:40
```

The host already reads checkpoints from outside to compare them (`file-changes-from-outside`), with
a reader in user space (first-build decisions, finding 3). `look` uses the same reader, and shows
what it reads as data: a file an agent wrote is printed with `«»` when it is printed at all.

### `silo put-back` (proposed)

```text
$ silo put-back ~/work/shared-fixtures --from 02:40
This copies 3,412 files (1.2 GB) from the checkpoint of 02:40 into main, at
~/work/shared-fixtures. Nothing is at that path now, so nothing in main is replaced.
Main keeps running. A checkpoint is taken first.
Put back? [y/N] y
  checkpoint 07:01 taken
  copying  ████████████████████  3,412 / 3,412 files   1.2 GB   14 s
Put back ~/work/shared-fixtures from 02:40.
next: silo changes --since 07:00
```

When the path exists now and differs, the command lists what it replaces and how much changed
since, and the checkpoint it takes first keeps the current version. Overwriting is destructive, so
it is yours to start, never a precaution's (`kinds-of-other-actions` already files "put back paths"
as destructive).

Putting a path back in main is a partial rewind of main that does not go through Git. That
challenges `fork-from-the-moment` (taken); the case and the reason are in
[`daily-loop.md`](daily-loop.md), gap 6.

### `silo restore`

```text
$ silo restore 02:40
Restore main to 02:40?
This ends every program running in main, and puts its whole disk back to 02:40.
Everything written in main after 02:40 leaves main: 4 h 22 of work, 3,944 files changed since.
Silo first takes a checkpoint of now (07:02), so you can come back to it.
Stays as it is: the record, the rules, what you granted.
Your agents' conversations go back to 02:40 too. Take one up again with `claude --resume`
or `codex resume`.
Restore? [y/N] y
  checkpoint 07:02 taken (kept until you set it aside)
  stopping main …  done in 6 s
  restoring disk … done in 2 s
  starting main …  ssh main answers after 41 s
Main is at 02:40. Where you were is checkpoint 07:02: silo restore 07:02 to go back.
```

Two things here go past the plan:

- The checkpoint taken before a restore is kept until you set it aside. `restore-saves-first`
  says it "is thinned like the others", so `thinning-checkpoints` can remove it two hours later
  (first-build decisions, finding 6). It is the only copy of what the restore left.
- The prompt counts what the restore leaves, from file facts between 02:40 and now, so you see the
  cost before you type `y`.

### `silo doctor` (proposed)

```text
$ silo doctor
✓ Silo 0.1.0, engine running since Mon 09:12
✓ Mac: macOS 27.2, Apple M3 Pro, 188 GB free
✓ main is running, ssh main answers in 0.2 s
✗ ~/.ssh/config: the Include line for Silo is not the first line
  A Host * block above it adds ForwardAgent yes to main.
  Fix: silo setup ssh    (moves the line to the top, with your yes)
✓ Mac kept awake while main runs, on power
! With its lid closed, a MacBook sleeps unless it is on power with an external display
  attached. Main stops until it wakes.
```

The SSH check is first-build decisions finding 7: a `Host *` above Silo's entries forwards the SSH
agent into main.

## Commands that were left out, on purpose

- **No `silo run claude`.** `what-silo-does-not-build` (open, no verdict yet) says Silo does not
  hide or wrap the agent program's own interface, and `unmodified-agent-programs` says you install and start programs the
  vendor's way. The agent is started in main, in a terminal over SSH. How a run outlives the
  terminal that started it is a real gap, covered in [`daily-loop.md`](daily-loop.md), and the
  proposal there needs no wrapper.
- **No `silo diff` in the first build.** `silo changes --since --until` is the diff of two moments
  of one computer. Two computers come with forks (record-ux 4.8).
- **No `silo logs`.** The record is the log. `silo record --json` is the way in.
