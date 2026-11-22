# What does Silo learn from model traffic

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Read a field only at its own level.** When reading a structured message, look for envelope
  fields only at the outer level, compare names on their raw bytes, and give up when the structure
  does not balance, rather than guessing where you are. Silo reads spend from model responses. A
  response whose text contains `"usage": {...}` must not change the spend Silo counts.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/rpc/scan.zig#L70))
- **Explicit transport security.** Set TLS versions, certificates and timeouts explicitly instead of
  inheriting system defaults. For the traffic Silo opens to read model calls, the helper's TLS
  settings are written down and tested.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/provider/REVIEW_SUMMARY.md?plain=1#L241))
- **Route before parsing, stream the rest.** Decide where a request goes from its headers before
  parsing its body, and write responses straight to the output. Model responses stream. The helper
  reads what it needs to count spend as it passes and keeps nothing else in memory.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/rpc/response.zig#L1))
- **Outside contracts tested against their own examples.** For each outside contract, name the
  specification that governs it, test against its published examples, and never edit an example to
  make a test pass. Each vendor's API and streaming format is tested against its published examples,
  and against captured traffic with every credential removed. Virtio and Apple's framework are named
  as the authorities for theirs.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/TESTING.md?plain=1#L451))
