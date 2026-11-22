# Can a fork run with no network

## For the owner

An agent's part, 22 November 2026. You run a benchmark in a fork, and afterwards you want to show
that the agent looked nothing up online.

The options:

- The record says it stayed closed (recommended). The fork runs with its network closed, and its
  record says the network was closed from start to end, with each attempt that was denied. It costs
  a run that fails when it needs to download something midway, so everything has to be in the fork
  before it starts.
- Closed, and nothing more is said. You read the record yourself to check that no connection was
  made. It costs you that reading, and whoever you show the result to has to take your word.

Recommended: the record says it stayed closed. A result is worth more when the record can back it
without anyone reading every line.
