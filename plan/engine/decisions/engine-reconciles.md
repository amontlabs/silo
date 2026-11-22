# How the engine brings what is seen to what is wanted

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
