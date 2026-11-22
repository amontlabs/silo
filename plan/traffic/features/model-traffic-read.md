# Model traffic read at the boundary

## For the owner

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
