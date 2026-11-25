# A team's budget kept in its Git repository, measured on GitHub

Research of 25 November 2026, by an agent, for polarzero. #388 found that several Macs can hold
one budget with leases from a ledger, and that a branch of the team's repository can be that
ledger with no server: a push that is not a fast-forward is refused, so a push is a
compare-and-swap. It measured this only against a local repository, and its simulated "leases, 2 s
(a Git push as the ledger)" kept a recall that Git cannot make. This folder runs the leases against
a real GitHub branch, simulates them as Git can actually do them, and adds the check every Mac
needs when every member can write the ledger.

Nothing here is a decision. It adds agents' notes to the open entry `pooled-budget-ledger` (#388).
The branch is #388's by a merge, so its simulator is here to import.

## In short

- **GitHub works as the ledger.** A Mac asks for a slice in one push: 1.6 to 1.7 s at the median,
  2.7 s at worst, from a cloud container. About 600 pushes in six minutes met no rate limit and no
  error other than the expected refusals. Across every run the branch never granted more than the
  budget and never lost a grant a Mac held.
- **Many Macs asking at the same moment wait.** Twelve Macs asking at once: the median ask took
  1.7 s, the slowest 56 s, because each refused push is fetched and tried again. Five Macs asking
  0 to 20 s apart: 4.9 s median, 12 s at worst. Leases ask about once every dozen calls, so the
  burst comes at the start of a night or the end of a budget, not on every call.
- **Git cannot recall a slice, and #388's recall made 287 writes per call at the end of a budget**
  in its 2 s row (team of five, `typical-cut`), 17 for a team of twelve. Without a recall, a Mac
  that is out of room gives its leftover back with its ask, a Mac with agents running checks in
  every 5 minutes and gives back what it holds beyond its calls under way and one slice, and a
  call with nothing free waits for the next check-in. That ended the team between 2.6% under and
  the budget, at 1.1 to 1.7 pushes per call.
- **A slice must stay the Mac's until the Mac gives it back.** Letting a slice expire, so that the
  ledger takes back what the Mac had not reported, went over: up to 14% with a Mac asleep, and
  14% to 42% when GitHub could not be reached for 10 minutes, since every Mac then spends what it
  never got to report. A slice that stays the Mac's never went over. It costs a Mac that goes to
  sleep holding a slice: that room is unusable until the Mac wakes. With $2 slices, the team of
  five ended 10% to 13% under its $15; with #388's larger first slices, up to 50% under, as
  #388's own leases do when the recall reaches a sleeping Mac. On a budget of $1,000 a month, one
  $2 slice is 0.2%.
- **Every member can write the ledger, so every Mac checks every change.** A check run before a Mac
  trusts the file (`verifyledger.py`) flagged all five cheats tried: taking more than was free,
  changing another Mac's entry, raising the budget, lowering one's own reported spend, and a
  rewritten history. It needs the commit's signer to be the Mac, which a rule requiring signed
  commits on the ledger branch gives; that rule was not tried.
