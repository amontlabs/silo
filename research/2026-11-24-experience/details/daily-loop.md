# The daily loop: start agents, leave them, come back, go back

Research of 24 November 2026, by an agent, for polarzero. It walks one day and one night with
Silo, first with the first build alone, then with what the later builds add, and writes out what
you see at each step. Nothing here is a decision, and nothing under `plan/` was edited. Prior art
and user quotes are in [`complaints.md`](complaints.md) and
[`notify-review-cli.md`](notify-review-cli.md); the morning screens build on the sketches of
record-ux (`research/2026-11-24-record-and-spend/details/record-ux.md`), which this file does not
repeat.

## The loop, in one table

| Step | What you do | What Silo must make true | First build | Gap found here |
|---|---|---|---|---|
| Evening | Start three agents in main, each with every check off | A run keeps going when your terminal closes, and while your Mac's lid is closed | partly | **1, 2** |
| Before bed | Check that they are working, then leave | One screen says what is alive and what it will do if something goes wrong | no runs yet | **3** |
| Night | Nothing, if all goes well | Nothing waits for you while you sleep. What cannot wait reaches your phone, and very little cannot wait | no phone | **4, 5** |
| Morning | See what happened, in a minute | One screen: what needs you, and what happened since you last looked | file changes only | record-ux |
| Undo | Bring back what one agent destroyed, keep what the others did | Put back paths, not only the whole computer | whole restore only | **6** |
| Day | Answer asks, raise a budget, approve a push | An ask is answered in seconds, from wherever you are | no asks | **4, 7** |

The gaps are numbered as in the sections below. Each one ends with what it would change in the
plan.

## Evening: starting the agents

What you type, with the first build:

```text
$ ssh main
ana@main ~ % cd ~/work/app
ana@main app % claude --dangerously-skip-permissions
```

Two things then go wrong for people who leave agents running, and neither is in the plan.

### Gap 1. A run dies with the terminal that started it

A program started in an SSH session gets SIGHUP when the session ends: you close the terminal tab,
your Mac's Wi-Fi drops for a minute, or the terminal app quits for an update. Claude Code and Codex
then end, mid-turn. People who leave agents running keep them in `tmux`: Claude Code's own Remote
Control page says the local process must keep running and to "use tmux over SSH", Claude Squad is
built on tmux, and users who run several sessions describe them as "via tmux"
([`onboarding.md`](onboarding.md), Claude Code and Claude Squad; [`complaints.md`](complaints.md),
item 79).

Silo must not wrap the agent program (`what-silo-does-not-build`), so `silo run claude` is out. A
way that wraps nothing: main ships with `tmux`, and Silo writes a second SSH entry beside `main`.

```text
Host main
  HostName …            # plain SSH, as today
Host main.keep
  HostName …
  RequestTTY yes
  RemoteCommand tmux new-session -A -s keep
```

`ssh main.keep` opens the same shell in a session that outlives the connection; `ssh main.keep`
again, from any terminal, comes back to it. Nothing is hidden: it is tmux, and `ssh main` still
gives a plain shell. The first run prints both, and says which one survives a closed terminal.

Proposed: a new decision in `agent-programs`, "Does a run outlive the terminal that started it?",
with the answer above. It needs `computer-reached-over-ssh` and touches `reaching-a-computer`
(the file of SSH entries). The name `main.keep` is a placeholder.

### Gap 2. Your Mac sleeps, and main stops with it

A virtual machine runs only while its host is awake. A MacBook with its lid closed sleeps unless
it is on power with an external display; a desktop Mac sleeps after its idle time unless something
holds it awake. Apple's framework does not hold the Mac awake while a virtual machine runs
([`macos-first-run.md`](macos-first-run.md), section 5). So the night most people picture, laptop
closed on the desk, runs for the minutes before sleep and then not at all. The morning shows a
night of work that never happened, and nothing says why.

What Silo can do, all of it from facts:

- While main is running and on power, the engine holds the Mac awake with a power assertion of the
  kind `caffeinate -i` takes (system idle sleep prevented, the display may sleep). It is switched
  on at the first run, with your yes, and shown in `silo status` ("kept awake").
- Silo cannot stop a lid-closed laptop on battery from sleeping, so it says so once, at the first
  run and in `silo doctor`.
