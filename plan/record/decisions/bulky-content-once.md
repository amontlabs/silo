# Bulky content

The design constraints of the store say the same: "Content stored once by hash for message text,
command output and recordings."

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Use a real hash for identity.** Never use XOR or another non-cryptographic mix as a digest for
  identity or deduplication. The record stores bulky content once by hash. With a weak hash, an
  agent could craft two contents with the same digest and make the record show the wrong one.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/CODEBASE_REVIEW.md?plain=1#L895))
