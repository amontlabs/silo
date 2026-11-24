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
| Evening | Start three agents in main, each with every check off | A run keeps going when your terminal closes, and while your Mac is awake; a closed lid is warned about | partly | **1, 2** |
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
then end, mid-turn. People who leave agents running keep them in `tmux`. Claude Code's own Remote
Control page says to "start it inside `tmux` or `screen` to keep the session running after you
disconnect from SSH". Claude Squad is built on tmux, and users who run several sessions describe them as "via tmux"
([`onboarding.md`](onboarding.md), Claude Code and Claude Squad; [`complaints.md`](complaints.md),
item 79).

`what-silo-does-not-build` (open, no verdict yet) says Silo does not wrap the agent program, so
`silo run claude` is out. A way that wraps nothing: main ships with `tmux`, and Silo writes a second
SSH entry beside `main`.

```text
Host main
  HostName …            # plain SSH, as today
Host main.keep
  HostName …
  RequestTTY yes
  RemoteCommand tmux new-session -A -s keep
```

`ssh main.keep` opens a shell in one tmux session, named `keep`, that outlives the connection.
Each agent gets its own tmux window in that session: you press Ctrl-b c to open a new window, and
start the next agent there. `ssh main.keep` again, from any terminal, attaches to the same session,
so all three agents come back at once. A second terminal attached at the same time shows the same
window as the first; Ctrl-b n moves between windows. Nothing is hidden: it is tmux, and `ssh main`
still gives a plain shell. The first run prints both, and says which one survives a closed
terminal. Since `main.keep` needs tmux, the first run's set-up step installs it in every case: it is
part of that fixed step, not one of its choices ([`first-run.md`](first-run.md), question 4).

Proposed: a new decision in `agent-programs`, "Does a run outlive the terminal that started it?",
with the answer above. It needs `computer-reached-over-ssh`. It changes the answer of
`reaching-a-computer` (open), which says "one named entry per computer": main would have two. The
name `main.keep` is a placeholder.

### Gap 2. Your Mac sleeps, and main stops with it

A virtual machine runs only while its host is awake. With its lid closed, a MacBook sleeps unless
it is on power with an external display attached. A desktop Mac sleeps after its idle time unless
something holds it awake. Whether Apple's framework holds the Mac awake while a virtual machine
runs is not known: no source says that it does ([`macos-first-run.md`](macos-first-run.md),
section 5). It is to check with `pmset -g assertions` on a Mac. If it does not, the night most
people picture, laptop closed on the desk, runs for the minutes before sleep and then not at all. The morning shows a
night of work that never happened, and nothing says why.

What Silo can do, all of it from facts:

- While main is running and on power, the engine holds the Mac awake with a power assertion of the
  kind `caffeinate -i` takes (system idle sleep prevented, the display may sleep). It is switched
  on at the first run, with your yes, and shown in `silo status` ("kept awake").
- With its lid closed, a MacBook sleeps unless it is on power with an external display attached,
  and `caffeinate` cannot stop it. Main stops until it wakes. Silo says so once, at the first run
  and in `silo doctor`.
- Every sleep and wake of the Mac is a fact the engine sees. The morning screen says "Your Mac slept
  from 01:12 to 06:40; main did no work for 5 h 28".
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

The 10-minute interval is this research's choice. The plan gives none, and the first-build research
says "every few minutes".

A checkpoint with your note marks "before I left" on the list, and the morning's `silo` counts from
the last time you looked. In a later build, `silo runs` prints Now (`now-place`): each run, its
state as an opinion, and how long it has been in it. The second build brings the processes it rests
on.

### Gap 3. An agent waiting for its user looks idle, and one that guessed looks fine

Four complaints describe the daily failure of an unattended run as a run silently blocked, not one
that destroyed something ([`complaints.md`](complaints.md), items 47, 48, 50 and 51). In the
other direction, an agent's question to its user now times out after 60 seconds and the agent goes
on with a guess: "No response after 60s — continued without an answer" (anthropics/claude-code #73125, 388
👍). People dislike both: the silent block and the silent guess. And "stuck" and "done" look the
same from outside the terminal.

