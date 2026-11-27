# The team ledger, built and run for real

Research of 25 and 26 November 2026, by an agent, for polarzero. #388 recommended that several
Macs hold one budget through leases from one ledger, with #385's `typical-cut`, but the cut and the
whole bound of work out of sight lived only in its simulator. This folder builds them into #388's
ledger server, writes the boundary's side of it (a lease client), and runs real processes against
it: one process per Mac, each streaming real model calls to #385's stand-in provider, which bills
every call itself. The ledger was killed with SIGKILL, Macs were cut off from it, one Mac was
stopped as a sleeping laptop. Every result is set beside #388's simulator run with the same
settings. Leases through a Git branch were measured against a real hosted repository.

Nothing here is a decision. Under `plan/` it changes the open entry `pooled-budget-ledger` from
#388 (its answer's recall clause, and agents' notes). The branch is stacked on #388's by a merge.

## In short

- **The simulator holds.** A team of five under $15, 4 Mac processes, 3 or 4 runs each: one
  boundary, a ledger asked per call and leases all ended between 0.4% and 1.5% under the budget
  with `typical-cut`, against 0.0% to 1.9% under in the simulator. With plain `cut`, 0.4% to 1.3%
  over against 0.0% to 0.5%. The live overshoot is a little larger because a cut call is
  settled on its output counted by time, while the provider bills what it generated before the
  socket closed. What the boundaries counted was within 0.37% of what the provider billed.
- **#388's recall cut agents while the team still had money.** Under `cut`, nothing is held for a
  running call's output, so a Mac that gave back "the room no call under way holds" was over its
  slice the moment its calls produced another token, and cut them a second later: 2.7 cuts a run
  came from "no answer" while the ledger was not empty. In one run two Macs' agents stopped at
  60 s of a 380 s run. The simulator has the same rule and showed it only as more money paid for
  cut answers ($1.73 against $1.49 for one boundary).
- **The fix: a recall keeps each running call's typical output.** No early cut in any run, $1.37
  paid for cut answers (one boundary: $1.41), and with `typical-cut` 0.4% to 0.8% under. Keeping a
  flat $1 instead ended `typical-cut` 3% to 5% under: the Macs kept dollars the refused Mac
  needed. The answer of `pooled-budget-ledger` now says this.
- **Nothing went over the budget in any fault.** Ledger killed for 30 s, or every Mac cut off from
  it: calls that waited over 10 s, per run, were 15 asked per call and 4 to 5 with leases
  (simulator: 8.5 and 1.6). Killed for 2 minutes, past the point where slices run out: 6.5 and
  18.5. Leases help for outages shorter than a slice, as #388 said, and not beyond. Refusing calls
  while the ledger was down ended the team 65% to 89% under.
- **A batch bigger than a slice is asked for and settled at its bill.** A $4.01 bound, sent by a
  Mac holding $2: admitted in every run, one boundary, central or leases, settled at what the
  provider billed ($1.56 to $1.59) to the cent. Holding it left 17% of the budget unspent in every
  way alike, as #388's simulator gave (16% to 18%).
- **A hosted Git branch is a ledger for a few Macs only.** On GitHub, one ask (read, then push)
  took 2.0 s with no other Mac asking, which is #388's assumed 2 s: a read 0.4 s, a push 1.6 s,
  and a refused push costs the same 1.6 s, since the branch refuses it on the server. Under
  contention the branch makes about one grant every 2 s whatever the number of Macs: 12 Macs
  asking every 0 to 20 s waited 10.7 s at the median and 43.6 s at worst, and 183 of 215 pushes
  were refused. It never granted past the budget.

## What was built

| File | What it is |
|---|---|
| `../2026-11-25-team-ledger/ledger/server.py` | #388's ledger, extended in place: `/admit_whole` and `/settle_whole` for work out of sight; #385's cut at the ledger (every half second, once what it counts passes the budget, it tells each Mac with calls under way to cut them); `/watch`, a long poll that carries cuts and recalls to a Mac; a recall that tells the other Macs and waits a moment for their gives before it answers; `want`, to ask for a bound larger than a slice; answers that carry the Mac's whole slice and a sequence number; with `--ttl`, slices that end when their Mac goes silent |
| `../2026-11-25-team-ledger/ledger/client.py` | The boundary's side: one interface (`admit`, `settle`, `admit_whole`, `settle_whole`, `check_cut`, `done`) for one boundary, a ledger asked per call, and leases. Requests are sent again with the same id until answered; a lease client sends one request at a time, so answers stay in order |
| `mac.py` | One Mac: the simulator's agents (8, 2 or no subagents) as threads, each step a streamed Anthropic Messages call to the stand-in provider; its boundary prices the request, admits it through the client, counts output by time, settles on the reported usage, and cuts by closing the socket |
| `netem.py` | A TCP relay per Mac in front of the ledger: half a round trip of delay each way, and a switch that cuts that Mac off (nothing forwarded, nothing new opened, as a partition) |
| `run.py` | One run: provider, ledger, relays and Mac processes on this machine, with the faults done to the real processes; the provider's bill is what the team spent |
| `experiments.py`, `compare.py` | Every run, and the tables beside the simulator (`out/tables.md`) |
| `gitbench.py`, `gitruns.sh` | #388's Git leases, against `git daemon` through the relay, and against a hosted repository |
| `test_ledger_live.py` | The server's new parts and the client, against a real server: bound refused and held, a bound bigger than a slice asked for, a recall, an expiry, a cut |

