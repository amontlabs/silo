# A team's budget

## The choice

An agent's part, 24 November 2026. A team lead gives each of 12 developers $1,500 a month for
agents. In September one of them spent $2,200. This is a real case: The Pragmatic Engineer
collected spending at 27 companies in October 2026, and 12 of them spend $1,000 or more per
developer a month. Caps are common, and "most people find them easy to get around".

Why caps are walked around today: each vendor's limit covers only its own product. Claude's Team
and Enterprise limits apply to the seat; a personal account, an API key from another organisation
or another program is outside them. A gateway such as Tailscale's Aperture holds a quota per person
across providers, but only for traffic sent through it. Silo's boundary sees every model call of a
computer, whatever program makes it and whatever key it carries.

The options:

- A budget on all of a member's computers, enforced on that Mac (recommended). The team's rule set
  carries "$1,500 a month for each member", and each Silo enforces it as a rule, exactly, like a
  budget on a computer. The team total is the sum of what the Silos report, and is shown as a sum,
  not as a limit. It costs a pooled budget: a member who spends little does not leave room for one
  who spends more.
- A pooled budget across Macs. All Silos count against one total. It costs a shared count that each
  Mac must reach before every model call, so a server, or Macs that wait on each other. When one
  cannot be reached, either the budget is no longer exact or every agent stops.
- No team budget. Each member sets their own. It costs the case above.

Recommended: a budget on all of a member's computers. It is exact, needs no server, and is the
limit leads already set by hand. A pooled budget is exact where the computers of several people
run on one host (`shared-host-system`), and can come with it.

What this does not cover, said plainly: agents a member runs outside Silo on the same Mac. A budget
holds what crosses Silo's boundary.

The word `budget` today means a limit on "a computer, a conversation or a run". A budget on all of a
member's computers adds a scope, and taking this decision would change that word.

## A pooled budget, built and measured (25 November 2026, by an agent)

The pooled option above says each Mac must reach a shared count "before every model call", and
that when it cannot, "either the budget is no longer exact or every agent stops". A simulation of
a team of five on its own Macs, with #374's counting on each, found a cheaper way
(`research/2026-11-25-team-ledger/`, and `pooled-budget-ledger`):

- A ledger can hand each Mac a slice of the budget, about a dozen calls' worth, and each Mac
  admits on its own until it needs more. A Mac then reaches the ledger every dozen calls, not on
  every call.
- While the ledger is down, each Mac goes on with its slice. With each call's typical output held,
  the team ended between 2.2% under and 0.4% over $15, outage or not.
- What a pooled budget costs is a ledger somewhere: the team's Linux host, a Mac that stays on, or
  a branch of the team's repository, where a refused push keeps two Macs from taking the same
  dollars.

The recommended answer, a budget on all of a member's computers, meets the same question as soon
as one member's computers run on two Macs (`main-on-several-devices`): two boundaries count
against one limit. `pooled-budget-ledger` answers both cases the same way.

## Where the drafts of 25 November disagreed (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

#362's pooled option says each Mac must reach a shared count before every model call, and that when it cannot, the budget is no longer exact or every agent stops. #388 measured leases instead: a Mac reaches the ledger about every dozen calls and goes on with its slice while the ledger is down, and the budget held within 0.4% with the typical output held. Recommended: the pooled option's cost is a ledger somewhere, not a round trip per call, as `pooled-budget-ledger` says.

## Settled in the consolidation (27 November 2026)

Settled, 27 November 2026, in the consolidation: the answer now says a team total across Macs is held only with `pooled-budget-ledger`'s ledger, and is a reported sum without it.
