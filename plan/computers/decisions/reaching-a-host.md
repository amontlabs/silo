# Reaching a host from another network

## The choice

An agent's part, 24 November 2026. A Mac mini at home runs main. Its owner is at a café with a
laptop. Every guide for an always-on Mac read for this question uses the same thing to reach it:
Tailscale, or another private network, with SSH.

The options:

- The user's own private network (recommended). The engine listens on the network the user already
  runs and accepts a person's key, as SSH does. It costs a user without such a network a step
  before reaching main from outside home.
- A relay run by Amont Labs. It costs a server, and every byte between the user and main through it,
  against `what-silo-sends-home`.

Recommended: the user's own private network.