#385's stand-in provider is not copied: `run.py` takes it from #385's commit
`f1f3a320` with `git show`. The team is #388's: two heavy members, one medium, one light, one idle
(no process, since it never calls), $15, Opus 5.5's prices. The stand-in streams at 500 tokens a
second, so a run takes 2 to 4 minutes; the round trip to the ledger is 30 ms.

## Results

Spent / budget is the provider's bill over $15, the mean of the runs. Over and under are the worst
runs. The simulator ran each setting 20 times. Full tables: [`out/tables.md`](out/tables.md).

### A team of five, $15

| Setting | Runs | Spent / budget | Worst over | Worst under | Paid for cut answers | Early cuts per run | Simulator: spent / budget, worst over, worst under, paid for cut answers |
|---|---:|---:|---:|---:|---:|---:|---|
| One boundary, `typical-cut` | 4 | 0.990 | −0.6% | −1.4% | $0.00 | | 0.991, −0.0%, −1.5%, $0.00 |
| Central 30 ms, `typical-cut` | 4 | 0.989 | −0.5% | −1.5% | $0.00 | | 0.993, −0.0%, −1.5%, $0.01 |
| Leases 30 ms, `typical-cut` | 3 | 0.994 | −0.4% | −0.8% | $0.00 | 0 | 0.990, −0.4%, −1.9%, $0.02 |
| Leases, `typical-cut`, a recall keeps $1 | 3 | 0.960 | −3.3% | −4.7% | $0.00 | 0 | (as above) |
| One boundary, `cut` | 3 | 1.005 | +0.7% | +0.4% | $1.41 | | 1.001, +0.2%, +0.0%, $1.49 |
| Central 30 ms, `cut` | 3 | 1.006 | +0.7% | +0.5% | $1.40 | | 1.000, +0.2%, −0.0%, $1.49 |
| Leases 30 ms, `cut` | 3 | 1.010 | +1.3% | +0.8% | $1.37 | 0 | 1.002, +0.5%, +0.0%, $1.73 |
| Leases, `cut`, a recall keeps $1 | 3 | 1.011 | +1.3% | +1.0% | $1.72 | 0 | (as above) |
| Leases, `cut`, #388's recall | 3 | 1.009 | +1.1% | +0.8% | $1.56 | 2.7 | (as above) |

The simulator's leases use #388's recall in every row. Messages per model call, live: 4.5 asked
per call (a request and its answer for the admission and for the settle, against 3.5 counted by
the simulator, which counts a settle as one), 3.3 to 6.7 with leases (simulator 6.0 to 6.6).

How #388's recall cut early, from one run's trace (`cut`, seed 2): a Mac asks and gets $2; another
Mac runs short, the ledger recalls, and the first Mac gives back $1.99 of it, everything its
calls under way did not hold. Its slice is now what it has counted, its calls keep generating,
and its watch sees it over its slice: it asks again, gets $2, gives it back on the next recall,
and after a second still over its slice it cuts, as if the ledger had not answered.

### The ledger killed, Macs cut off from it (`typical-cut`)

| Setting | Runs | Spent / budget | Worst over | Calls that waited over 10 s, per run | Longest wait | Simulator: spent / budget, waited over 10 s |
|---|---:|---:|---:|---:|---:|---|
| Central, ledger killed 40 to 70 s | 2 | 0.994 | −0.2% | 15.0 | 30 s | 0.991, 8.5 |
| Leases, ledger killed 40 to 70 s | 2 | 0.992 | −0.6% | 5.0 | 29 s | 0.990, 1.6 |
| Central, every Mac cut off 40 to 70 s | 2 | 0.992 | −0.6% | 15.0 | 30 s | 0.991, 8.5 |
| Leases, every Mac cut off 40 to 70 s | 2 | 0.989 | −1.0% | 4.0 | 29 s | 0.990, 1.6 |
| Leases, 2 of 4 Macs cut off 40 to 100 s | 2 | 0.988 | −1.2% | 3.5 | 60 s | no model |
| Central, ledger killed 40 to 160 s | 2 | 0.994 | −0.4% | 6.5 | 120 s | 0.991, 8.6 |
| Leases, ledger killed 40 to 160 s | 2 | 0.984 | −1.0% | 18.5 | 119 s | 0.989, 11.8 |
| Central, ledger killed 40 to 70 s, calls refused | 2 | 0.233 | −64.8% | 0 | | 0.109, 0 |

A killed ledger and a partition gave the same numbers: a Mac cannot tell them apart, and both
are retried. The killed ledger lost nothing: it started again on its database, the leases it had
granted stood, and the calls under way were settled once it was back. The retries are counted as
messages: every 0.25 s a held call tries again, which made 42 to 198 messages per call over a
2-minute outage. A real client backs off.

