# One budget shared by several Macs

Research of 25 November 2026, by an agent, for polarzero. #374 measured a budget held by one
boundary. #362 proposed a budget per team member, enforced on each Mac, and said a budget pooled
across Macs needs "a shared count that each Mac must reach before every model call". This folder
designs the ways several Macs can hold one budget, builds them, and measures how far past the
budget each one goes when many agents spend at once. Each Mac counts with #374's rule and with
#385's additions: a call under way cut once the budget is passed, and work out of sight (a batch, a
background response) held at its whole bound until its result comes back.

Nothing here is a decision. Under `plan/` it adds one open decision, `pooled-budget-ledger`, and
agents' notes on `team-budget`, `what-a-budget-promises` and `shared-host-system`. The branch is
stacked on #362's by a merge, since those entries come from #362.

## The case

A team of five has $15 a day for agents, at API prices. Two members run Claude Code with 8
subagents, one runs 2 subagents, one runs a single conversation, one runs nothing today. Each has
their own Mac with its own boundary. The budget is reached around the 17th minute at a real
model's speed. The question is what each Mac's boundary must know, and from where, to stop the
team at $15.

A member with a laptop and a Mac mini (`main-on-several-devices`) is the same problem with one
person: #362's budget "on all of a member's computers" spans two boundaries as soon as the
member's computers run on two Macs.

## In short

- **Where the count lives changes almost nothing about the overshoot. The admission rule
  decides it.** Every way that shares one count (one boundary, a ledger 1 ms or 150 ms away, a
  ledger on a shared Linux host, leases, gossip every second) ended within a point or two of each
  other. Counting a running call's output by time (#374's `output_tps`) ended the team at most
  15% over. Holding each call's typical output ended it at most 0.4% over, and at most 2.2%
  under with a ledger or leases.
- **Over a team, the overshoot is a smaller share than on one computer.** One Mac under $3 ended
  35% over in #374 and here; five Macs under $15 ended at most 15% over, twelve under $36 at most
  12%. The calls under way at the limit are the same few dollars, against a larger budget.
- **A ledger asked on every call costs one round trip per call and stops every new call when it
  is down.** 30 ms or 150 ms added to calls that last two minutes at a real model's speed is
  nothing. During a 10-minute outage new calls wait the whole outage. Refusing them instead
  stopped the agents: the team spent half its budget.
- **Leases ride out short outages.** The ledger gives each Mac a slice ($2 here, about a dozen
  calls), the Mac admits locally with #374's rule and asks for more before it runs out. While
  plenty is free, a Mac talks to the ledger once every dozen calls; recalls near the end bring the
  whole run to 2 to 6 messages per call, about what a ledger asked per call needs. During a 1-minute outage 0.1 to 0.5 calls waited over 10
  seconds per run, against 2.7 with a ledger asked on every call. A 10-minute outage outlasts any
  slice that size, and leases then wait like the rest.
- **Leases need a recall, or budget is stranded on Macs that stopped asking.** Without one, a
  team of 12 ended up to 35% under its budget: slices sat on Macs whose agents had stopped. With
  the ledger taking back unused room when it runs short, the same team ended within 0.6% of it.
- **Gossip between Macs, with no ledger, overspends when the team is cut in two.** Each half
  thinks the other stopped, and spends what is left: 33% to 45% over for a 10-minute split.
  When a Mac that cannot hear a peer limits itself to its share of what was left (an escrow), the
  same split ended at most 2.5% over. Gossip every second also sends 240 to 840 messages per
  model call, every Mac to every Mac, which laptops behind NAT and asleep cannot do.
- **Fixed shares per Mac need no talk and leave budget unspent.** Fifths of $15 ended the team 5%
  to 26% under the budget: the idle member's fifth was never spent while the heavy members were
  refused.
- **A ledger server is small, and a Git repository can be one.** The server here (`ledger/`)
  admitted 137 calls from 12 Macs in 0.2 s, never past the budget, 3.6 ms per admission on
  localhost with each change written to disk first. Killed with SIGKILL and dying between its
  commit and its answer, it never granted more than the budget and never lost a grant a Mac had
  seen. A lost answer left $1.32 stranded until requests carried an id, then none. Leases also
  work with no server at all: a push to a branch of the team's repository is a compare-and-swap,
  and 12 Macs took $30 in 32 grants with none lost.

