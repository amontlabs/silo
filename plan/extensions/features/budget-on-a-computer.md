# Budgets on a computer or a conversation

## The choice

An agent's part, 22 November 2026. Main has a budget of 50 dollars. At 3 in the morning an agent
loops on a failing test and keeps calling the model. At 50 dollars its next model call is refused,
and the agent can go no further.

The options:

- Build it (recommended). You set a budget on a computer or on one conversation, and Silo enforces
  it where the agent cannot reach. It costs an agent cut short in the middle of its work until you
  raise the budget, and an end slightly over the budget, by the calls already under way. It counts
  only the providers Silo reads.
- Leave it out. Your only limit is the provider's own, on your whole account. It costs a limit per
  computer or per conversation, so one runaway agent can spend a month's worth in a night.

Recommended: build it. A limit the agent cannot reach is what makes it reasonable to leave agents
working overnight.
