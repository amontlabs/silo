## Proposed, not confirmed by the owner

Why: A fork that goes wrong halfway can only be dropped or left to finish.

### For the owner to settle

- Do you watch in the agent program's own interface, attached in a terminal, or in a view the app draws from its session?
  - Recommendation: The agent program's own interface first, since a view in the app has to read each program's session format.
- Can one message go to every running fork's agent at once?
  - Recommendation: No; addressing many agents is the work of your agent in main (agent-lives-inside).

### For whoever builds it

- Rule: Nothing is added to the conversation in main.
- Test: Attach to a running fork's agent, send a correction and detach; the run goes on with the correction and without a restart.
