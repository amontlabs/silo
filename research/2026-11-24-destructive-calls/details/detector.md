# A detector for destructive calls, measured

A prototype of what Silo's boundary would run on a request it has already decrypted. It takes
(host, method, target, headers, body bytes, facts from the boundary) and returns:

- `decision`: `hold` or `allow`. Allow only means the detector does not hold it. Silo's own rules
  still decide if the request may leave.
- `verdict`: `destructive`, `unknown`, `harmless`, or `not covered` (no rule names the host).
- `says`: one sentence per thing a yes would let pass, such as
  "deletes the Railway volume 5f2c9b1e, with the backups stored on it".
- `facts`: what it read to decide (GraphQL root fields, SQL statement kinds, AWS service and
  operation, git ref changes, the key mode of a Stripe key), and per match only the values its
  sentence names. Headers and bodies are never copied into facts.

Everything is in `proto/detector/`. Run `python3 measure.py` for the numbers and
`python3 -m pytest -q` for the tests (11 tests, under 1 s). Needs `graphql-core`, `sqlglot`,
`brotli`, `zstandard` from pip.

| File | Holds |
|---|---|
| `rules.json` | the rule set, as data: 17 APIs, 110 rules plus 14 AWS parameter rules. The Zig engine could load the same file |
| `detector.py` | request reading (hosts, path, method overrides, body decoding) and the REST, GraphQL, SQL, MCP and git matchers |
| `awsmatch.py` | the AWS matcher; loads `../aws_destructive.json` (3008 operations from botocore, made by another agent) |
| `sqlclass.py` | the SQL classifier |
| `schemas/` | Railway's and Fly's mutation names, from public introspection on 2026-11-24 |
| `corpus_build.py` → `corpus.jsonl` | 298 labelled requests: 193 plain, 105 evasion attempts |
| `capture_labels.py` | turns the 74 distinct requests in `proto/capture/captures/*.jsonl` (real CLIs) into labelled cases |
| `measure.py` → `results.json` | confusion per family, unknowns, misses, timing |
| `check_rules_against_specs.py` | checks every REST template against the provider's OpenAPI file |

## Results

372 requests. Positive means "should be held". Unknown counts requests whose verdict was unknown
(all of them held, as the rule for their host says).

| Family | Kind | n | TP | FP | TN | FN | Unknown |
|---|---|---|---|---|---|---|---|
| railway | plain | 25 | 10 | 0 | 15 | 0 | 0 |
| railway | evasion | 41 | 40 | 0 | 0 | 1 | 7 |
| github (REST + GraphQL) | plain | 20 | 12 | 0 | 8 | 0 | 0 |
| github | evasion | 3 | 3 | 0 | 0 | 0 | 0 |
| git push | plain | 4 | 1 | 0 | 3 | 0 | 0 |
| git push | evasion | 2 | 1 | 0 | 0 | 1 | 0 |
| vercel | plain | 14 | 8 | 0 | 6 | 0 | 0 |
| vercel | evasion | 11 | 11 | 0 | 0 | 0 | 0 |
| fly | plain | 10 | 5 | 0 | 5 | 0 | 0 |
| fly | evasion | 1 | 1 | 0 | 0 | 0 | 0 |
| heroku | plain | 9 | 4 | 0 | 5 | 0 | 0 |
| heroku | evasion | 2 | 2 | 0 | 0 | 0 | 0 |
| cloudflare (incl. D1 SQL) | plain | 14 | 8 | 0 | 6 | 0 | 0 |
| cloudflare | evasion | 3 | 3 | 0 | 0 | 0 | 0 |
| stripe | plain | 15 | 7 | 0 | 8 | 0 | 0 |
| stripe | evasion | 4 | 4 | 0 | 0 | 0 | 0 |
| supabase (management + SQL) | plain | 23 | 11 | 0 | 12 | 0 | 0 |
| supabase | evasion | 11 | 9 | 0 | 0 | 2 | 3 |
| supabase data API (PostgREST, Storage) | plain | 6 | 3 | 0 | 3 | 0 | 0 |
| supabase data API | evasion | 2 | 1 | 0 | 0 | 1 | 0 |
| neon (API + SQL over HTTP) | plain | 10 | 6 | 0 | 4 | 0 | 0 |
| neon | evasion | 2 | 2 | 0 | 0 | 0 | 0 |
| turso | plain | 4 | 2 | 0 | 2 | 0 | 0 |
| turso | evasion | 4 | 4 | 0 | 0 | 0 | 1 |
| aws | plain | 21 | 9 | 1 | 11 | 0 | 0 |
| aws | evasion | 11 | 11 | 0 | 0 | 0 | 1 |
| gmail | plain | 5 | 2 | 0 | 3 | 0 | 0 |
| gmail | evasion | 2 | 2 | 0 | 0 | 0 | 1 |
| mcp | plain | 13 | 5 | 0 | 8 | 0 | 0 |
| mcp | evasion | 6 | 5 | 0 | 0 | 1 | 0 |
| real CLI captures (aws, fly, gh, git, heroku, neon, railway, stripe, supabase, vercel, wrangler) | capture | 74 | 28 | 0 | 46 | 0 | 0 |
| **total** | | **372** | **220** | **1** | **145** | **6** | **13** |

