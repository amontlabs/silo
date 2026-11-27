# The rules that hold destructive calls ship as an extension

## The choice

An agent's part, 27 November 2026. `destructive-calls-held` names Railway, Vercel, Fly, Supabase,
Neon, AWS, Google Cloud, Cloudflare, Stripe and GitHub. `outcome-of-a-call` sorts each operation
into gone, recoverable, outage, money or exposed. Both are positions, about vendors and about
what counts as harm. `mechanism-or-use` asks that the core hold the mechanism.

There are two things here, and only one is a position. Reading that a request runs
`DeleteBucket` is reading a format, like reading a model call. Saying that `DeleteBucket` is
"gone" and must be held is a position.

The options:

- The reader in the core, the lists in an extension (recommended). The lists are rules, so they
  are data, shown before you install them (`rules-are-data`), and the extension needs no computer.
  Someone can ship the list for a provider Silo does not cover. It costs keeping the reader's
  formats general, with JSON-RPC added for `transaction-guard`.
- All of it in the core, as `operation-of-a-request` and `outcome-of-a-call` read today. It costs
  a release of Silo for every vendor's new API.
- All of it in an extension's computer, parsing included. It costs speed on every request, and a
  rule could no longer be exact about an operation.

Recommended: the reader in the core, the lists in an extension.

An earlier review by this agent proposed moving the parsing out of the boundary too. Writing out
`transaction-guard` changed that: its lightest form is a rule that holds `eth_sendRawTransaction`
on the hosts named, which executes nothing and needs the boundary to read the operation.
