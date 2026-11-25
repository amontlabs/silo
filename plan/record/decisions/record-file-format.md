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
