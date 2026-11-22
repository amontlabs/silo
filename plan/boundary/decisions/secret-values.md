# Where can a secret's real value be found

## The choice

An agent's part, 22 November 2026. An agent in main uses your API key through Silo. The service has
a page that shows the key it was called with, and the agent asks for that page.

The options:

- Silo never lets it in or writes it down (recommended). Silo never gives the value to a computer
  and never writes it anywhere. It also removes the value from what those hosts send back, where it
  appears as it is. It costs one case the promise does not cover: a host that sends the key back in
  disguise.
- Nothing inside can ever get it. The promise covers every way, whatever a host sends back. It costs
  a promise nobody knows how to keep, so it could not be said in public.

Recommended: Silo never lets it in or writes it down. It is the strongest sentence a test can show
true, and it says where it ends.

## Notes

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
