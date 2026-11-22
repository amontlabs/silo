# Model calls in the record

## The choice

An agent's part, 22 November 2026. Two days later you ask why main's agent deleted the migrations
folder. The record shows the turn: the model that answered, its tool call `rm -rf migrations`, what
came back, and the conversation it belongs to.

The options:

- Build it (recommended). Each model call is in the record as something Silo saw itself. It costs
  disk: a long conversation is sent again in full on every turn, and Silo has to keep it without
  storing every copy, which is still to measure.
- Leave it out. To see a turn, you read the history the agent program keeps, which an agent can
  change. It costs the link from a change in the computer back to the turn that asked for it.

Recommended: build it. Why an agent did something is the question the record is for, and this
answers it with what Silo saw.

## An agent's notes (21 November 2026)

From the owner's list of smaller features, where most are extensions (`what-ships-with-silo`).

- For `spend-split-and-forecast`, `budgets-on-rates` and `plan-limit-reached`, each model call
  needs as facts: the effort level and speed mode requested, the cache reads and writes, and a
  provider's answer that a limit is reached, with the reset time it gives. Extensions can show and
  alert on these only if the record holds them.
