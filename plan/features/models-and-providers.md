## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Does Silo run a model of its own: (a) never, (b) a local model on the Mac for small judgements, or (c) also an optional agent that answers what is going on?
  - Recommendation: talk it through. (a) fits the starter, and (b) keeps judgement on the Mac if fewer-asks later needs it.
- Silo never chooses or switches the agent's model; falling back on overage or moving to a stronger model after failures is the agent program's job.
  - Recommendation: yes. Silo runs the program unmodified, and the program knows its own limits.
- No Silo feature depends on a headless mode such as `claude -p`, and Silo starts agent programs as interactive terminal sessions.
  - Recommendation: talk it through. Tasks from outside run with nobody watching either way, so the rule changes only how the use looks to the provider.
- When a provider restricts how a subscription may be used, Silo changes nothing, since the agent program and the account are the user's.
  - Recommendation: yes. Silo runs the vendor's own program unmodified, so the terms bind the program and the account.
- Silo builds nothing that helps get around a provider's terms, such as rotating accounts or hiding that an agent runs in a virtual machine.
  - Recommendation: yes. Helping evade terms would put every user's account at risk.
- Anything in Silo that spends the user's money, such as forks on a cloud machine, stops at a limit the user sets.
  - Recommendation: yes. resource-floors already handles disk with a floor, so this line is about money.
- Nothing Silo does to watch or report on work spends the user's tokens.
  - Recommendation: yes. Users pay for the agent's tokens and would not expect Silo to spend more of them.
- No Silo feature depends on a particular model, or on one more capable than today's.
  - Recommendation: yes. A feature that needs a better model fails today and on any cheaper one.
