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
