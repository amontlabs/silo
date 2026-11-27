# Model traffic read at the boundary

## The choice

An agent's part, 22 November 2026. You use Claude Code on your Max plan in main. Silo reads what
passes between Claude Code and Anthropic: each turn, each tool call, and what it cost.

The options:

- Build it (recommended). You sign in inside main as usual, and Silo reads the traffic the way a
  company's proxy does. It costs a certificate of Silo's that each computer trusts, and a program
  that refuses it cannot reach its model. Two questions on Anthropic's terms are still open: what it
  means by "intermediate", and whether Silo's recipes fall under its rule for products that run
  Claude Code.
- Leave it out. The record holds only hosts and sizes for model traffic, with no turns, tool calls
  or spend. It costs budgets and denied tool calls, which cannot exist without it, and what an agent
  did then comes only from what its program says.

Recommended: build it. Nearly everything the record says about an agent's work, and every budget,
rests on it.

## In the first build

An agent's part, 24 November 2026, from [the comparison of first builds](../../../research/2026-11-24-first-build-and-the-record/README.md). #348 puts this feature in the third build. The
comparison proposes it in the first, beside main and its checkpoints, because the record of what
agents sent and spent is what no other product found gives, and the first build otherwise records
only which files changed. It costs 19 more open decisions, most of them already tried in #347,
#367, #369 or #371.

One need would have to go. This feature needs `secrets-held-outside`, which needs
`open-limited-or-closed` and with it the rule engine. Reading traffic does not need a secret held
outside: a subscription's sign-in stays in the program (`logins`), and an API key the program keeps
in main is read like any other request. Holding that key outside would come in the second build.

## Under option A, it needs the key-or-plan setting (25 November 2026)

An agent's part. With `secrets-held-outside` cut from its needs, as option A of #373 does, this
feature has no way left to keep a Claude plan unread if Anthropic says no. `claude-key-or-plan` is
that way; if polarzero takes both, this feature should need it.