- Every sleep and wake of the Mac is a fact the engine sees. The morning screen says "Your Mac slept
  from 01:12 to 06:40; main did no work for 5 h 28". That one line explains a quiet night.
- A macOS update that restarts the Mac at night brings main back only once you log in, because the
  framework runs in your login session (first-build decisions, finding 1). The morning screen says
  so too, and lists the runs that were alive before the restart, as `computer-stops-by-itself`
  already asks.

Proposed: a new feature in `computers`, "The Mac kept awake while agents work", and a sentence in
`first-run-failure` or `agent-programs-first-run` that the first run asks for it. It is protective:
it keeps a computer running that you asked to run.

## Before bed: is everything working?

With the first build there is no list of runs: processes come with the probe on macOS, in the
second build (first-build.md, "What is left out"). The best the first build can print before you
leave:

```text
$ silo checkpoint -m "3 agents tonight"
main · checkpoint Mon 22:58 taken · "3 agents tonight"
Silo takes one every 10 minutes while main runs. Your Mac is on power and kept awake.
next: silo (tomorrow morning)
```

A checkpoint with your note marks "before I left" on the list, and the morning's `silo` counts from
the last time you looked. With the second build, `silo runs` prints Now (`now-place`): each run, its
state as an opinion, and how long it has been in it.

### Gap 3. An agent waiting for its user looks idle, and one that guessed looks fine

The user reports agree that the daily failure of an unattended run is a run silently blocked, not
one that destroyed something ([`complaints.md`](complaints.md), items 47 to 51). One background
Claude Code session waited on a prompt for about 8.5 hours before anyone noticed. In the other
direction, an agent's question to its user now times out after 60 seconds and the agent goes on with
a guess: "No response after 60s — continued without an answer" (anthropics/claude-code #73125, 388
👍). People dislike both: the silent block and the silent guess. And "stuck" and "done" look the
same from outside the terminal.

Running with every check off removes permission prompts, not questions the model chooses to
ask. Silo does not answer for you and builds no window
to talk to an agent (`what-silo-does-not-build`), and that is right. But it sees the case from
outside: the model's last turn ended with no tool call, and the run has made no model call since.

Both are facts once model traffic is read. "Waiting on its user" is then an opinion of the
classifier of `how-each-run-is-going`, whose states today are working, waiting on the model,
waiting on a command, idle or stuck. Idle hides the one idle that matters.

Proposed: in `how-each-run-is-going` (feature) and `now-place` (decision), a sixth state, "waiting
on you", for a run whose last turn ended without a tool call, with the last line of that turn shown
beside it in `«»`. An alert on it after a time you set is a precaution on an opinion, so it can only
alert, which is all it should do.

The silent guess is visible too. The question the agent asked is a tool call in the model's reply,
and the answer it got ("No response after 60s — continued without an answer") is in the next
request: both facts in the traffic. The morning list can then hold one line per run, "asked you 2
questions while you slept and went on without answers", each opening the turn. Which tool is a
question to the user differs per program, so it belongs in each program's data file
(`agent-program-data-file`), as the command that resumes a conversation does.

## Night: what reaches you, and what waits for the morning

The plan has two pieces that meet here and do not fit.

