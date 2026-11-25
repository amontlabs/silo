# Where can a secret's real value be found

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 1).

The case: an agent asks an allowed host for a page about its account, and the page quotes the token
in base64 inside a JSON field. The earlier answer removes the value only "where it appears as it
is", and leaves every other form uncovered without saying which, so no test can check the promise.
#355's prototype caught the base64 echo and not a reversed one, on 7 kinds of echo.

The options:

- Name the forms that are removed: as it is, base64, and any run of 12 or more of its characters,
  in headers and bodies, compressed or not (recommended). Each removal keeps the length, and is a
  fact in the record. It costs a search of every reply over a small table per secret, and holding
  back tens of bytes of a reply; its speed in Silo's boundary is not measured yet.
- Only as it is, as the earlier answer says. It costs the base64 echo, which is common.

Recommended: name the forms. A promise a hostile test can check is worth more than a wider one it
cannot.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