Running with every check off removes permission prompts, not questions the model chooses to
ask. Silo does not answer for you and builds no window to talk to an agent. That is the answer of
`what-silo-does-not-build`, which is open, with no verdict yet. The same answer says that an answer
to an agent's question is something you may send from Silo. Silo also sees the case from outside:
the model's last turn ended with no tool call, and the run has made no model call since.

Both are facts once model traffic is read. "Waiting on its user" is then an opinion of the
classifier of `how-each-run-is-going`, whose states today are working, waiting on the model,
waiting on a command, idle or stuck. A run waiting on its user shows as idle.

Proposed: in `how-each-run-is-going` (feature) and `now-place` (decision), a sixth state, "waiting
on you", for a run whose last turn ended without a tool call, with the last line of that turn shown
beside it in `«»`. An alert on it after a time you set is a precaution on an opinion, so it can only
alert, which is all it should do. The natural action on that line is to answer the question from
Silo, which `what-silo-does-not-build` allows. How the answer reaches the program, without wrapping
it, differs per program and is to find out for each.

The silent guess is visible too. The question the agent asked is a tool call in the model's reply,
and the answer it got ("No response after 60s — continued without an answer") is in the next
request: both facts in the traffic. The morning list is what `silo` shows when you come back:
what needs you, and what happened since you last looked (record-ux sketch (a)). It can then hold
one line per run, "asked you 2
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

1. **Requests refused as held, not answered, stay on the list until morning.** In `one-queue`: when
   a hold ends without your answer, the request is refused, and it is no longer an ask. It stays on
   the list as one line, "requests refused as held, not answered", grouped by computer, host and
   kind of request, with a count. It no longer holds anything, so it cannot be allowed; it offers
   "allow next time", which proposes a rule (`silo-proposes-rules`, taken: a proposal changes
   nothing until you accept it). You see that the agent was blocked, and you decide once instead of
   every night. Docker's `sbx` already works this way for its network rules: a blocked host is
   denied at once, and an approval waits in a list with two answers, allow and dismiss
   ([`onboarding.md`](onboarding.md), section 1).
2. **Away, said by you.** `silo away until 07:00`, or the Mac's Sleep focus if you let Silo read it.
   While you are away, a rule that would hold a request refuses it at once, with words that say
   so: "denied: the user is away until 07:00; not sent". The agent does not wait 90 seconds
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
  other (`.authenticationRequired`). This is the "step-up for risky answers" of
  [`notify-review-cli.md`](notify-review-cli.md), section A2. Duo Verified Push goes further and
  makes you type a code, when it sees a risk. That is against push fatigue: the Uber breach of 2022
  went through a person who approved one push among many.
- **"Allow once" is the only allow on the lock screen.** "Allow always" makes a rule, and a rule
  states its worst case when it is set (`rule-states-its-worst-case`). That sentence needs the
  phone unlocked and read, so it is one step deeper.
- **The time left is shown**, because the request goes when it ends.
- **A flood is folded.** More than three asks in a minute from one computer arrive as one
  notification, "7 asks from main", that opens the list. Number matching against MFA fatigue does
  not apply, since the ask comes from Silo and not from a login, but the same fatigue does.

### Gap 5. Every alert interrupts equally

`alert-only-on-a-change` (open) decides when an alert is raised and merges alerts of one kind. It
does not say which alerts may wake you. Pager tools split by urgency for this reason. Pushover,
GitHub Mobile and iOS Focus all have quiet hours ([`notify-review-cli.md`](notify-review-cli.md)).

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

Two more rules, both from Claude Code's Remote Control. It has two push toggles, "Push when Claude
decides" and "Push when actions required". It "skips mobile push notifications while you are typing
in or focused on the connected terminal" ([`notify-review-cli.md`](notify-review-cli.md),
section A):

- **Nothing reaches the phone while you are at the Mac.** The Mac's screen being unlocked and in use
  is a fact the engine can read. The ask shows on the Mac instead.