- `hold-limit-length` (open): a hold lasts 90 seconds, so that the agent gets Silo's words instead
  of its own timeout. The measures of 24 November (PR #347) show Claude Code waits 120 seconds then
  keeps the command for 10 more minutes in the background, and Codex waits as long as the model
  keeps polling. curl and git wait with no limit; pip gives up after 15 seconds and tries again.
- `one-queue` (open): "An ask whose hold ends while it waits leaves the list, and the record says it
  was refused as held and not answered."
- `phone-app` (feature, no needs) comes late in `order-of-work`, and `ask-reaches-you` says only
  Silo's own apps answer an ask.

Put together for a night: at 01:12 claude #2 pushes to GitHub, a rule holds the push, nobody
answers for 90 seconds, the push is refused, and the ask leaves the list. At 06:58 you see nothing
about it unless you read the record. The agent may have tried again, or found another way, or
stopped. This is the overnight case the plan's own `merges-and-deploys-held` describes, and it
ends with nothing to act on in the morning.

### Gap 4. An ask nobody answered vanishes

Proposed, three changes:

1. **Refused asks stay on the list until morning.** In `one-queue`: an ask whose hold ended without
   your answer stays as one line, "refused while you were away", grouped by computer, host and
   kind of request, with a count. It no longer holds anything, so it cannot be allowed; it offers
   "allow next time", which proposes a rule (`silo-proposes-rules`, taken: a proposal changes
   nothing until you accept it). You see that the agent was blocked, and you decide once instead of
   every night. Docker's `sbx` already works this way for its network rules: a blocked host is
   denied at once, and an approval waits in a list with two answers, allow and dismiss
   ([`onboarding.md`](onboarding.md), section 1).
2. **Away, said by you.** `silo away until 07:00`, or the Mac's Sleep focus if you let Silo read it.
   While you are away, a rule that would hold a request refuses it at once, with words that say
   so: "held for the user, who is away until 07:00; not sent". The agent does not wait 90 seconds
   for nobody, nine times in a night, and its words tell it to go on with other work. The trigger is
   you, so it may do this (`less-sure-less-power`). It grants nothing: a hold becomes a deny.
3. **The phone app's first job is these two, not a full view.** The smallest phone app that serves
   the loop shows the list of what needs you, answers an ask, and sets you away or back. The rest
   (`phone-app` says "what needs you, with asks you can answer and alerts") can wait.

What the ask looks like when it does reach you, on the lock screen. It is written from facts, as
`what-an-ask-shows` asks, and the agent's reason, when there is one, is marked as its own words:

```text
┌─────────────────────────────────────────────┐
│ SILO · main                            now  │
│ Push to github.com/ana/app, branch main     │
│ 3 commits · held, 74 s left                 │
│ ◇ claude #2 · «tests pass, pushing the fix» │
│                                             │
│   [ Deny ]                [ Allow once ]    │
└─────────────────────────────────────────────┘
```

- **Deny is one tap. Allow asks for Face ID.** A deny is protective and a wrong one costs a retry;
  an allow is granting. Apple's notification actions can require Face ID for one button and not the
  other (`.authenticationRequired`). Duo and 1Password go further and make you type a code, against
  push fatigue: the Uber breach of 2022 went through a person who approved one push among many
  ([`notify-review-cli.md`](notify-review-cli.md), section A).
- **"Allow once" is the only allow on the lock screen.** "Allow always" makes a rule, and a rule
  states its worst case when it is set (`rule-states-its-worst-case`). That sentence needs the
  phone unlocked and read, so it is one step deeper.
- **The time left is shown**, because the request goes when it ends.
- **A flood is folded.** More than three asks in a minute from one computer arrive as one
  notification, "7 asks from main", that opens the list. Number matching against MFA fatigue does
  not apply, since the ask comes from Silo and not from a login, but the same fatigue does.

### Gap 5. Every alert interrupts equally

`alert-only-on-a-change` (open) decides when an alert is raised and merges alerts of one kind. It
does not say which alerts may wake you. Pager tools split by urgency for this reason, and quiet
hours are the setting people ask for first ([`notify-review-cli.md`](notify-review-cli.md)).

Proposed, a new decision in `views`, "Which alerts interrupt you?":

| Interrupts (phone, sound, time sensitive) | Waits for the morning list |
|---|---|
| An ask, while you are not away | A host seen for the first time |
| A computer that stopped and that Silo left stopped after three restarts | A deletion that a rule flagged (its checkpoint is kept, record-ux recommendation 1) |
| Disk below the second level, computers paused (`low-disk`) | A run waiting on you, or stuck |
| A budget reached, when you are not away | A run that ended |
| | A plan limit reached (`plan-limit-reached`) |

Each line on the left is something that gets worse by the minute and that you can act on. The
deletion is on the right because the checkpoint it needs is kept: it does not get worse by the
morning. You can move a kind from one column to the other.

Two more rules, both from Claude Code's Remote Control, which pushes to the phone only "when
actions required" and not while you are at the terminal ([`notify-review-cli.md`](notify-review-cli.md),
section A):

- **Nothing reaches the phone while you are at the Mac.** The Mac's screen being unlocked and in use
  is a fact the engine can read. The ask shows on the Mac instead.
- **A push only signals.** When the phone is offline, Apple keeps one notification per app, so the
  phone app loads the list from Silo when it opens, and a notification is withdrawn as soon as the
  ask is answered anywhere else.

