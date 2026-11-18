## Proposed

- Silo never chooses or switches the agent's model. Falling back on overage, or moving to a stronger model after failures, is the agent program's job.
- No feature depends on a particular model, or on one more capable than today's.
- Nothing Silo does to watch or report on work spends the user's tokens.
- Anything in Silo that spends the user's money, such as forks on a cloud machine, stops at a limit the user sets.
- When a provider restricts how a subscription may be used, Silo changes nothing: the agent program and the account are the user's.
- Silo builds nothing that helps get around a provider's terms, such as rotating accounts or hiding that an agent runs in a virtual machine.

## Open

- Does Silo run a model of its own?
  - Never.
  - A local model on the Mac, for small judgements.
  - Also an optional agent that answers what is going on.
- Does Silo start agent programs only as interactive terminal sessions, never in a headless mode such as `claude -p`? Tasks from outside run with nobody watching either way, so this changes only how the use looks to the provider.