### A Mac that sleeps holding a slice

The heavy Mac's process stopped with SIGSTOP from 30 s to 150 s, two runs each:

| Setting | Spent / budget | Worst under | Calls that waited over 10 s | Renewals per run |
|---|---:|---:|---:|---:|
| No expiry | 0.987 | −1.4% | 0 | 0 |
| 20 s leases, 5 s margin | 0.992 | −1.2% | 0 | 39 |

Both held the budget, and in this case neither starved anyone: the sleeping Mac held one $2 slice
of $15, and woke before the others ran out. With the expiry, the ledger took the slice back 20 s
after the Mac fell silent, and the Mac, on waking, found its slice ended by its own clock and asked
again. This test does not show the case the expiry is for (a Mac that never wakes, holding a slice
the others need); `test_ledger_live.py` shows the mechanism.

### Leases through a Git branch

Each Mac asks for $2 slices of a budget of $10 per Mac until nothing is left. The hosted branch is
`claude/project-thread-cdondi` of a private repository of the owner's, over HTTPS through this
container's proxy.

| Where | Macs | Asks come | Ask p50 | Ask max | Read p50 | Push p50 | Refused push p50 | Pushes refused |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| Hosted | 1 | one at a time | 2.0 s | 2.4 s | 0.41 s | 1.60 s | | 0 of 5 |
| Hosted | 4 | every 0 to 10 s | 4.2 s | 10.7 s | 0.59 s | 1.56 s | 1.54 s | 15 of 29 |
| Hosted | 4 | all at once | 2.4 s | 22.9 s | 0.53 s | 1.59 s | 1.69 s | 24 of 34 |
| Hosted | 12 | every 0 to 20 s | 10.7 s | 43.6 s | 0.57 s | 1.56 s | 1.58 s | 183 of 215 |
| Hosted | 12 | all at once | 19.3 s | 42.4 s | 0.58 s | 1.57 s | 1.61 s | 143 of 162 |
| `git daemon`, 150 ms | 1 | one at a time | 0.96 s | 1.1 s | 0.32 s | 0.64 s | | 0 of 5 |
| `git daemon`, 150 ms | 12 | every 0 to 20 s | 3.4 s | 7.8 s | 0.48 s | 0.64 s | 0.64 s | 74 of 106 |

A push to the hosted repository costs 1.6 s whether it lands or not, so a branch serves about one
grant every 2 s in all. Twelve Macs asking every 10 s on average want more than that, and queue.
In #388's simulator at a real model's speed, a team of five asks about once a minute or less in
the middle of a run, which a branch serves easily; the end of a budget, where slices are recalled and
asked again within seconds, is where it queues (#388's simulator: waits of 8 to 48 s). A recall
also cannot reach a Mac through a branch: the Mac learns of it only when it next reads, so it
would have to read the branch every few seconds while it holds a slice.

## What this changes in the plan

- **`pooled-budget-ledger`** (open, from #388). Its answer said the ledger "takes back the room
  other Macs hold and no call under way uses". Under `cut` that rule stops agents while the team
  has money. The answer now says the ledger takes back the room beyond what each Mac's calls under
  way will typically still use. Agents' notes add the live numbers, the hosted Git measurements,
  and that a Git branch suits a few Macs only. Its recommendation stands: leases from one ledger,
  run on the team's shared Linux host or a Mac that stays on.
- **`what-a-budget-promises`.** Nothing new to add: live runs give #388's numbers for `cut` and
  `typical-cut` across Macs, with `cut` up to 1.3% over where the simulator said 0.5%.

## Not verified

- **A real model's speed.** The runs at 50 tokens a second (17 minutes each) were started three
  times and lost each time to this container restarting; only the stand-in's 500 tokens a second
  was measured live. The simulator gives the same picture at both speeds.
- **Leases inside #374's Zig boundary.** The client is Python; this container has no Zig. The
  boundary's rule is #374's, ported by #388, and the counts matched the provider's bill within
  0.37%.
- **Real networks.** The relay adds a fixed delay and cuts cleanly; no jitter, loss or NAT. A
  stopped process is not a sleeping laptop: its TCP connections stay open.
- **Pushes to GitHub at scale.** Rate limits and branch protection were not met at a few hundred
  pushes. The hosted runs wrote 85 ledger commits to the branch above; it holds nothing
  else and can be deleted.
- **Few runs.** 2 to 4 runs per setting live, against 20 in the simulator.

## Running it

Python 3.13, Git and openssl, from this folder. A run takes 2 to 4 minutes.

```
python3 test_ledger_live.py                      # the server's new parts and the client
python3 run.py '{"way": "lease", "seed": 1}'     # one team run; settings in run.py's DEFAULTS
python3 experiments.py calib faults sleep batch  # every run, 4 at a time, into out/live.jsonl
python3 compare.py                               # out/tables.md, with the simulator beside
./gitruns.sh                                     # Git leases; HOSTED=<url> BRANCH=<branch> adds a hosted repository
```
