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