- **With #385's cut, every way that shares one count stops the team at the budget.** Cutting
  calls under way once the total is passed ended a team of five within 0.6% over $15, for one
  boundary, a ledger, leases or gossip. It paid $1.40 to $1.90 a run for answers that were cut.
  Gossip with the team cut in two still went 35% over; with the escrow, not at all.
- **Under leases a Mac cuts on its own slice, not on the team's total.** It cannot see what the
  others spend. Here it asks the ledger once more and cuts when the ledger says nothing is left,
  or when no answer comes within a second and a round trip. That was never more than 0.2% over,
  but cut more answers than one boundary ($1.85 a run against $1.43), and through Git's 2 s pushes
  ended up to 10% under. #385's `typical-cut` (hold the typical output, cut only what passes
  anyway) avoids nearly all of it: leases ended between 2.2% under and the budget, with $0.09 a run
  cut.
- **A batch bigger than a slice is asked for, not refused.** A batch with a $4 bound, sent by a
  Mac holding a $2 slice, was admitted in every run under leases: the Mac asked the ledger for what
  was missing. Fixed $3 shares refused it every time. Holding the bound until the results came back
  left 17% of the budget unspent in every way alike; that is the cost #385's rule accepts, not the
  ledger's.

## The ways

| Way | What each Mac's boundary does | What it needs |
|---|---|---|
| One boundary | All computers run behind one boundary, which counts everything | The team's computers on one host: a shared Linux host (`shared-host-system`) |
| Central ledger | Asks the ledger before every model call, tells it when the call ends | A ledger every Mac reaches on every call |
| Leases | Takes a slice from the ledger, admits locally against it, asks for more when what is left falls under $1, gives back what it does not use when its agents end or when the ledger recalls it | A ledger every Mac reaches every dozen calls or so |
| Gossip | Tells every other Mac what it has counted, every few seconds; admits against its own count plus what it heard | Every Mac reaches every other Mac |
| Fixed shares | Holds a fixed part of the budget | Nothing |

Where a ledger can run, for the central ledger and leases:

- **On a Linux host the team owns.** The host of `shared-host-system` runs it beside the team's
  shared computers, which are behind its boundary and count exactly with no message at all.
  Members' Macs reach it over the LAN or a VPN (1 to 30 ms).
- **On one member's Mac that stays on.** The same program. A laptop that sleeps cannot serve.
- **In the team's Git repository**, for leases only. The ledger is one JSON file on a branch. A
  Mac reads it, adds its grant, and pushes a commit on top of what it read; a push that is not a
  fast-forward is refused, so two Macs never take the same dollars. No server runs. An admission
  per call is not possible this way: that would be a push per call.