With a new-host alert switched on for an open main (`new-host-alert-is-a-rule`), the first week
brings many hosts: registries, CDNs, GitHub, a model provider. Folded into the morning list as one
line with a count ("23 new hosts, 3 never seen in any of your computers"), the alert stays useful;
pushed to a phone, it teaches you to ignore Silo.

## Morning

The morning screen is sketch (a) of record-ux, and `silo` in [`cli.md`](cli.md). Three lines are
added by this file:

- the sleeps and restarts of the Mac and of main (gap 2);
- the asks refused while you were away (gap 4);
- the runs waiting on you, with their last words (gap 3).

The rule that orders the list, from record-ux and from every morning-review practice found: what
gets worse by waiting comes first, and each line ends with the one command or button that deals
with it.

## Undo: bringing back what one agent destroyed

### Gap 6. With several agents in main, the first build's only way back costs everyone's night

The first build has one way back: restore the whole of main (`restore-to-a-checkpoint`). The plan
makes several agents in one main the normal case (`several-agents-in-main`, taken), and says that
on a shared main you do not rewind, you fork from the moment (`fork-from-the-moment`, taken). Forks
are in the second build.

The case: claude #2 deleted `~/work/shared-fixtures` at 02:47. claude #1 worked until 05:52 and
left three uncommitted files and a running Postgres. Restoring main to 02:40 brings the folder back
and throws away three hours of claude #1's work. Not restoring keeps the work and loses the folder.
The first build offers no third way, except doing it by hand: there is no way to read a checkpoint
except by restoring it.

The third way needs only what the first build already builds:

- the host reads checkpoints from outside to compare them (`file-changes-in-the-record`);
- a file goes into main through a channel you grant (`files-imported-from-your-mac`).

`silo look` reads a path from a checkpoint and `silo put-back` copies it into main through that
channel ([`cli.md`](cli.md)). It needs no opinion about which run deleted what: you name the path.
It is smaller than `putting-back-one-run`, which asks Silo to find one run's paths, an opinion.

Proposed: a new feature in `going-back`, "Paths put back from a checkpoint", in the first build,
with `kinds-of-other-actions` (put back paths: destructive) and `restore-saves-first` (a checkpoint
is taken first) applying to it as written. It makes the first build's promise hold in the case the
first build itself uses as its example.

## Day: answering asks and changing limits

### Gap 7. Answering takes the Mac until the phone app exists

`ask-reaches-you` keeps answering in Silo's own apps, for a good reason: a yes can grant. Until the
phone app is built, an ask reaches you only at the Mac. With a 90-second hold, an ask raised while
you make coffee is refused. Two cheap steps before the phone app:

- A macOS notification from the engine, with Allow and Deny buttons, answered on the Mac. It is
  Silo's own, so `ask-reaches-you` allows it, and it reaches you when the terminal is not in front.
- A forwarded message that only shows the ask and links back, as `ask-reaches-you` permits (ntfy,
  Pushover or a chat app). It answers nothing, and it tells you to go to the Mac within the 90
  seconds or let it go.

Neither needs a plan change beyond the order: `forwarding-to-a-phone` sits behind extensions
(`extension-that-reads-and-alerts`, `forwarding-tool`), and the macOS notification has no entry.
Proposed: a feature in `views`, "Asks and alerts as Mac notifications", placed with
`held-requests-and-asks`.

## What the day looks like when it works

For the record of what this design aims at, the same day in five lines:

- 22:55 `ssh main.keep`, three agents started, `silo checkpoint -m "3 agents tonight"`, `silo away
  until 07:00`.
- 01:12 a push is refused at once, with words that say you are away. claude #2 goes on with the
  tests. Nothing reaches your phone.
- 02:47 a folder is deleted outside the repository; a rule flags it and keeps the checkpoint before
  it. Nothing reaches your phone.
- 04:30 the disk drops below the second level; main pauses; your phone rings once, because each
  minute paused is a minute of no work. You leave it until morning, or move that kind of alert to
  the morning column the next day.
- 06:58 `silo`: four lines. You put the folder back in 14 seconds, allow pushes from main to
  `ana/app` from now on, open claude #1's receipt, and see its tests failed.
