# The engine's events before the recorder

To find out. The aim is that control facts never have a gap. "Recorder" is set by `processes` in
the part `engine`.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Sizes must fit their fields.** Check a payload against the length it declares, give
  decompression a limit of its own, and when encoding, refuse a size its field cannot hold. A
  message from the probe whose payload disagrees with its declared length, or that decompresses past
  a cap, is refused and recorded.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/network/rlpx/frame.zig#L72))
- **Nothing crosses before the handshake.** Do not run a higher-level protocol until the handshake
  that gates it has completed on both sides. The probe's channel carries nothing before its
  handshake, including its version, has completed. A restore or a fork brings an older probe back
  from disk, so either the helper reads every past probe version, or the probe is put in from
  outside at each start; still to decide.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/docs/context/%5Bpass%202/5%5D%20phase-8-networking%20%28Networking%20%28devp2p%29%29.md?plain=1#L127))
