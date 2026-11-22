# Refusals

## The choice

An agent's part, 22 November 2026. A rule denies a connection from main to paste.rs. The agent's
`curl` gets back an answer that says the request was denied, and the agent tells you so and goes on
with something else.

The options:

- Build it (recommended). A refused request comes back as denied, or as held and not answered. It
  costs you one thing: an agent learns that a limit exists at that spot. How the answer is given on
  a connection that is not HTTP is still to find out, and what the reason may say is still open.
- Leave it out. A refused request fails like a network that is down. It costs time and tokens:
  agents retry, wait for timeouts and try other ways to do the same thing.

Recommended: build it. What comes back to an agent should be true, and a rule holds whether the
agent knows it or not.
