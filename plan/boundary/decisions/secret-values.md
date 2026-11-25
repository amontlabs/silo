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

## What the record must look for to keep the value out

An agent's part, 25 November 2026, from [the redaction
research](../../../research/2026-11-25-secret-redaction/README.md).

`secret-never-written-down` has no "as it is" in it, but the record format's redactor (#386) looks
only for the value as it is. On 816 samples it took the whole value in 444 and left it whole in the
other 372. A private key read by a tool reaches the record inside a JSON request body, with `\n`
for each line break, and none of the 6 such samples was caught. A token in `Authorization: Basic` or in
a base64 file is never caught.

A redactor that looks for every form of the value (JSON escaped once and twice, percent-encoded,
base64 at its three alignments, hex, shell-quoted) and follows a match across line breaks took the
whole value in 733 and part of it in 30. To keep the property:

- the recorder redacts a model's reply after the stream is put together, never stream piece by
  piece (0 of 51 raw streams were caught by any method);
- an output read in pieces is redacted as one string before it is cut into lines (19 of 51 values
  split in two were caught whole);
- it looks the forms up by windows of 16 bytes in a hashed table, not by #386's table of starting
  pairs, which falls to 6 MB/s with the forms of 200 secrets. The windowed lookup ran at 740 MB/s
  with the forms of 50 secrets and 254 MB/s with 200.

This also answers "how replies are searched without slowing them", for the record. The same lookup
fits the boundary's search of a host's replies.
