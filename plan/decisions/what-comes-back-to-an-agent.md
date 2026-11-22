# What may come back to an agent?

## The choice

An agent's part, 22 November 2026. An agent in main runs `git push`, and a rule denies it.

The options:

- True and readable (recommended). The agent gets an error it can read, which says the push was
  denied. It costs a hostile agent a clear view of where each limit is.
- May mislead. The push seems to succeed, or nothing comes back. It costs an honest agent turns and
  tokens spent trying again, and it can tell you the push went through.

Recommended: true and readable. Nearly all agents are honest ones doing your work, and a limit that
only holds while the agent is misled is not a limit.

## Notes

polarzero said it about the message of a hold and about a denied tool call. Those cases are the decisions `refusal-says-so` and `denied-tool-call`.

The general sentence is an agent's, put forward on 20 November 2026. polarzero has not answered it.
