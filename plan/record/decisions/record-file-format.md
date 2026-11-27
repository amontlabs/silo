# In what form are facts and reports kept on disk

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Small inline, large by reference, repeats once.** Store small values inline, large ones by
  reference, and repeated ones once in a shared pool keyed by their binary value. Record lines keep
  small fields inline and bulky content by hash. Paths and hosts are stored once and referred to by
  id.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/performance/SYNTHETIC-OPCODES.md?plain=1#L75))
- **Compute derived values only when asked.** Compute a summary or an index only when someone asks
  for it. Look a key up once and write through the same handle. Skip a write when the value is
  already stored. Hashes that give content its identity are computed when it arrives, since storing
  once needs them. Summaries and indexes over old lines wait until asked.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/trie/README.md?plain=1#L43))

## Measured

An agent's part, 25 November 2026, from [the record's format](../../../research/2026-11-25-record-format/README.md). The segments
of #370 (a header, then frames of length, CRC-32C and one JSON line) hold #371's real night of 3,570
lines in 1.55 MB, 268 KB with zstd at level 3, and a modelled night of three agents for 8 hours in
34.9 MB, 7.0 MB with zstd. A closed segment can be compressed as a new file, since the hashes are
over the uncompressed lines.

## The flush on a Mac

An agent's part, 25 November 2026, from [the record's head off the Mac](../../../research/2026-11-25-record-head-off-the-mac/README.md).
On a Mac, `fsync(2)` hands the data to the drive, which may keep it in its cache and write it out of
order; only `fcntl(F_FULLFSYNC)` makes it durable. Zig's `std.posix.fsync`, `fdatasync` and
`File.sync` call plain `fsync` on macOS (Go and Rust issue `F_FULLFSYNC`), so a writer built on them
keeps nothing through a power cut: in a simulation of 4,100 power cuts, acknowledged lines were lost
in 394 states and the record could not start again in 3,376. The writer calls `F_FULLFSYNC` itself,
and `fsync` on a volume that refuses it. `F_FULLFSYNC` before each acknowledgement and
`F_BARRIERFSYNC` for the writes before it lost nothing either; what each costs on a Mac is #359's
test 06.

## The flush on a Mac, measured

An agent's part, 25 November 2026, from [durability on a Mac](../../../research/2026-11-25-durability-on-a-mac/README.md). The writer takes
`SILOREC_FLUSH=full|barrier|fsync` and `silorec bench` times an acknowledgement. The Mac kit's
test 50 runs it on APFS, quiet and while another program writes, and test 51 cuts the power under
it: a macOS computer on a stand-in drive that keeps writes in a cache until a flush and loses part
of the cache in the cut. Each new blob costs three flushes (the file and two folders) before its
line is acknowledged; on Linux, while another program wrote, that was 117 ms at the 99th
percentile against 0.6 ms for a line, so the barrier rule may matter more for blobs than for lines.

## More research on this (27 November 2026)

An agent's part, 27 November 2026. These reports bear on this entry, and its notes did not point to them. The answer is unchanged.

- `research/2026-11-26-headline-numbers/` (#439): the record format's numbers came out the same on a second run: crash-safe at 209 of 209 steps, 16 of 18 tampers found, and 49.6 MB against 5.5 MB for a heavy night's model-call lines.
