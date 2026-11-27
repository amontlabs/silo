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

## Searching replies that come back compressed (24 November 2026)

An agent's part, 24 November 2026. "Where it appears as it is" has a catch: most replies do not
come back as they are, but compressed.

- Most programs ask for compression by default. Measured against a local server: node `fetch`,
  npm, pip, Python `requests` and Go ask for gzip. git, bun and `curl --compressed` ask for gzip,
  brotli and zstd. Plain curl, wget and Python `urllib` do not ask.
- Hosts answer with it. Through this environment's network, with the header set,
  `registry.npmjs.org`, `pypi.org` and `api.cloudflare.com` answered gzip, and
  `api.anthropic.com` and `slack.com` answered brotli. In a compressed reply, a secret's bytes do
  not appear as they are, so a search of the bytes on the wire never finds it. The boundary has to
  search the decoded body. That means gzip, brotli and zstd, and the spike of
  `forwarded-byte-for-byte` decodes only gzip and zstd.

What each step costs, on one processor of this container, for an 824 KB JSON reply (npm's
document for `express`):

| Step | Speed |
| --- | --- |
| Search the decoded body for the 3 secrets sent to that host | 4 600 MB/s |
| Decode gzip / zstd / brotli | 600 / 1 450 / 950 MB/s |
| Encode again, gzip level 6 / level 1 | 75 / 175 MB/s |
| Encode again, zstd 3 / brotli 5 / brotli 1 | 284 / 85 / 628 MB/s |
| Encode again, brotli 11, as for files compressed ahead of time | 1 MB/s |

On a 2 KB reply, every step stays above 30 MB/s, which is under 0.1 ms.

- The search is not the cost, because a host's replies are searched only for the secrets sent to
  that host, a handful at most.
- Decoding is cheap. Encoding again is the cost, and only on the replies where a secret is found.
- So a proposal, in two cases:
  - A reply that says its length, under a limit such as 1 MB, is held whole and searched before it
    goes on. Holding 1 MB costs about 2 ms of decoding. When a secret is found, the value is
    replaced by its placeholder and the body is encoded again at a fast level.
  - A longer or streamed reply goes on as it comes. The boundary decodes a copy and searches it,
    and forwards the original bytes unchanged. It holds back only the end of what it has searched
    while that end could still be the start of a secret, which in practice is almost never. When
    it finds a secret, it cuts the reply at that point, so the value never reaches the computer,
    and the record says why.
- What a cut costs: the agent sees a broken reply instead of a placeholder. It happens only when a
  host sends a secret back in a long reply, which should be rare. Whether it is rare enough is for
  the owner.

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
