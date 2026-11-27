# One budget across several Macs

## The choice

An agent's part, 25 November 2026. A team of five has $15 a day for agents at API prices. Two
members run Claude Code with 8 subagents, one runs 2, one runs a single conversation, one runs
nothing today. Each member's computers run on their own Mac, behind that Mac's boundary. The
same question comes up with one person: a member with a laptop and a Mac mini
(`main-on-several-devices`) under a budget "on all of a member's computers" (`team-budget`) has
two boundaries counting against one limit.

Each way below was built and run in a simulation of that team, with #374's counting on each Mac,
20 runs each; the ledger was built as a server and tested under crashes
(`research/2026-11-25-team-ledger/`). With the typical output of each call held (#374's
`reserve_typical`), every way that shares one count ended the team between 2.2% under and 0.4%
over the budget. With output counted by time, at most 15% over, against 35% for one computer
under $3 in #374. So the choice is about what each way needs and what breaks it, not about
precision.

The options:

- Leases from one ledger (recommended). The ledger gives each Mac a slice, $2 here, about a dozen
  calls. The Mac admits on its own against its slice and asks for more when less than $1 is left.
  When the ledger runs short, it takes back unused room from the other Macs. A Mac talks to the
  ledger once every dozen calls while plenty is free. While the ledger cannot be reached, each
  Mac goes on with its slice: in a 1-minute outage, 0.5 calls per run waited more than 10 s,
  against 2.7 with a ledger asked on every call. It costs a ledger somewhere, and a recall:
  without one, slices stayed on Macs whose agents had stopped, and a team of 12 ended 35% under
  its budget.
- A ledger asked before every model call. The simplest to get right: one count, as exact as one
  boundary. It costs a round trip per call (30 ms to 150 ms, nothing against calls that last
  minutes), and while the ledger is down every new call waits for it. Refusing calls instead
  stopped the agents: the team spent half its budget.
- Gossip between Macs, no ledger. Each Mac tells every other what it has counted every few
  seconds. It costs every Mac reaching every other, 240 to 840 messages per model call, which
  laptops asleep or behind NAT cannot do. When the team was cut in two for 10 minutes, each half
  spent what it thought was left: 33% to 45% over. A Mac that limits itself to its share when it
  cannot hear a peer brought that to 2.5%, which is leases with no ledger to rebalance them.
- Fixed shares, no talk. Each Mac holds a fifth. It costs the budget the quiet members do not use:
  the team ended 5% to 26% under while its heavy members were refused.

Where the ledger runs, for leases:

- On the team's shared Linux host (`shared-host-system`), beside the computers that run there,
  which count exactly behind its boundary with no message at all.
- On a member's Mac that stays on. A laptop that sleeps cannot serve.
- On a branch of the team's repository. A push that is not a fast-forward is refused, so a push is
  a compare-and-swap: 12 Macs took $30 in 32 grants and never more. No server runs. An ask took
  0.7 s to 2.6 s against a local repository; a hosted one was not measured.
- A service Silo runs for the team. It is the one place a member's spend would leave the Mac for
  someone other than the team, so it would be the team's choice to turn on, beside
  `what-silo-sends-home`.

Recommended: leases from one ledger, with a recall, run where the team has a place for it. They
keep the budget as exact as one boundary, need the ledger only every dozen calls, and ride out a
ledger that is down for a minute or two. A longer outage outlasts a slice, and calls then wait
for the ledger like any held model request (#364).

With #385's options for `what-a-budget-promises`, the recommendation holds, with one difference.
Cutting calls under way once the budget is passed held a team of five to its budget through one
boundary, a ledger or leases. Under leases a Mac cannot see what the others spend, so it cuts on
its own slice after asking the ledger once more: never more than 0.2% over, but it cut more answers
than one boundary ($1.85 a run against $1.43) and, through a Git repository's 2 s pushes, ended up
to 10% under. Holding the typical output with a cut behind it (`typical-cut`) cut $0.09 a run and
ended within 2.2% of the budget. A batch whose bound is larger than a Mac's slice is asked for:
under leases a $4 batch was admitted in every run, where fixed $3 shares refused it every time.

Not built yet: a lease that expires, so a slice held by a Mac that went to sleep comes back; and
admitting each call at the ledger once less than a slice per Mac is free, which would remove the
recalls at the end of a budget (they made calls wait up to 48 s through a Git repository).

## Where the drafts disagree, second pass (integration)

An agent's part, 25 November 2026, written while merging #390 and #392 to #396 onto #391 (`research/2026-11-25-integration-2/`).

#392's `pooled-budget-on-a-host` answers the same question for a team on one host, with one exact count. The note there proposes that the host's count is this ledger, so one team budget has one count wherever its computers run.

## The ledger's commit on a Mac

An agent's part, 25 November 2026, from [durability on a Mac](../../../research/2026-11-25-durability-on-a-mac/README.md). The team ledger commits each
grant to SQLite with `synchronous=FULL`. On a Mac that is a plain `fsync(2)`, which leaves the
commit in the drive's cache, so a ledger on a member's Mac that stays on can lose a grant a Mac
already spends from in a power cut, and grant the same dollars twice. SQLite asks for
`F_FULLFSYNC` only with `PRAGMA fullfsync` and `PRAGMA checkpoint_fullfsync`, both off by
default; the prototype now sets both.

## Through a branch of the team's repository

An agent's part, 25 November 2026 (`research/2026-11-25-team-budget-in-git/`). The leases above
were run against a real GitHub branch, and simulated as a branch can actually serve them.

A branch works as the ledger. One push asks for a slice: 1.6 to 1.7 s at the median from a cloud
container, 2.7 s at worst. Twelve Macs asking at the same moment waited up to 56 s, since each
refused push is read again and retried. In about 600 pushes the branch never granted more than the
budget and never lost a grant.

A branch cannot call a Mac, so the recall in the answer above cannot happen there. #388's 2 s row
kept it, and at the end of a budget it made 287 writes per model call for a team of five. In its
place, in the simulation of the same team:

- a Mac that is out of room gives back what it holds when it asks;
- a Mac with agents running checks in every 5 minutes and gives back what it holds beyond its
  calls under way and one slice;
- a call with nothing free waits for the next check-in of a Mac that holds room, and is refused
  only when no Mac holds any.

That ended the team between 2.6% under and the budget, at about one push per model call.

A slice must stay the Mac's until the Mac gives it back. A slice that expired, with the ledger
taking back what the Mac had not reported, went up to 42% over when GitHub could not be reached for
10 minutes: each Mac had spent what it could not push. A slice that stays the Mac's never went
over. Its cost is a Mac that sleeps holding a slice: that room waits for the Mac to wake. With $2
slices, that left the team of five 10% to 13% under $15. With the larger first slices above, it left
up to 50%, and so did the recall above, which a sleeping Mac does not answer.

Every member can push to the branch, so each Mac checks every change before it trusts the file:
only the pushing Mac's entry changes, by no more than was free, nothing goes down, the budget stays,
and the history is not rewritten. A check that does this caught each of five cheats. It needs the
commit's signer to be the Mac, so the branch requires signed commits and refuses force-pushes.

Proposed in place of the answer's recall: "A Mac gives back the room it cannot use when it asks, and
the room beyond its calls under way and one slice when it checks in. A slice is the Mac's until the
Mac gives it back." Nothing else in the answer changes.
