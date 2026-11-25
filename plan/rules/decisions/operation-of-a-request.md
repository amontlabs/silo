# How does a rule tell what a request to an API does

## The choice

An agent's part, 24 November 2026. Railway's API takes every call, a read or a delete, as
`POST /graphql/v2`. An agent sends `mutation { a: volumeDelete(volumeId: "…") }`, with an alias
and an operation name of its own choosing. AWS RDS takes `POST /` with `Action=DeleteDBInstance`
in the form body. A Git branch delete is a line of zeros inside the body of a push.

The options:

- By the operation (recommended). The set of rules for a host lists the operations of its API,
  and the boundary reads which one each request runs: the GraphQL root field after parsing, the
  AWS action, the MCP tool and its arguments, the ref updates of a push, the SQL statements. What
  it cannot read on such a host is held. It costs a parser per kind of API, and a list per host
  that has to follow the provider's changes.
- By method and path. A rule holds `DELETE` requests and named paths. It costs every case above:
  Railway, RDS, DynamoDB and Git would pass, while a Heroku dyno restart, which is a `DELETE`,
  would be held.

Recommended: by the operation. A prototype reading the operation held 220 of 226 requests that
should be held, held 1 harmless one, and decided all 74 requests captured from eleven real CLIs
right, in 0.06 ms per request at the median.

## An agent's notes

- The prototype's rules are data (`rules.json`, 17 APIs), which `rules-are-data` and
  `rule-carries-its-data` ask for. `research/2026-11-24-destructive-calls/details/detector.md`.
- The six requests it let through, and why the boundary cannot see most of them (a delete run
  from inside the provider, a force push without the commit graph, SQL whose `WHERE` matches every
  row), are listed in `research/2026-11-24-destructive-calls/README.md`.
- Holding what cannot be parsed is a rule on facts: the host and the failed parse are both seen
  (`rule-reads-only-facts`).
- It changes what "only read" means for a secret (`secret-limits`, answered): GraphQL APIs and
  AWS send reads as POST, so a list by method lets writes through or refuses reads.
- SQL sent inside HTTPS (Supabase's `/database/query`, Neon's `/sql`, Turso, Cloudflare D1)
  should use the same classifier as the Postgres proxy designed in `research/2026-11-24-gap-fixes/`.
