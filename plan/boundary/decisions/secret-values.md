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
