# Model traffic and spend

What an agent program and its model provider say to each other, read at the boundary: model
calls, spend, budgets.

## For

To know what an agent asked its model and what the model answered, without trusting the agent
program to say so, and to act on it at the boundary before the agent program reads the reply.

## Why it is so

- **Tool calls are read from the traffic, not from hooks** (`tool-calls-from-traffic`,
  `hooks-for-tool-calls`, `no-hook-for-a-denial`). A hook runs inside the computer, where the agent
  can reach it, and it sees a call but not its traces.
- **Adapters follow providers** (`adapters-follow-providers`): three or four formats, not one
  adapter per agent program.
- **A budget is a rule or a precaution, by what the boundary can see** (`budget-rule-or-precaution`):
  the same split as everywhere else in Silo, between what is exact and what can be wrong.
- **A change to traffic is never silent** (`denied-call-in-the-reply`,
  `no-silent-change-to-traffic`), and each denied call stands alone (`each-call-on-its-own`).
- **A disagreement between an agent program's own account and the boundary is handled by a
  precaution** (`account-mismatch-precaution`): it compares a report with facts.

## One example

Silo denies a tool call that Claude Code's model asked for. At the boundary, the model's
reply is changed before Claude Code reads it, and the record holds the original tool call and its
replacement, both as facts. The agent's next call is judged on its own, like any other.