- **A server would not hold a member who wants out.** A member controls their own Mac and can turn
  its boundary off. A hosted ledger counts what honest Macs send, as a branch does. Only rules
  a member cannot switch off (`rules-a-member-cannot-switch-off`, #362) change that.

## What a ledger learns, wherever it runs

Each write carries a Mac's name, the time, the dollars granted and given back, and what the Mac
says it has spent so far. From those, whoever reads the ledger learns:

- when each member's agents run, to the 5-minute check-in;
- what each member spends, over time;
- how many members and Macs the team has, and its budget.

It learns no prompt, model, program, host or file.

In a branch, the readers are every member with read access and the Git host, who already holds
the team's code. That every member sees every other's spend is what `spend-the-team-sees` (#362)
asks for. A ledger that Silo ran would also give Silo all of the above, plus each Mac's address, for
every team that used it. `what-silo-sends-home` (proposed in #360) sends nothing by default, so a
hosted ledger would be the team's choice to turn on.

## What this changes in the plan, by entry

- `pooled-budget-ledger`: its answer says "When the ledger runs short, it takes back the room other
  Macs hold". Through a branch that cannot happen. The notes propose in its place: a Mac gives back
  room it cannot use when it asks, and room beyond its calls under way and one slice when it checks
  in, every 5 minutes; a slice never expires on its own. They add that every Mac checks each
  change to the branch, and that the branch needs signed commits and no force-push.
- `team-budget`, `what-a-budget-promises`, `shared-host-system`: unchanged.

## The runs on GitHub

`hostedlease.py` is #388's `gitlease.py` pointed at a hosted repository, with a second way of
asking: *optimistic* pushes on top of the last head the Mac knows and fetches only when refused (one
round trip when nobody else pushed), against *read-first*, #388's way (fetch, then push). The
branch was one of a private GitHub repository. The ledger file sat beside the branch's other files.
All 12 runs passed: granted equals held, never past the budget, no grant lost. Raw results:
[`out/github-runs.jsonl`](out/github-runs.jsonl).

| Run | Way | Asks | Pushes | Ask, median | Ask, slowest |
|---|---|---:|---:|---:|---:|
| 1 Mac, 3 runs | read-first | 21 | 21 | 2.2 to 2.3 s | 2.7 s |
| 1 Mac, 3 runs | optimistic | 21 | 21 | 1.6 to 1.7 s | 2.7 s |
| 5 Macs at once, $15 | read-first | 15 | 56 | 3.0 s | 33 s |
| 5 Macs at once, $15 | optimistic | 14 | 49 | 1.9 s | 27 s |
| 12 Macs at once, $30 | read-first | 29 | 219 | 2.6 s | 69 s |
| 12 Macs at once, $30 | optimistic | 29 | 202 | 1.7 s | 56 s |
| 5 Macs, 0 to 20 s apart, $30 | optimistic | 18 | 49 | 4.9 s | 12 s |

The client retries at once after a refusal. A random wait before retrying would likely shorten the
slow tail; not measured.

A ledger commit took about 150 bytes compressed in the repository, plus the new file and tree. At
about one push per model call, a member who makes 5,000 calls a month adds 5,000 commits to the
branch. A new branch each month keeps it small; not tried.

## The simulation

`gitsim.py` adds `gitlease` to #388's simulator and runs it next to #388's leases, 20 seeds each,
at a real model's speed (50 tokens a second). Every table is in [`out/gitsim.md`](out/gitsim.md).
The team of five is #388's: two members with Claude Code and 8 subagents, one with 2, one with a
single conversation, one idle.

Team of five, $15, #385's `typical-cut`:

| Way | Spent / budget, mean | Over, worst | Over, best | Longest wait | Writes per call |
|---|---:|---:|---:|---:|---:|
| #388's leases, 2 s, with a recall | 0.99 | −0.1% | −3.0% | 180 s | 287 |
| Git, a slice expires after 5 min | 0.99 | +3.0% | −1.9% | 265 s | 1.25 |
| Git, a slice stays the Mac's, check-in every 5 min | 0.99 | −0.4% | −1.9% | 265 s | 1.25 |
| Git, as above, $2 slices only | 0.98 | −0.5% | −2.6% | 320 s | 1.17 |
| Git, as above, pushes of 5 s | 0.99 | −0.2% | −2.7% | 310 s | 1.10 |

The longest waits come at the end of the budget: a call that waits for another Mac's check-in to
free room. Under #388's recall the same wait was a storm of writes instead.

A heavy Mac goes to sleep at 300 s holding its slice; its agents stop where they are:

| Way | Spent / budget, mean | Over, worst | Over, best |
|---|---:|---:|---:|
| #388's leases, 30 ms, the recall reaches no one | 0.76 | −1.3% | −48.4% |
| Git, a slice expires after 5 min | 1.03 | +13.6% | −0.3% |
| Git, a slice stays the Mac's | 0.83 | −6.8% | −49.9% |
| Git, a slice stays the Mac's, $2 slices only | 0.88 | −10.3% | −12.8% |

GitHub cannot be reached for 10 minutes from 600 s:

| Way | Spent / budget, mean | Over, worst | Over, best | Longest wait |
|---|---:|---:|---:|---:|
| #388's leases, 30 ms, ledger down | 0.99 | +0.0% | −7.7% | 600 s |
| Git, a slice expires after 5 min | 1.32 | +41.7% | +13.9% | 645 s |
| Git, a slice stays the Mac's | 0.99 | −0.3% | −2.2% | 1,091 s |
| Git, a slice stays the Mac's, $2 slices only | 0.99 | −0.5% | −3.0% | 638 s |

Expiry goes over because a slice is freed on what the Mac last reported, and a Mac that could not
push had spent more. With a 1-minute outage it went 10% over in one run and 90% under in another,
where every Mac cut its calls at its expiry and the agents stopped.

## The check every Mac runs

`verifyledger.py` walks the branch from the last head the Mac trusted and refuses a change that:
does not keep the trusted head in its history; changes any entry but the one of the Mac the commit
names; grants that Mac more than was free in the parent; lowers a grant, a give-back or a reported
spend; or changes the budget. Its test runs five honest Macs, then each cheat on top of their
branch:

```
honest run: clean
a Mac takes $5 more than is free: flagged: m3 took 5.0000 with 0.0000 free
a Mac lowers another Mac's grant: flagged: 'lease m3 0.0000' changed m1's entry
a Mac raises the budget: flagged: budget changed 15.0 -> 25.0
a Mac lowers its own reported spend: flagged: m2's grant, given or reported went down
history rewritten: flagged: history rewritten: the trusted head is not in the new history
PASS
```

A cheat that takes exactly what is free looks like an honest ask, in a branch as in a server.

## Not verified

- A Mac's own network. Every push went through this container's proxy to GitHub.
- GitLab, Bitbucket and self-hosted Git servers.
- GitHub's limits beyond about 600 pushes in six minutes. Its documentation gives secondary limits
  for its API (80 content-generating requests a minute) and says nothing of pushes.
- Signed commits required on the ledger branch, and which GitHub plans allow that rule on a private
  repository.
- A budget change by the team's lead, and the lead freeing the slice of a Mac that stays asleep.
  Both would be commits the check accepts from the lead's key only; not built.
- A monthly budget. The runs here are #388's: $15 spent in about half an hour.

## Running it

Python 3.13 and Git.

```
python3 verifyledger.py test                                   # the five cheats
python3 gitsim.py                                              # every table, about 15 minutes
python3 hostedlease.py URL BRANCH optimistic 5 15 2 out.json   # against a branch you may push to
```

`hostedlease.py` writes commits to the branch it is given. Point it at a branch made for the run.