- **A push only signals.** When the phone is offline, Apple keeps one notification per app, so the
  phone app loads the list from Silo when it opens, and a notification is withdrawn as soon as the
  ask is answered anywhere else.

With a new-host alert switched on for an open main (`new-host-alert-is-a-rule`), the first week
brings many hosts: registries, CDNs, GitHub, a model provider. Folded into the morning list as one
line with a count ("23 new hosts, 3 never seen in any of your computers"), the alert stays useful.
Pushed to a phone one host at a time, it would ring dozens of times in a week.

## Morning

The morning screen is sketch (a) of record-ux, and `silo` in [`cli.md`](cli.md). Three lines are
added by this file:

- the sleeps and restarts of the Mac and of main (gap 2);
- the requests refused as held, not answered, while you were away (gap 4);
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
It is smaller than `putting-back-one-run` (open), which asks Silo to find one run's paths, an
opinion. That entry already plans putting paths back in a shared main.

Proposed: a new feature in `going-back`, "Paths put back from a checkpoint", in the first build,
with `kinds-of-other-actions` (put back paths: destructive) and `restore-saves-first` (a checkpoint
is taken first) applying to it as written. It makes the first build's promise hold in the case the
first build itself uses as its example.

This is a challenge to `fork-from-the-moment` (taken). It says that on a shared main "you do not
rewind … the past is read-only, you continue from it in a fork … the result leaves the fork through
Git". Putting a path back in main is a partial rewind of main that does not go through Git. In the
case above, the fork it asks for does not exist in the first build, and the shared fixtures are not
in a repository, so Git cannot carry them. The partial put-back is acceptable in the first build for
two reasons. A checkpoint of now is taken first, so nothing is lost. And the other way back the
plan allows, a restore of all of main, rewinds every agent's work, which is worse. Proposed: an
exception in `fork-from-the-moment` for paths you name, until forks are built.

## Day: answering asks and changing limits

### Gap 7. Answering takes the Mac until the phone app exists

`ask-reaches-you` keeps answering in Silo's own apps, for a good reason: a yes can grant. Until the
phone app is built, an ask reaches you only at the Mac. With a 90-second hold, an ask raised while
you make coffee is refused. Two cheap steps before the phone app:

- A macOS notification from the engine, with Allow and Deny buttons, answered on the Mac. It is
  Silo's own, so `ask-reaches-you` allows it, and it reaches you when the terminal is not in front.
  Allow asks for Touch ID or the Mac's password, as Allow asks for Face ID on the phone.
- A forwarded message that only shows the ask and links back, as `ask-reaches-you` permits (ntfy,
  Pushover or a chat app). It answers nothing, and it tells you to go to the Mac within the 90
  seconds or let it go.

Neither needs a plan change beyond the order: `forwarding-to-a-phone` sits behind extensions
(`extension-that-reads-and-alerts`, `forwarding-tool`), and the macOS notification has no entry.
Proposed: a feature in `views`, "Asks and alerts as Mac notifications", placed with
`held-requests-and-asks`.

## What the day looks like when it works

For the record of what this design aims at, the same day in five lines:

- 22:55 `ssh main.keep`, claude #1 and claude #2 started in main, codex started in the fork
  `try-bun`, `silo checkpoint -m "3 agents tonight"`, `silo away until 07:00`.
- 01:12 a push is refused at once, with words that say you are away. claude #2 goes on with the
  tests. Nothing reaches your phone.
- 02:47 a folder is deleted outside the repository; a rule flags it and keeps the checkpoint before
  it. Nothing reaches your phone.
- 05:55 the disk drops below the second level, three minutes after claude #1 ended at 05:52; main
  pauses; your phone rings once, because a paused computer does no work. You leave it until
  morning, since no agent in main is still working, or move that kind of alert to the morning list
  the next day.
- 06:58 `silo`: four lines. You put the folder back in 14 seconds, allow pushes from main to
  `ana/app` from now on, open claude #1's receipt, and see its tests failed.
