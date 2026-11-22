# How the engine brings what is seen to what is wanted

## The choice

An agent's part, 22 November 2026. Main and two forks are running when your Mac restarts for an
update overnight. In the morning you open the app.

The options:

- What you asked and what is seen, kept apart (recommended). For each computer the engine keeps what
  you asked for and what it last saw, and it looks again after the restart. The app shows the three
  computers as they are. The engine acts until they are as you asked, or tells you what it is
  waiting for. It costs in-between states on screen: a computer can read as starting, as waiting for
  something, or as unknown until the engine has looked at it.
- Trust what was saved. The engine does what you ask once and remembers the result. It is simpler,
  and the app always shows a settled answer. It costs the truth after anything unplanned: in the
  morning the app shows three computers running that are not.

Recommended: kept apart. Computers die and Macs restart, and an app that shows a computer running
when it is not is worse than one that says it does not know yet. Kubernetes and NATS manage what
they run this way.

## Notes

- The record checks `wanted-and-seen-agree` with the simulator.
- What the engine does when a computer stops by itself is `computer-stops-by-itself`, in the
  part `computers`.

## Sources

- [NATS Server 2.15: desired state reconciliation](https://www.synadia.com/blog/how-we-move-streams-safely-nats-2-15-desired-state-reconciliation):
  wanted and seen, done by people who first got it wrong.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Read related values from one observation.** When state can change between reads, take related
  values from one observed object, or read twice and retry a bounded number of times until both
  reads agree. A driver reports a virtual machine's state as one observation, never as fields read
  at different moments.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/blockchain/chain.zig#L21))
- **Unknown never reads as done.** A value that means "not known yet" reports not ready, so an
  unknown target never reads as reached. A computer whose state the engine has not observed yet is
  shown as unknown, never as matching what you asked.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/sync/status.zig#L30))
