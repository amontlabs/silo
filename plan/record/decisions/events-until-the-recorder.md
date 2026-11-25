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

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The specs keep the engine's events
in a ring of 16,384 rows that is never overwritten before the recorder acknowledges, and refuse a
call with `record_behind` when the room left would not hold what the effects under way may still
add. The finding: with no recorder the ring fills, so the recorder has to be built with the engine,
in step 1, which is earlier than #348 puts `record-of-a-computer`.