- **A service Silo runs for the team.** The same program, run by Silo. It is the one place that
  sends each member's spend off the Mac to someone other than the team. `what-silo-sends-home`
  (proposed in #360) says nothing leaves by default, so this would be the team's opt-in.

What any ledger holds is dollars per Mac and per call id. No prompt, model name, file or command
reaches it: the boundary prices each call itself and sends only the amount.

## What was built

| File | What it is |
|---|---|
| `ledger/book.py` | #374's admission rule (`boundary/src/budget.zig`, total limits), with #385's whole bound for work out of sight, ported to Python, used by both the simulator and the server, so what is measured is the rule that runs |
| `ledger/server.py` | The ledger: central admission (`/admit`, `/settle`) and leases (`/lease`, `/give`), over HTTP and JSON. SQLite in WAL mode with `synchronous=FULL`; every change is committed before the answer goes out. A lease request carries an id, so a request sent again after a lost answer gets the same answer |
| `ledger/test_ledger.py` | 12 Macs at once against the server; the server killed and restarted while they ask |
| `ledger/gitlease.py` | Leases through a branch of a Git repository, with its test |
| `sim/sim.py` | A discrete-event simulation of a team: each Mac's agents, its boundary, and each way above, with network delay, a ledger that goes down, and a team cut in two. With `--cut`, #385's cut: each way decides every half second which calls under way end, and a cut call is billed for what it generated, then ends its conversation, as Claude Code did in #385 |
| `sim/experiments.py` | Every table below, 20 seeds each; the full tables are in [`out/tables.md`](out/tables.md) |

### The simulator against #374

Each Mac runs what #374 ran on its one computer: Claude Code with a main conversation of 40 steps
that starts 8 subagents of 14 steps at step 2, each call reasoning 2,000 to 12,000 tokens that the
boundary cannot see, on Opus 5.5's prices. "Medium" runs 2 subagents, "light" none. Calls stream
at 500 tokens a second, as #374's stand-ins did, or at 50, a real model's speed.

On one Mac with $3, the simulator gives #374's measured overshoot:

| Rule | #374, measured, worst of 3 | The simulator, worst of 20 (best) |
|---|---:|---:|
| `finish`: count what is visible | +54% | +50% (+28%) |
| `finish` and count output by time | +35% | +35% (+11%) |
| `reserve_typical`: hold the typical output | −7% to −9% | −2% to −6% |
| `reserve_max`: hold `max_tokens` | −82% | −79% to −85% |

## Results

Over: what the team spent past the budget, worst of 20 runs. Under: the furthest under the budget
a run ended. Wait: how long a call waited for its answer before it could leave. Messages: all the
messages between Macs or to the ledger, per model call made.

### A team of five, $15, a real model's speed

| Way | Over, output by time | Over, typical output held | Under, typical output held | Wait, p99 | Messages per call |
|---|---:|---:|---:|---:|---:|
| One boundary | +14.6% | −0.1% | −1.5% | 0 | 0 |
| Central ledger, 1 ms | +14.6% | −0.1% | −1.5% | 1 ms | 3.4 |
| Central ledger, 30 ms | +14.6% | −0.1% | −1.5% | 30 ms | 3.4 |
| Central ledger, 150 ms | +14.9% | −0.2% | −1.5% | 150 ms | 3.4 |
| Central on a shared Linux host, 2 members on it, 3 Macs at 30 ms | +14.6% | −0.1% | −1.5% | 30 ms | 3.4 |
| Leases, 30 ms | +12.3% | +0.1% | −2.2% | 120 to 180 ms | 2.3 to 5.8 |
| Leases, 30 ms, no recall | +13.5% | −0.1% | −16.3% | 30 ms | 0.2 to 0.3 |
| Leases in a Git repository, 2 s a push | +14.0% | +0.0% | −3.8% | 8 to 48 s | 2.3 to 62 |
| Gossip every 1 s | +14.6% | −1.3% | −4.1% | 0 | 240 to 290 |
| Gossip every 10 s | +14.8% | −1.3% | −4.6% | 0 | 24 to 29 |
| Fixed fifths | −5.2% | −22.1% | −25.7% | 0 | 0 |

With #374's plain `finish` (visible output only) every shared way ended up to 24% over at a real
model's speed. Twelve members under $36 gave the same picture (`out/tables.md`): at most 13% over
with output counted by time. With the typical output held, a ledger and leases ended within 1% of
the budget, gossip up to 6% under, leases with no recall up to 35% under and fixed shares 12% to
14% under.

Leases send more messages near the end than in the middle. While plenty is free, a Mac asks once
every dozen calls (0.2 messages per call without recalls). In the last few dollars, slices are
recalled and asked for again; with Git's 2 s pushes, that end game made the waits of 8 to 48 s. A
ledger can switch to admitting each call once less than a slice per active Mac is free. That was
not built.

### The ledger down, or the team cut in two

A team of five, $15, a real model's speed, typical output held unless said. The budget runs out
around 1,000 s.

| What happened | Spent / budget, mean | Over, worst | Calls that waited over 10 s, per run | Longest wait |
|---|---:|---:|---:|---:|
| Ledger asked per call, down 1 minute from 300 s | 0.99 | +0.4% | 2.7 | 60 s |
| Leases, ledger down 1 minute from 300 s | 0.99 | −0.3% | 0.5 | 52 s |
| Ledger asked per call, down 2 minutes from 300 s | 0.99 | −0.1% | 6.7 | 120 s |
| Leases, ledger down 2 minutes from 300 s | 0.99 | −0.2% | 1.2 | 112 s |
| Ledger asked per call, down 10 minutes from 600 s, calls wait | 0.99 | −0.3% | 14.6 | 592 s |
| Ledger asked per call, down 10 minutes from 600 s, calls refused | 0.52 | −34.7% | 0 | 0 |
| Leases, ledger down 10 minutes from 600 s | 0.99 | +0.2% | 8.8 | 600 s |
| Gossip, team cut in two for 10 minutes from 600 s | 1.27 | +32.5% | 0 | 0 |
| Gossip, cut, a Mac that hears nothing limits itself to its share | 0.98 | −1.1% | 0 | 0 |

With output counted by time instead, the cut team overspent by up to 45%, and with the escrow by
up to 2.5%.

A call waiting for the ledger is a held model request. #364 found a held model reply can last
indefinitely when its headers go out at once with a keep-alive, so waiting is possible for every
program #369 tested except Codex on the Responses format, which needs a real event.

### On #385's counting: a cut, and a batch held at its bound

One Mac, $3, stand-in speed, 20 runs: cutting at the budget with output counted by time ended at
0.1% under to 0.3% over, and paid $0.57 a run for cut answers; #385 measured $2.99 to $3.03 and
$0.52 to $0.92. Holding the typical output with a cut behind it ended 2% to 6% under, with nothing
cut; #385 measured 7% under.

A team of five, $15, a real model's speed. Over and under are the worst of 20 runs.

| Way | `cut`: over, worst | `cut`: under, worst | `cut`: paid for cut answers | `typical-cut`: over, worst | `typical-cut`: under, worst |
|---|---:|---:|---:|---:|---:|
| One boundary | +0.0% | +0.0% | $1.43 | −0.1% | −1.5% |
| Central ledger, 30 ms | +0.0% | +0.0% | $1.44 | −0.1% | −1.5% |
| Central ledger, 150 ms | +0.0% | +0.0% | $1.47 | −0.2% | −1.5% |
| Leases, 30 ms | +0.0% | +0.0% | $1.85 | +0.0% | −2.2% |
| Leases in a Git repository, 2 s | +0.2% | −10.1% | $1.89 | −0.1% | −3.0% |
| Gossip every 1 s | +0.1% | +0.0% | $1.44 | −1.3% | −4.1% |
| Gossip every 10 s | +0.6% | +0.2% | $1.45 | −1.3% | −4.5% |
| Fixed fifths | −19.9% | −20.0% | $1.40 | −22.1% | −25.7% |
| Gossip, team cut in two for 10 minutes | +35.3% | +8.3% | $1.60 | +24.5% | +8.3% |
| Gossip, cut in two, escrow on silence | +0.0% | +0.0% | $1.56 | −1.1% | −3.6% |
| Central, ledger down 10 minutes, calls wait | +0.0% | +0.0% | $1.18 | −0.3% | −1.4% |
| Leases, ledger down 10 minutes | +0.0% | +0.0% | $1.52 | +0.0% | −7.7% |

A ledger asked per call cuts by telling each Mac with calls under way, half a round trip later.
Under leases each Mac decides alone: its first version cut as soon as its slice was spent and the
ledger's last answer had been "nothing left", which ended the team up to 38% under through Git
and, when the ledger was down, 3% over, since a Mac waiting for an answer did not cut. The rule
above (ask once more; cut on "nothing left" or after a second and a round trip without an answer)
fixed both. Its cost is the extra cut answers in the table, and 25 to 38 messages per call at the
end of a run with plain `cut`, from recalls.

The batch: the medium member sends one at 120 s with a $4 bound, which costs $1.50 and whose
results come back at 1,500 s, after the budget is spent. Under `typical-cut`:

| Way | Batch admitted | Spent / budget, mean |
|---|---:|---:|
| One boundary | 20 of 20 | 0.82 |
| Central ledger, 30 ms | 20 of 20 | 0.82 |
| Leases, 30 ms | 20 of 20 | 0.82 |
| Gossip every 1 s | 20 of 20 | 0.80 |
| Fixed fifths, $3 each | 0 of 20 | 0.76 |

### The ledger server

`python3 ledger/test_ledger.py`:

```
central: 137 admissions from 12 Macs in 0.2 s; settled $14.8888 of $15; admission p50 3.58 ms, p99 9.20 ms on localhost with fsync. PASS
crash (without request ids): the ledger started 15 times, 210 failed requests; granted $300.0000 of $300, Macs hold $298.6799, stranded $1.3201; grants missing from the database: 0. PASS
crash (with request ids): the ledger started 11 times, 189 failed requests; granted $300.0000 of $300, Macs hold $300.0000, stranded $0.0000; grants missing from the database: 0. PASS
```

In the crash test the ledger is killed with SIGKILL every 50 to 300 ms, and it also kills itself
after committing 5% of its grants, before answering. That is the one moment a Mac cannot know
whether it got a slice. Without a request id, the Mac asks again, gets a second slice, and the
first is never spent. With one, the ledger answers the repeated request with the grant it already
made.

`python3 ledger/gitlease.py test`:

```
git leases: 12 Macs, 32 asks in 3.0 s, 271 pushes tried for 32 commits; granted $30.0000 of $30.0, Macs hold $30.0000; an ask took p50 708 ms, p99 2565 ms on a local bare repository.
```

All 12 Macs asked at the same moment, so most pushes were refused and tried again: 271 pushes for
32 grants. That is the worst case. In the team runs a Mac asks every dozen calls, a few minutes
apart.

## What this changes in the plan

- **`team-budget`** (open, from #362). Its pooled option says each Mac must reach a shared count
  "before every model call", and that when it cannot, "either the budget is no longer exact or
  every agent stops". With leases, a Mac reaches the ledger every dozen calls, and while the
  ledger is down each Mac goes on with its slice; with the typical output held, the budget stays
  within 0.4%. The pooled option costs a ledger somewhere, not a round trip per call. Its
  recommendation, a budget per member, also needs leases as soon as a member's computers run on
  two Macs. Notes added.
- **`pooled-budget-ledger`** (new, open). How several Macs hold one budget: leases from one
  ledger, with a recall, admitted on each Mac with the rule `what-a-budget-promises` settles; the
  ledger runs where the team has a place for it. Options with their costs in its notes.
- **`what-a-budget-promises`.** Over a team, the overshoot from calls under way is a smaller
  share of the budget than on one computer: at most 15% for five Macs, against 35% for one. With
  #385's options, `cut` and `typical-cut` both hold a team to its budget through any way that
  shares one count; under leases `typical-cut` cuts far less. Note added.
- **`shared-host-system`.** The shared Linux host is the natural place for a team's ledger: the
  computers on it count exactly, and members' Macs reach it already. Note added.

## Not built, or not measured

- The cut and the whole bound in the ledger server. They are in `book.py` and the simulator; the
  server admits and settles calls and leases, and would need a way to tell a Mac to cut.
- Leases inside #374's Zig boundary. The rule is ported from it and the simulator gives #374's
  numbers, but no real agent program ran against a shared ledger.
- Real networks. Delays are fixed numbers per Mac; no jitter, no packet loss, no NAT.
- A Mac that sleeps holding a slice. Its slice is stranded until the ledger takes it back. The
  design is a lease with an expiry: the Mac stops spending a slice when its own clock passes the
  expiry minus a margin, and the ledger reuses it after the expiry. Not simulated.
- Pushes to GitHub, GitLab or another host. Only a local bare repository was measured; the
  latency, the rate limits and branch protection of a hosted repository are not known.
- The end game of leases: admitting each call once little is free. Described above, not built.
- A ledger that holds several budgets at once, such as a member's own budget inside the team's.
  The server keys budgets by name, and #374 found every budget that covers a call applies
  (`budgets-together`); a call against two ledgers was not run.
- Spend that is estimated, not counted (#374: calls without the provider's final count). A ledger
  receives what the boundary priced, estimated or not, and inherits its error.

## Running it

Python 3.13 and Git. Nothing else.

```
python3 sim/experiments.py          # every table, 20 seeds, about 20 s; writes out/tables.md
python3 sim/sim.py --way lease --policy reserve_typical --members heavy,heavy,medium,light,idle --budget 15 --tps 50
python3 ledger/test_ledger.py       # the server: 12 Macs at once, and the crash test
python3 ledger/gitlease.py test     # leases through a Git branch
```
