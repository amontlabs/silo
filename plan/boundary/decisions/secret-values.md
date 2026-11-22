- The first sentence is row 5 of the record's section 10. The two others are lines of 4.3. All
  three are marked Notes.
- `secret-never-written-down` speaks of "a real value": the line sits among the lines on secrets.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Secrets: constant time, wiped, never logged.** Operations on secret values run in constant time,
  buffers that held a secret are wiped on release, and diagnostics never print one. `secret-values`
  keeps a secret's real value out of the computer and the record. The helper still handles real
  values at the boundary: it compares them in constant time, wipes the buffer after use, and never
  prints one in a diagnostic.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/kzg/README.md?plain=1#L238))