99 of 105 evasion attempts were held. All 74 captured CLI requests were decided right.

Read these numbers with care. I wrote the corpus while I wrote the rules, so the plain and evasion
rows measure what I thought of, not recall on traffic nobody has seen. The captures are the
honest part: other agents made them by running the real CLIs, and they found one wire form I
had not written a rule for (below).

Timing, per request, median of 30 runs each, Python 3.13 on this container: median 0.056 ms,
p99 0.26 ms. The slowest request is the decompression bomb (9 MB of gzip'd padding) at 27.7 ms,
which stops at the 8 MiB read limit and comes back unknown. The slowest normal request is a large
Railway CLI query at 0.6 ms. A Zig port should be faster; the costs that matter are parsing
GraphQL and SQL, and both are bounded by the body limit.

### Unknown requests (all held)

- GraphQL text that does not parse (a `\u` escape in a field name is a syntax error in GraphQL).
- A JSON body naming the same key twice. Railway runs the last `query` key (probe below), so a
  harmless first key could hide a mutation.
- A persisted query: only a hash is sent, so the fields are not visible. The rule format has a
  `persisted` table (hash → document) for hashes a user registers; an unknown hash is unknown.
- A mutation field on Railway that is not in Silo's copy of Railway's public schema. This is how
  `/graphql/internal` is covered: its schema is not public (introspection is off), so anything
  beyond the public mutations is unknown.
- A body Silo did not read whole (boundary fact `body_complete: false`), a decompressed body over
  8 MiB, a Content-Encoding Silo does not read (`compress`).
- SQL the classifier cannot read: `DO` blocks, `dblink_exec` and other functions that run SQL from
  a string, `CREATE FUNCTION / PROCEDURE / TRIGGER / RULE`, and SQL that does not parse.
- A Turso statement that runs SQL stored by an earlier request (`sql_id`).
- CloudFormation `ExecuteChangeSet` and `UpdateStack`: they may delete, and the request does not
  say what.
- A Google batch request (`POST /batch/gmail/v1`, multipart/mixed with requests inside). The
  prototype does not open it.

## Every request that got through, and the one wrongly held

| Case | Why it passes | What would catch it |
|---|---|---|
| Railway: `serviceCreate` whose start command runs `railway volume delete` inside Railway | The deletion happens on Railway's machines and never crosses the boundary | Nothing at the boundary. Only a token that cannot delete volumes (Railway offers none narrower than project scope, see `paas.md`) |
| `git push --force` that rewrites `main` | receive-pack carries old and new commit, not whether new descends from old; only the server's history can tell | Silo could hold every non-fast-forward if it fetched the old commit first, or hold every push to a named branch. Deleting a ref (new commit all zeros) is held |
| SQL `DELETE FROM users WHERE id IS NOT NULL` | The WHERE names a column, so the classifier counts it as a filter | Nothing simple. `WHERE 1=1` and `WHERE id = 5 OR true` are caught (constant or simplifies to true) |
| SQL `SELECT public.cleanup_everything()` | Calling a function is a read to the classifier; what the function does was created earlier | Holding `CREATE FUNCTION` (done: unknown). Functions that already exist stay out of reach |
| PostgREST `DELETE /rest/v1/users?id=gte.0` | It has a filter; that the filter matches every row is a fact about the data | A rule on DELETE with only range filters, or Supabase's own `pg_safeupdate`-style guard (unverified on Supabase) |
| MCP tool `cleanup_project` that wipes data | The generic MCP rule goes by tool name | Per-server rules naming tools (done for Supabase's MCP server) |
| Wrongly held: DynamoDB `DeleteItem` of one item | The botocore catalogue holds every `Delete*` operation | An allow list per operation, if the user agrees one-row deletes may pass. I left it held |

## What the probes showed (2026-11-24, unauthenticated, harmless requests only)

Railway, `https://backboard.railway.com/graphql/v2`:

- `backboard.railway.app` answers the same API. The public docs name only `.com`
  (https://docs.railway.com/reference/public-api, read 2026-11-24).
- `/graphql/internal` also answers GraphQL (Apollo Server, introspection off). A rule on
  `/graphql/v2` alone misses the dashboard API.
- Accepted: gzip and deflate bodies, a UTF-8 BOM, a trailing slash, `//graphql/v2`, `/graphql/%76%32`.
  The path is case-sensitive. `;x=1` is a 404.
- Refused ("Problem processing request"): GET with a query, batched arrays, `application/graphql`,
  `text/plain`, form bodies, persisted query hashes, a document with two operations and no
  operationName, `variables` sent as a string. br and zstd: "Unsupported Content-Encoding".
- Duplicate `query` keys: the last one ran.
- Introspection of `/graphql/v2` works without a token: 255 mutations. A `*Delete` glob misses
  `projectScheduleDeleteForce`, `sandboxDestroy`, `deploymentRemove`, `volumeInstanceBackupRestore`
  (which takes `wipeServiceIds`) and `environmentPatchCommit` (below). The rules name these and add
  a wider glob as fallback.

The detector still handles every refused form (GET, batches, `application/graphql`, form bodies),
because servers change and other GraphQL servers accept them.

Method overrides, probed by sending POST to a GET-only public path with an override to GET:

- Heroku honours `X-HTTP-Method-Override` and a `_method` form field, on POST only (PUT with the
  header did not switch; GET with `DELETE` did not switch). This is Rack's
  `Rack::MethodOverride` behaviour. `X-HTTP-Method`, `X-Method-Override`, JSON `_method` and
  `?_method=` did not switch.
- Cloudflare and Stripe did not honour any of the three headers or `_method`.
- Vercel, Fly, Supabase, Neon: answer 400/401 before routing, so the probe cannot tell. Unverified.
- GitHub: not probed (this container's proxy only lets repository-scoped GitHub paths through).
  Unverified.

The detector treats every override as possibly honoured, from any method, on every host. The
cost is a false hold when a client sends an override it did not mean, which no client in the
captures did.

Path handling, same method: Cloudflare accepts `//`, a trailing slash, `%69` for `i` and `;x`
and is case-sensitive. Stripe accepts `//` and `%62` and refuses a trailing slash, `;x` and
upper case. Heroku accepts a trailing slash and `%73`, refuses `//`, `;` and upper case.

The detector matches every reading a server might take: raw, decoded once, decoded until stable,
then with `;params`, empty and dot segments removed, backslashes as slashes, lower-cased. A match
on any reading counts.

## The rule format

`rules.json` is a list of APIs. An API names its hosts (globs), what an unknown request does there
(`unknown: hold | allow`), its GraphQL endpoint if any, and its rules. A rule has an `id`, one
`match`, an `outcome` (gone, recoverable, outage, money, or null when the four words do not fit),
an `action` (`hold` by default, `allow` to exempt a look-alike, `unknown` to say the request cannot
be read), an optional `when` on facts, `fallback: true` for a catch-all that only fires when no
other rule of the API matched, and `say`, the sentence. `{name}` in `say` is a path variable, a
GraphQL argument (flattened: `input.name`), `body.<field>`, `query.<key>`, `count.<list>`, or
`a|b` for the first one present. A missing value renders as "(not in the request)".

Match kinds:

- `rest`: `method` (one or a list), `path` (one or a list of templates: `{x}` one segment, `{x+}` the
  rest, `v*` a glob segment, `?key` a required query key), optional `query`, `body`
  (`{"field": "items.*.operation", "in": ["delete"]}`, field globs allowed), `no_filter`
  (query keys that are not filters, for PostgREST).
- `graphql`: `type` (mutation by default), `field`, `field_glob`, `field_in`, or `field_from` (a JSON
  file of names), and `arg` (argument values, globs over flattened keys).
- `sql`: `path`, `shape` (`supabase`, `neon`, `d1`, `hrana`), `dialect`.
- `mcp`: `paths`, `tool` globs, `sql_arg` (classify that argument as SQL).
- `git`: receive-pack `path`, `ref_change` (`delete`).
- AWS has its own block: the catalogue file, hand operations used when the catalogue is missing,
  and `params` rules for operations whose names look harmless (`ModifyDBInstance` with
  `BackupRetentionPeriod=0`). A params rule replaces the catalogue entry for its operation.

Example, the rule for the PocketOS call and the one for the CLI's other path to the same deletion:

```json
{"id": "railway",
 "name": "Railway",
 "hosts": ["backboard.railway.com", "backboard.railway.app"],
 "unknown": "hold",
 "graphql": {"paths": ["/graphql/v2", "/graphql/internal"],
             "known_mutations": "schemas/railway_mutations.json", "persisted": {}},
 "rules": [
  {"id": "railway.volumeDelete",
   "match": {"graphql": {"type": "mutation", "field": "volumeDelete"}},
   "outcome": "gone",
   "say": "deletes the Railway volume {volumeId}, with the backups stored on it"},
  {"id": "railway.environmentPatchCommit-delete",
   "match": {"graphql": {"type": "mutation", "field": "environmentPatchCommit",
                         "arg": {"patch.*.isDeleted": [true]}}},
   "outcome": "gone",
   "say": "applies a patch to the Railway environment {environmentId} that deletes {matched}"}
 ]}
```

On the second rule the sentence reads "applies a patch to the Railway environment e1 that
deletes volumes.vol_9".

## Choices made, for the coordinator's list

1. Railway `environmentPatchCommit` is held when any `patch.*.isDeleted` is true, also when the
   patch is sent as a JSON string or the field is aliased. `environmentStageChanges` with
   `isDeleted: true` is held at staging. `environmentPatchCommitStaged` and
   `environmentApplyChangeSet` are always held: the detector keeps no state between requests, so it
   cannot know what was staged. Remembering staged patches per environment would let the commit
   pass when the staging was seen and harmless; I did not build that.
2. GitHub GraphQL `updateRefs` with `afterOid` all zeros is held as a delete, with `force: true` as a
   force-move; REST `PATCH .../git/refs/{ref}` with `force: true` too; receive-pack with a new
   commit of zeros is held.
3. Heroku `DELETE /apps/{app}/dynos[/{dyno}]` (restart) is an `allow` rule, so the any-DELETE
   fallback does not fire. Vercel Global Config: `DELETE /v1/global-config/{id}` and `PATCH .../items`
   with an item whose `operation` is `delete`; an upsert passes.
4. wrangler's `DELETE .../workers/services/{name}` and KV `DELETE .../bulk` (and the documented
   `POST .../bulk/delete`) are rules.
5. Gmail `messages/batchModify` adding `TRASH` (recoverable 30 days) and `messages/batchDelete`
   (gone) are held, with the count of messages in the sentence. The batch endpoint is unknown.
6. MCP: the detector decides on the JSON-RPC body. When `Mcp-Method` or `Mcp-Name` headers
   (protocol 2026-07-28) disagree with the body, it holds and says so, even if the body is harmless.
7. Neon SQL over HTTP: the driver posts to `https://api.<region>.aws.neon.tech/sql` (the endpoint
   host with its first label replaced by `api.`), with `{query, params}` or `{queries: [...]}` and the
   connection string, password included, in the `Neon-Connection-String` header (read in
   `@neondatabase/serverless` 1.2.1, `index.mjs`, 2026-11-24). The sentence names host and database
   and never the password. The header means Silo could swap the database password there like any
   other secret. Supabase `POST /v1/projects/{ref}/database/query` takes `{query, read_only}`
   (https://api.supabase.com/api/v1-json, read 2026-11-24); a DROP with `read_only: true` is still
   held, since I could not verify what Supabase does with it.
8. `aws_destructive.json` is loaded (3008 operations). Several services share an endpoint prefix or
   signing name (`rds` for RDS, DocumentDB and Neptune); the matcher keeps the service the host
   names, so one call gives one sentence.

## Findings

1. **The detector only works where Silo reads the request, and in the PocketOS case Silo would not
   have.** The agent used a real Railway token it found in a file. Silo ends TLS only on hosts
   where it swaps a secret, so it would have seen an opaque connection. Silo should end TLS on every
   host in the catalogues (`details/*.md`) that a rule allows, whether or not it holds a secret for
   it, or deny those hosts outright. Once it reads them, a key Silo does not hold is itself a
   fact: the prototype reads only the prefix of such a key (`sk_live_`) and records
   `key_not_held_by_silo`.
2. **A list of paths per API is not enough; the schema is the unit.** Railway has a second
   endpoint (`/graphql/internal`), a second host (`.app`), deletions without "Delete" in the name
   (`environmentPatchCommit`), and deletions in two requests (stage, then commit). Comparing each
   mutation field against the public schema, and calling anything else unknown, covered all of
   these. The same holds for REST: the Vercel CLI sends `DELETE /projects/{id}` with no version,
   which no OpenAPI path describes; only the "any DELETE on this host" fallback caught it.
3. **The sentence needs names the request does not carry.** `volumeDelete` carries only a volume id.
   "deletes the volume pg-data of the project pocketos-prod" needs one read call to Railway with
   the swapped token before asking. That read is a fact Silo fetched, not an opinion, and it
   should be part of the ask. Which read query returns a volume's name and project: unverified.
4. **Some deletions cannot be seen at the boundary at all.** A Railway service that runs the delete
   from inside Railway, an S3 lifecycle rule that expires everything tomorrow, a CloudFormation
   change set, a Postgres function created last week. The detector holds the creating step where
   it can (lifecycle, change sets, `CREATE FUNCTION`), but a token that cannot delete is the only
   full answer.
5. **The SQL classifier should be Postgres's own parser.** sqlglot is not the server's grammar: it
   falls back to raw text for `DO`, `CALL` and `EXPLAIN`, and I had to special-case `EXPLAIN ANALYZE`,
   which runs its statement. The Postgres wire proxy thread needs the same classifier. Both should
   share one, built on libpg_query (Postgres's parser as a C library, which a Zig engine can link),
   with a SQLite path for Turso and D1. Until then, unparseable means unknown means held.

## Decisions I would challenge

- **The record keeps only sizes for non-model traffic.** For a held request, the record should keep
  what the ask showed: the sentence and the facts (GraphQL root fields, SQL statement kinds, AWS
  operation). Without them, the user cannot later see what they said yes to. This agrees with the
  recommendation under review (method, path, GraphQL operation) and adds the sentence.
- **The four outcome words miss exposure.** Making a private repository public (`PATCH /repos/{o}/{r}`
  with `visibility: public`) loses nothing, stops nothing and moves no money, yet is often the
  worst call of the day. I left `outcome: null` on it. A fifth word, such as "exposed", would fit.

## Not verified

- Whether Vercel, Fly, Supabase, Neon or GitHub honour any method override header.
- Whether `vercel.com/api/v9/...` reaches the same API as `api.vercel.com`, and whether
  `/v1/edge-config/...` still answers after the rename to Global Config.
- Fly GraphQL argument names for `deleteApp` (`input.appId` is from memory). The mutation names are
  from introspection of `https://api.fly.io/graphql` on 2026-11-24.
- Fly Machines REST paths are from the fly-go client source
  (https://github.com/superfly/fly-go/tree/main/flaps, read 2026-11-24), not from a published spec.
- Supabase MCP tool names are from the server's source
  (https://github.com/supabase-community/supabase-mcp, `packages/mcp-server-supabase/src/tools/`,
  read 2026-11-24); argument names `query`, `project_id`, `branch_id` are from the same files but I
  did not read every schema.
- Turso pipeline shapes are from the Hrana 3 spec
  (https://github.com/tursodatabase/libsql/blob/main/docs/HRANA_3_SPEC.md, read 2026-11-24) and
  https://docs.turso.tech/sdk/http/reference. D1 query body from
  https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/query/.
- 68 REST templates were found in the providers' OpenAPI files (Vercel https://openapi.vercel.sh/,
  Neon https://neon.com/api_spec/release/v2.json, Cloudflare
  https://github.com/cloudflare/api-schemas, GitHub https://github.com/github/rest-api-description,
  Stripe https://github.com/stripe/openapi, Supabase https://api.supabase.com/api/v1-json, Heroku
  https://api.heroku.com/schema; all fetched 2026-11-24). Four are wire forms seen in CLI captures or
  old names, not in the specs: Vercel `DELETE /projects/{id}`, `/v1/edge-config/{id}` and its
  `items`, and Cloudflare `DELETE .../workers/services/{name}`.
- The detector was measured on Python; the Zig engine's speed and its GraphQL and SQL parsers are
  still to build.
