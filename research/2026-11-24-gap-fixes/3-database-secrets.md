# Database passwords: the boundary speaks the database's protocol (G6)

Gap G6 of PR #350, for polarzero. Written 24 November 2026. The code is in
[`prototypes/database-proxy/`](prototypes/database-proxy/), and every number below comes from
running it in this container against a real Postgres 16.

## Summary

- **A database password can be swapped in, and `secret-placeholder` should say so.** The
  boundary ends the Postgres connection for the hosts a rule names. It checks the computer's
  placeholder in the login exchange, then logs in to the real server itself with the real
  password. I built it and ran it with psql, node-postgres, psycopg 3, Prisma 6 and pgbench.
  The login was SCRAM-SHA-256 with channel binding on both sides. The real password never
  entered the client's process. How sure: high for Postgres, which I measured. Medium for the
  providers below, which I read but did not connect to.
- **The guarantee against a hostile agent comes from Postgres, through a read credential: the
  boundary logs in as a role that cannot write.** A statement check at the boundary cannot be
  that guarantee. The prototype let `DELETE FROM orders WHERE total > $1 OR true` through and
  emptied the table, because the statement has a WHERE. A DO block also got past a read-only
  session: it called `set_config` inside its body, and then the owner's SECURITY DEFINER
  function deleted 10 rows. Recommendation: a database secret has a read credential and,
  optionally, a write credential, and the read one is used unless you say otherwise. How sure:
  high. Each step was measured.
- **Holding statements at the boundary is a rule, and it catches the evidence cases.** Those
  cases are tools that reset a database by design. Prisma 6.19's reset sends
  `DROP SCHEMA "public" CASCADE`, which I read in its schema engine. `drizzle-kit push --force`
  sends `DROP TABLE` and `alembic downgrade base` sends `DROP TABLE`, according to the issues.
  The Postgres parser classifies a statement's text exactly, so "hold every DROP, TRUNCATE,
  DELETE without WHERE, DO block" is a rule triggered by a fact. It is not a precaution. A held
  statement never reaches the server. The client gets an ordinary Postgres error with Silo's
  words. The server's transaction is left in exactly the state a failed statement leaves,
  which I measured with psql, node-postgres and a psycopg pipeline. How sure: high for the
  mechanism. The setting it ships with is yours to choose.
- **For migrations, the better answer is a database branch, not a hold.** Neon and Supabase
  make a copy-on-write branch of a database in about a second. A fork of a computer could get a
  branch, with a URL whose placeholder the boundary maps to that branch. Recommendation: a new
  feature entry. How sure: medium. It depends on each provider's API.
- **Cost in the Python prototype:** about 0.4 to 0.8 ms per query, against 0.04 to 0.09 ms
  direct to a local server. About 7 ms per new connection. A single process saturates at about
  3,000 queries a second. Most of the cost is relaying the messages; parsing adds little. This
  says nothing yet about a Zig or Rust boundary.
- **The PocketOS case is not a database password.** It was a Railway API token used in a
  GraphQL mutation (`volumeDelete`). The fix there is the HTTP secret limits (G2), extended so
  that "only read" on a GraphQL host means "the operation is a query". I propose that sentence
  for `secret-limits`.

## The case

Four production databases were wiped by commands that reset a database by design. In each case
the agent ran the command with a production `DATABASE_URL` from `.env`:

- `drizzle-kit push --force` against Railway Postgres dropped 60+ tables
  ([#27063](https://github.com/anthropics/claude-code/issues/27063));
- `alembic downgrade base && upgrade head`, run "as a verification step", dropped 21 tables
  twice in one day ([#26913](https://github.com/anthropics/claude-code/issues/26913));
- `prisma db push --force-reset` was run on production
  ([#36183](https://github.com/anthropics/claude-code/issues/36183));
- `prisma migrate diff --shadow-database-url` was given the production URL. Prisma resets its
  shadow database, so it reset production. The user's `ask` rules did not fire in auto mode
  ([#80868](https://github.com/anthropics/claude-code/issues/80868)).

Other reports follow the same pattern: a `sequelize.sync()` test run against production
([#9966](https://github.com/anthropics/claude-code/issues/9966)), and a request for a way to
"allow `psql ... SELECT` but deny DROP" ([#6938](https://github.com/anthropics/claude-code/issues/6938)).
The evidence file sums it up: "It almost always involves a prod credential sitting in `.env` or
the shell" (`research/2026-11-24-real-pains/evidence/1-destructive.md` on the PR #350 branch).

What the plan says today, in `secret-placeholder`: "A value used in another way (a key that
signs requests, an SSH key, a database password) cannot be swapped in; how those are handled is
to find out."

The case this design must handle: a computer's `.env` holds
`DATABASE_URL=postgresql://app:<placeholder>@db.example.com:5432/shop`. The agent runs
`npx prisma db push --force-reset`. The production data must survive, and the agent must read
why it did not run.

## What the prototype showed

Run: `PY=<python with pglast and cryptography> bash research/2026-11-24-gap-fixes/prototypes/database-proxy/demo.sh`
(as root: it runs Postgres as the user `postgres`). It creates two test certificate authorities
and a real Postgres 16, with TLS, `scram-sha-256` and real passwords. It starts
`pgboundary.py` in front of the server and runs the cases. The whole run printed what is
quoted below, and it removes everything when it ends.

| Case | What was run | What came back |
|---|---|---|
| Login | psql with `channel_binding=require`, `sslmode=verify-full` against a "Silo CA" certificate, the placeholder as password | `app | 5000`. The record: `"client_tls": true, "sni": "db.prod.example", "upstream_auth": "SCRAM-SHA-256-PLUS"` |
| Wrong password | the same with `guess` | refused by the boundary: `password authentication failed` |
| Placeholder sent to the real server directly | `psql host=127.0.0.1 port=<real>` | `FATAL: password authentication failed for user "app"` |
| `DROP TABLE orders`, nobody answers | psql | after 4.1 s: `ERROR: Silo held this statement (DROP TABLE) on shop (prod, writes) and nobody answered within 4 s. It did not run.` with `DETAIL: hold 234bae2c179a` |
| `BEGIN; DELETE FROM orders; SELECT 1; ROLLBACK;`, denied | psql | `Silo denied this statement (DELETE without WHERE)`, then the server's own `current transaction is aborted`, then `ROLLBACK`, and 5000 rows are still there |
| `TRUNCATE scratch \bind \g` (Parse/Bind/Execute), allowed by a file | psql | `TRUNCATE TABLE` |
| `DO $$ BEGIN EXECUTE format('DROP TABLE %I', 'orders'); END $$` | psql | held: `a DO block, whose body the boundary does not read` |
| Ctrl-C during a hold | psql sends its cancel request on a new connection | `Cancel request sent`, then `Silo held this statement (TRUNCATE) ...; the client cancelled it.` |
| The client is killed during a hold | `timeout 2 psql -c "TRUNCATE scratch"` | the record: `"outcome": "closed", "waited_s": 1.97` |
| Extended protocol, named statement | node-postgres 8.23 `{text: "truncate orders", name: "t1"}` | `code=42501 message=Silo held this statement (TRUNCATE) ...` after 6009 ms |
| A WHERE clause that limits nothing | node-postgres, `delete from orders where total > $1 or true` | let through, classified as `write`: 0 rows left inside the transaction. It was rolled back in the demo |
| A held statement in a pipeline | psycopg 3.3.9 pipeline: INSERT, `drop table orders`, INSERT, all sent before any reply | `sqlstate=42501 Silo denied this statement (DROP TABLE)`, transaction `INERROR`, 0 rows inserted, as a real error would leave it |
| Startup options from the URL | `options=-c default_transaction_read_only=off` on the read route | dropped and recorded. The session still says `on` |
| `sslmode=disable` inside the computer | psql to the boundary without TLS | works. The server leg still uses TLS (`pg_stat_ssl.ssl = true`) |
| Redis | `redis_boundary.py`, `redis-cli -u redis://:<placeholder>@...` | `set`/`get` work. `FLUSHALL` gets `ERR Silo denied FLUSHALL on this Redis: a rule refuses it`, and so does `EVAL`. The placeholder sent straight to Redis gets `WRONGPASS` |

Latency, measured with `pgbench -S` (one SELECT by primary key) against a local server, TLS on
every leg. Three runs. The second also measured a mode that relays without parsing:

| Mode | Direct | Relay only | Through the boundary (parse + rules) |
|---|---|---|---|
| simple, 1 client | 0.090 / 0.062 / 0.069 ms | 0.403 / 0.459 ms | 0.718 / 0.658 / 0.863 ms |
| prepared, 1 client | 0.080 / 0.070 / 0.041 ms | 0.584 / 0.460 ms | 0.468 / 0.463 / 0.568 ms |
| simple, 8 clients | 38,239 tps | | 2,617 tps |
| prepared, 8 clients | 48,236 tps | | 3,198 tps |
| new connection (`pgbench -C`) | 17.3 ms | | 24.7 ms |

How to read it:

- Most of the cost is the Python relay, which handles each message twice over two TLS legs.
- The parse adds about 0.2 ms to simple queries, whose text differs each time because the value
  is written into it. pglast takes about 82 µs per statement, mostly building Python objects.
  The parse adds nothing to prepared statements, which are parsed once.
- The single asyncio process caps the throughput.
- The relay-only prepared run (0.584 ms) is slower than the run with parsing (0.468 ms). That is
  noise between runs: the second run gives 0.460 and 0.463.
- Against a database across the internet, where the round trip itself takes milliseconds, the
  added time is a small share. I did not measure that here.
- The extra 7 ms per connection is the second TLS handshake plus the 4,096 PBKDF2 rounds of the
  server-side SCRAM login. The boundary can compute those rounds once per credential.

## Mechanism

### Where it runs

In the helper of the computer (`which-process-holds-the-boundary`), in its network stack
(`network-stack`). It runs for the host and port a database secret names, and only for those,
in the way `ending-the-encrypted-connection` and `registry-traffic-read` already end
connections for model and registry hosts.

The boundary knows the connection is Postgres from its first eight bytes. They are an
SSLRequest (`80877103`), a GSSENCRequest (`80877104`, answered `N`), a CancelRequest
(`80877102`), or a startup message for protocol 3.0 (`196608`). Postgres 17 clients with
`sslnegotiation=direct` send a TLS ClientHello with ALPN `postgresql` right away. The
prototype does not handle that, and the boundary must.

The host comes from the name in the TLS handshake (SNI). libpq has sent it since Postgres 14
(`sslsni=1`), and node-postgres sends it when the host is a name. A client without TLS sends no
name. Then the boundary knows the host only from the name lookup it answered. The prototype
picks the route by SNI, then by user and database name.

### Client leg: the computer to the boundary

The boundary presents a certificate for the database's host name, signed by Silo's authority.
The computer trusts that authority (`ending-the-encrypted-connection`). Which clients check it,
measured:

- **psql and libpq** (psycopg uses the libpq it bundles): `verify-full` with `sslrootcert`
  pointing at the authority works. libpq 16 also takes `sslrootcert=system`, which uses
  OpenSSL's default store, where Silo puts its authority. The default `sslmode=prefer` checks
  nothing.
- **node-postgres**: with `ssl: {ca}`, or with Node's store plus `NODE_EXTRA_CA_CERTS`, it
  works. With neither, it fails: `unable to verify the first certificate`.
- **Prisma 6.19**: `sslmode=require` alone checks nothing. It connected with the authority
  missing, and also with the wrong one given in `sslcert`. With `sslaccept=strict` it uses
  OpenSSL's store and follows `SSL_CERT_FILE`. With the Silo authority missing it fails with
  `P1011 ... certificate verify failed`.

A client that checks nothing still reaches the boundary, because the boundary is on the path.
The certificate only decides whether a client that does check agrees to connect.

The boundary then asks for SCRAM-SHA-256, and for SCRAM-SHA-256-PLUS when the leg uses TLS. It
checks the proof against the placeholder: it computes StoredKey and ServerKey from the
placeholder once per route, as Postgres does from a password. md5 and cleartext would work as
well, since the boundary knows the placeholder. The boundary also refuses `y` in the
channel-binding flag on a TLS leg, the downgrade case in RFC 5802. psql's
`channel_binding=require` passed, because the boundary hashes its own certificate for
`tls-server-end-point`.

The placeholder buys no secrecy: every program inside the computer can read it. Checking it is
still worth doing for two reasons:

- It picks the secret. Two databases on one host, or a read and a write credential, have two
  placeholders.
- It keeps clients working. libpq 16's `require_auth=scram-sha-256` and `channel_binding=require`
  refuse a server that skips authentication.

### Server leg: the boundary to the real server

The boundary opens its own TLS connection and checks the server's certificate against the
server's authority, under the real host name. It never sends a password without TLS. If the
server refuses TLS, the connection fails with `upstream refused TLS`. It logs in with the real
credential. SCRAM-SHA-256-PLUS worked against Postgres 16, because the boundary holds that TLS
session and can bind to it. The login also checks the server's signature, so a server that
does not know the password is refused.

What cannot work is channel binding from the client all the way to the real server: the client
is bound to the boundary's certificate. That is what channel binding is for, so it is expected.
Once the client trusts Silo's authority, the boundary is the server, as far as the client can
tell.

The prototype reads the real password from a file at each login and drops it from memory
afterwards. That keeps `secret-value-never-inside` and `secret-never-written-down`: the record
holds the statement texts, never a password.

### Relay

The boundary passes the server's ParameterStatus and BackendKeyData to the client unchanged,
and keeps the cancel key. A later CancelRequest with that key ends a hold, or goes on to the
server. Some startup parameters pass: `application_name`, `client_encoding`, `DateStyle`,
`TimeZone`, `extra_float_digits`, `IntervalStyle`, `search_path`. `options` is dropped and
recorded. `replication` is refused, because a replication connection would read every row
through a different protocol. A route can add `options=-c default_transaction_read_only=on`.

### Sorting statements

`classify.py` parses each Simple Query (`Q`) and each Parse (`P`) with libpg_query, through
pglast. libpg_query is Postgres's own parser, taken out of its source code. Each statement gets
one kind:

| Kind | Examples |
|---|---|
| read | SELECT, SHOW, EXPLAIN without ANALYZE, LISTEN, calls to builtins that do not write |
| write | INSERT, UPDATE or DELETE with a WHERE, CREATE, most ALTER, NOTIFY, GRANT |
| destructive | DROP (any object, CASCADE noted), TRUNCATE, DELETE or UPDATE without WHERE, ALTER TABLE ... DROP COLUMN/CONSTRAINT, DROP DATABASE/OWNED/ROLE, a data-modifying CTE (`WITH d AS (DELETE ...)`), EXPLAIN ANALYZE of any of these |
| opaque | DO, CALL, a call to a function defined in the database, EXECUTE of a prepared statement not seen, the fast-path call message (`F`, by OID), dblink |
| admin | volatile builtins that act on the server: `set_config`, `pg_read_file`, `lo_*`, `pg_terminate_backend`, `pg_reload_conf`. Also COPY ... PROGRAM and COPY to or from a server file |
| session | SET of `default_transaction_read_only`, `role`, `session_authorization`, `search_path` and a few others; `BEGIN READ WRITE`; `SET SESSION CHARACTERISTICS ... READ WRITE`; RESET ALL; DISCARD |
| unparsed | text the parser refuses |

A string of several statements takes the worst kind among them. The function lists come from
the server's `pg_proc` when the route first connects. PG16 has 259 volatile builtins, and a
reviewed list of about 50 of them (`random`, `nextval`, `pg_sleep`, advisory locks, sizes) count
as reads. A name defined outside `pg_catalog` is opaque even when it shadows a builtin, so
`public.lower(int)` does not pass as `lower`. A `CREATE FUNCTION` seen through the boundary is
added to that set at once. `query_to_xml('delete from orders ...')` looks like a way to hide a
statement in a string. It is not, measured: Postgres runs the string read-only and answers
`DELETE is not allowed in a non-volatile function` (also `DROP TABLE is not allowed ...`), so it
stays a read.

**What a parse knows.** It knows exactly what was written. It does not know what running it
will do. Three measured cases:

- `delete from orders where total > $1 or true` has a WHERE, so it is `write`, and it emptied
  the table.
- A view can call a function that deletes. `SELECT count(*) FROM v_recent`, where the view calls
  `purge_count()`, is a SELECT, and it ran a DELETE. As `app_ro` it failed only because that
  role may not delete.
- A trigger, a rule (`CREATE RULE`), a row-level policy or a default expression can each run a
  function on a plain INSERT.

So "this statement is a DROP" is a fact. "This statement destroys nothing" is not something a
parse can know. That is why the read credential, enforced by Postgres, is the guarantee, and
the statement rules are a second layer. They catch the evidence cases, which are tools doing
what they were built to do, and every hostile attempt that writes the destructive word.

**Is a DO block an opinion?** No, and this changes the brief's framing. "This statement is a DO
block whose body the boundary does not read" is a fact from the parse. A rule "hold every DO
block" is triggered by that fact and is exact about what it holds. An opinion appears only when
a classifier reads the body and says "this looks harmless" or "this looks destructive". By
`precaution-grants-nothing`, that opinion may add a hold but can never release what the rule
holds. Only you can release it.

### Holding a statement on a database connection

Where the hold happens:

- For a Simple Query, at the `Q` message.
- For the extended protocol, at the Execute (`E`) of a portal whose statement is held. The
  Parse and Bind go to the server, since parsing a DROP drops nothing.

While held, the statement waits and only that connection waits (`held-requests-and-asks`).
Other connections from the same computer go on.

When the hold ends:

- **Allowed:** the message goes to the server.
- **Denied, or ended without an answer:** the statement never reaches the server. In its place
  the boundary sends one token that cannot parse, `silo_held_<random>`: as a Query for a
  Simple Query, or as a Parse for the extended protocol, and then drops the client's messages
  up to the next Sync. The server answers with its own syntax error. It is in the state a
  failed statement leaves: an aborted transaction inside `BEGIN`, and nothing open outside one.
  The boundary swaps that one error, recognised by the token, for its own words with SQLSTATE
  `42501` (insufficient_privilege). If an earlier statement in the same pipeline already
  failed, the server skips the token, and the client sees its own real error unchanged.
- **The client cancels** (psql's Ctrl-C, or a driver's cancel): it is refused the same way with
  `57014`, the code of a cancelled query.
- **The connection closes:** the hold ends with nothing sent, and the record says
  `closed while held` (`program-gives-up-first`).

The words follow `reason-in-a-refusal`. A rule's refusal names what it refused, for example
`DROP TABLE`, `DELETE without WHERE` or `a DO block`, because a rule holds even when the agent
knows it. The HINT tells the agent what to do: "Ask the user to allow it in Silo, or change the
statement." In my runs, psql, node-postgres and psycopg each reported the refusal once and did not
resend the statement. I did not check other drivers or ORMs.

Client timeouts, measured or read:

- psql has none.
- node-postgres has `query_timeout`, which is client-side and off by default.
- `statement_timeout` is enforced by the server, which never sees the held statement.
- Claude Code ends a command after two minutes by default, which is why `hold-limit-length` is
  90 seconds. That close shows up as `closed`.

One cost is not measured: a hold inside a transaction keeps that transaction's locks for the
length of the hold. Picture an agent that runs `BEGIN; UPDATE orders SET ... WHERE id = 7;` and
then sends a DROP that is held for 90 s. Row 7 stays locked on the production database for
those 90 s. I measured the same effect from a read role: an idle `BEGIN; SELECT ... FROM orders`
made the owner's `ALTER TABLE orders ADD COLUMN` fail with `canceling statement due to lock
timeout`. The ask should show it, and the boundary can read it from `pg_locks` on a side
connection. Marked to find out.

### Three settings for a database secret

A database secret holds one or two credentials. You choose one of three settings. Each action
is filed under the plan's kinds:

| Setting | The boundary logs in with | Statement rules (rule, triggered by the parse, a fact) | Who can change it |
|---|---|---|---|
| **read** (proposed default for anything you mark as production) | the read credential, plus `default_transaction_read_only=on` | deny `session`, `admin`, `opaque` | you |
| **write, with holds** | the write credential | hold `destructive`, `opaque`, `admin`, `session`, `unparsed`; you can add `write` | you |
| **write** | the write credential | none. Each statement is still recorded | you |

A write is never granted by an opinion (`grant-needs-a-fact-or-you`). You give the write
credential, and you answer each hold. A precaution, for example a classifier reading a DO
body, can only add a hold or close the connection (`kinds-of-action`). Taking the secret back
closes every connection of that route and never waits (`actions-that-never-wait`).

### A read role and a read-only session, compared

Measured against Postgres 16. The first column is the owner role with
`default_transaction_read_only=on`. The second is a role with only SELECT.

| Attempt | Read-only session, owner role | Read role (SELECT only) |
|---|---|---|
| `DELETE FROM orders WHERE id = 1` | `cannot execute DELETE in a read-only transaction` | `permission denied for table orders` |
| `SELECT purge_old()` (a plpgsql function that deletes) | refused (read-only) | refused (permission) |
| `BEGIN READ WRITE; DELETE ...` | **ran** | refused (permission) |
| `SELECT set_config('default_transaction_read_only','off',false)`, then DELETE | **ran** | refused (permission) |
| The owner's SECURITY DEFINER function that deletes (EXECUTE is granted to PUBLIC by default) | refused (read-only) | **ran: 10 rows deleted** |
| Both together, with `set_config` (or a DO block calling it) first, then the definer function | | **ran: 10 rows deleted** |
| The same through the boundary's read route (deny `session`, `admin`, `opaque`) | | denied: `calls set_config()`, `a DO block`, `calls archive_orders()` |
| `NOTIFY jobs, 'x'` | **ran** | **ran** |
| `CREATE TEMP TABLE`, `lo_create(0)` | refused (read-only) | **ran** |
| `LOCK TABLE ... ACCESS EXCLUSIVE`, `SELECT ... FOR UPDATE` | | refused (permission) |
| Idle in a transaction after one SELECT | | **holds a lock that makes the owner's ALTER TABLE wait** |
| `COPY ... TO PROGRAM`, `pg_read_file` | need superuser or `pg_execute_server_program` / `pg_read_server_files` | same |

The answer to "which is the rule":

- **The read role is the rule.** Postgres enforces it inside every function, view and DO block.
  The client cannot unset it, because the boundary holds the only credential.
- **The read-only session is a second layer.** It closes the SECURITY DEFINER hole, but only
  while the boundary refuses every way to switch it off.
- **The statement rules on the read route close those ways:** `SET`, `BEGIN READ WRITE`,
  `set_config`, and DO or function bodies that could call it. On their own, the session and the
  statement rules would lose to the first DO block. The run above shows it: with `opaque`
  allowed, `DO $$ BEGIN PERFORM set_config(...) END $$` followed by `SELECT archive_orders()`
  deleted 10 rows through the boundary.

`read_role_check.sql` lists what a read credential can still change. Run it as the read role:
SECURITY DEFINER functions it may call, tables it may write, schemas it may create in, TEMP,
and role attributes such as BYPASSRLS. Against the test database it printed
`security definer | archive_orders() | app` and `temp objects | shop`. When you set up a read
credential, Silo can run it and show you each line. What can never come from a read role is
protection for your data's privacy: the agent reads every row it may SELECT, and sends it to
the model provider. That is `what-was-sent-to-a-host`, and the record holds the statements.

Where you have no second role, the settings still work with one credential, in "write, with
holds". On Neon and Supabase anyone can create a role. On Railway's Postgres template, the
owner connects as `postgres`. The ask that sets up the secret should offer to create the read
role with one statement shown to you, for example `CREATE ROLE silo_read LOGIN PASSWORD ...;
GRANT pg_read_all_data TO silo_read;` (`pg_read_all_data` exists since Postgres 14).

### The better answer for migrations: a branch

The four evidence cases were migrations or schema pushes. Holding them stops the loss, but the
agent still cannot do its task. Neon branches a database copy-on-write in about a second,
whatever its size ([Neon, branching for agents](https://neon.com/branching/branching-for-agents)).
Its guide gives each git worktree its own branch
([Neon guide](https://neon.com/guides/git-worktrees-neon-branching)). Supabase has branches too.

This matches what Silo already does with a disk. A fork of a computer could get a database
branch: the boundary creates it through the provider's API, with a credential that only you
hold, and maps the fork's placeholder to the branch's host and password. The agent in the fork
runs `prisma db push --force-reset` freely, and production is untouched. Dropping the fork
deletes the branch.

This also fills a gap in `fork-identity`. It says "the database an agent needs starts as it
does in the parent", which is true for a database inside the computer and says nothing about a
remote one. `what-a-fork-is-given` already gives a fork no secret. A branch is a way to give it
a database without giving it production.

A local database inside the computer needs none of this: the disk's checkpoint covers it, and
`what-a-restore-leaves` brings it back.

## Providers

What each one needs. I read the driver sources named, and I did not connect to these services.

| Provider | How the client connects | What the boundary needs |
|---|---|---|
| **Neon**, Postgres protocol | `ep-<id>.<region>.aws.neon.tech:5432`, TLS. Neon's proxy routes by the endpoint ID in SNI. Without SNI, the client passes `options=endpoint%3D<id>` or puts `endpoint=<id>;<password>` in the password ([Neon, connection errors](https://neon.com/docs/connect/connection-errors)) | The boundary sends the real host as SNI on its own leg. It drops `options`, so it must add the endpoint option itself when it has no SNI. A password of the form `endpoint=...;<placeholder>` must be recognised |
| **Neon**, serverless driver over HTTP (`@neondatabase/serverless` 1.2.1) | `POST https://api.<rest of host>/sql`, with the header `Neon-Connection-String: postgresql://user:password@ep-.../db` and a JSON body holding the query | The secret sits inside a header value, inside a URL. The PR #347 rule ("a secret names where it goes in a request") must allow "the password of the URL in this header". The query is in the body, so the same statement rules apply to the JSON |
| **Neon**, serverless driver over WebSocket | `wss://<host>/v2`, the Postgres protocol inside WebSocket frames, with Postgres's own TLS turned off (`forceDisablePgSSL: true`) | The same Postgres relay, framed as WebSocket inside the TLS the boundary already ends |
| **Supabase** | Direct `db.<ref>.supabase.co:5432`. Through Supavisor, `aws-0-<region>.pooler.supabase.com`, port 6543 for transaction mode or 5432 for session mode, with user `postgres.<ref>`, which is how Supavisor finds the project ([Supabase docs](https://supabase.com/docs/guides/database/connecting-to-postgres)) | Pass the user as written. In transaction mode, named prepared statements are not supported, so the boundary must not create any of its own. The HTTP API (PostgREST with the `service_role` key) is an HTTP header secret: the Supabase MCP leak in the evidence (B3) was that key |
| **AWS RDS and Aurora, IAM** | The password is a token valid 15 minutes: a SigV4-presigned `rds-db:connect` request built from AWS keys ([AWS CLI](https://docs.aws.amazon.com/cli/latest/reference/rds/generate-db-auth-token.html)) | The boundary holds the AWS keys and makes the token itself at each login. The computer never has the keys. This answers "a key that signs requests" for one protocol, because the boundary logs in on the computer's behalf. Cloud SQL and Azure, which use an OAuth token as the password, work the same way |
| **RDS with a password, or RDS Proxy** | TLS against Amazon's own authority | The server-leg authority is Amazon's bundle |
| **Railway** Postgres | A public TCP proxy, `<x>.proxy.rlwy.net:<port>` | To find out: whether the template's certificate can be checked. If it is self-signed, the rule must pin its fingerprint, which you confirm once, and never "skip the check" |
| **Railway API** (the PocketOS case) | `POST https://backboard.railway.app/graphql/v2` with a bearer token, GraphQL `volumeDelete` | Not a database password. An HTTP secret with a limit: on a GraphQL host, "only read" means the operation in the parsed body is a `query`. A `mutation`, or a batch holding one, is not swapped in. Railway made API deletes undoable in May 2026 ([changelog](https://railway.com/changelog/2026-05-01-undoable-deletes)) |
| **PlanetScale**, MySQL protocol | MySQL with `caching_sha2_password` or `mysql_native_password` | The same design with a MySQL relay. Both logins can be checked against the placeholder. The full `caching_sha2` exchange needs TLS, which the boundary has. Statements need a MySQL parser (Vitess's or TiDB's) |
| **PlanetScale**, HTTP driver (`@planetscale/database`) | `POST https://<host>/psdb.v1alpha1.Database/Execute` and `/CreateSession`, `Authorization: Basic base64(user:password)` | A header secret, but encoded: the boundary decodes the Basic value, checks the placeholder, and re-encodes it with the real password. The query is in the JSON body |
| **Redis** (prototype) | `AUTH [user] <password>` or `HELLO 3 AUTH user <password>` | Measured: swap at AUTH, deny FLUSHALL, FLUSHDB, EVAL, CONFIG and the like. A hostile agent can still `SCAN` and `DEL` key by key. The guarantee is again a Redis ACL user with `-@write`, which Redis enforces |

## How it fits the plan

- **`secret-placeholder`**: changes. Its last two sentences become wrong for databases. The
  boundary can swap a password when it ends the database protocol. Proposed sentence below.
- **PR #347's proposal** (swap only where the secret names it goes): this design is its case
  for databases. The place a database secret goes is the login exchange of a connection to
  host:port in protocol P. The placeholder can also appear in statement text or in a row the
  agent writes. There it is forwarded as it is and recorded, as PR #347 proposes for a body.
  The boundary never puts the real password anywhere but the login.
- **`secret-limits`**: "Only read means GET and HEAD..." has no meaning for a database. The
  database version of "only read" is the read credential, enforced by the server. Proposed
  sentence below, including GraphQL.
- **`secret-values`**: kept. The real password exists only in the helper. The server never
  sends a password back in the Postgres protocol, so there is nothing to scrub from replies.
- **`ending-the-encrypted-connection`**: kept, with one addition: a database host a secret
  names is a host the boundary ends.
- **`registry-traffic-read`**: the precedent. The boundary reads one protocol for the hosts one
  rule names.
- **`record-shows-what-was-sent`**: every statement the boundary sees goes in the record with
  its kind, as a fact. The prototype writes non-read statements, and every statement on a route
  set to record reads.
- **`hold-limit`, `hold-limit-length`, `hold-sentence`, `program-gives-up-first`,
  `how-a-hold-ends`**: kept. `how-a-hold-ends` gains a row: on a database connection the boundary
  reads, a refusal is the protocol's own error (an ErrorResponse with SQLSTATE `42501`, or
  `57014` for a cancel), sent where the statement would have failed, and the server's
  transaction is left as a failed statement leaves it.
- **`reason-in-a-refusal`**: kept. The rule names what it refused.
- **`kinds-of-action`, `precaution-grants-nothing`, `grant-needs-a-fact-or-you`,
  `what-a-rule-may-do`**: kept. Holds and denies by kind are rules, triggered by the parse. A
  classifier on DO bodies is a precaution that may only add holds.
- **`rules-inside-are-no-boundary`**: a new real case. Prisma 6.15 added a check that refuses
  `migrate reset`, `db push --force-reset` and `--accept-data-loss` when it detects an agent
  ([Prisma 6.15 release](https://www.prisma.io/blog/orm-6-15-0-ai-safety-guardrails-for-destructive-commands-and-more)).
  It detects one from environment variables: `CLAUDECODE`, `GEMINI_CLI`, `CURSOR_AGENT`,
  `REPLIT_CLI`, `OR_APP_NAME=Aider`, and `CODEX_SANDBOX=seatbelt`, which is set only on macOS.
  I read this in `node_modules/prisma/build/index.js` of 6.19.3. It then lets the command run
  when `PRISMA_USER_CONSENT_FOR_DANGEROUS_AI_ACTION` is set, which is a variable the agent sets.
  When I ran `db push --force-reset` from Claude Code, Prisma answered `Prisma Migrate detected
  that it was invoked by Claude Code`. I then tried to set the variable myself to capture the
  SQL, and Claude Code's own auto-mode classifier denied that command. Both checks live inside
  agent programs and tools, so neither is a boundary.
- **`fork-identity`, `what-a-fork-is-given`, `fork-never-more-than-parent`**: the branch
  feature extends them. A fork gets a branch or the read credential, never the parent's write
  credential.
- **`what-a-restore-leaves`** and G9: a restore never brings back a remote database. The ask
  that sets up a database secret should say so.
- **`channel-list`**: no new channel. This is the network channel, read for one more protocol.

## Prior art

| Who | What they do | What differs for Silo |
|---|---|---|
| PgBouncer | Checks clients against stored SCRAM secrets. It can log in to the server by recovering the client's ClientKey from the client's proof, which works only when both sides share one password ([Crunchy Data](https://www.crunchydata.com/blog/pgbouncer-scram-authentication-postgresql)) | Silo uses two passwords on purpose, so it must hold the real one. The same post notes that whoever holds ClientKey can impersonate the user. That is why the boundary keeps the real credential and never the client's proof |
| Supavisor, pgcat, PgDog | Poolers that end the connection. Supavisor routes by user suffix. PgDog parses with pg_query to route reads and shards ([PgDog docs](https://docs.pgdog.dev/features/sharding/query-routing/)) | The same parser and relay. They pool and route; none holds statements for a person's answer |
| Neon proxy | Routes by SNI and sits in front of every Neon database | It speaks Postgres in front of the server, like Silo's boundary on the server leg |
| Cloudflare Hyperdrive | Pools near the database and caches reads. It parses each query to tell reads from writes ([Cloudflare](https://developers.cloudflare.com/hyperdrive/concepts/how-hyperdrive-works/)) | It uses a parse to cache. Silo uses one to hold |
| AWS RDS Proxy | IAM login at the proxy, the password in Secrets Manager | Credential injection at the proxy, the same idea as the placeholder |
| Teleport database access | Short-lived client certificates to Teleport. Teleport logs in to the database itself and can create a database user for each session ([Teleport](https://goteleport.com/docs/enroll-resources/database-access/auto-user-provisioning/postgres/)) | A user per session is a strong form of the read credential. Silo could create a role for each computer where the provider allows it |
| HashiCorp Boundary | Injects credentials for SSH and RDP. For databases it brokers: it hands the credential to the user ([HashiCorp](https://developer.hashicorp.com/boundary/docs/concepts/credential-management)) | Brokering would put the password in the computer. Silo injects |
| hoop.dev guardrails | A Postgres listener classifies each statement by its worst effect, so a DELETE inside a CTE counts as a delete. `CALL` and `EXECUTE` report `unknown` "because their bodies live in the catalog and no parser can say what they touch". It denies, or holds for approval through its analyzer ([hoop.dev](https://hoop.dev/docs/features/guardrails)) | The closest product. It reaches the same conclusion about opaque calls. It suggests a regex for a bare DELETE, where a parse is exact |
| StrongDM, Formal | Database access gateways with query logging and policies | Built for people with corporate identities. I did not read their docs in depth |
| Anthropic's reference Postgres MCP server | Wrapped each query in a read-only transaction. `COMMIT; DROP SCHEMA public CASCADE;` ended the transaction and ran the DROP, because node-postgres accepts several statements in one call ([Datadog Security Labs](https://securitylabs.datadoghq.com/articles/mcp-vulnerability-case-study-SQL-injection-in-the-postgresql-mcp-server/)) | The real case for "a read-only session alone is not a limit". Silo's read route uses a read role, and parses multi-statement strings |
| Postgres MCP Pro (crystaldba) | Restricted mode: read-only transactions, plus a pglast parse that refuses COMMIT and ROLLBACK, plus a time limit. It warns that unsafe procedural languages can get around it ([GitHub](https://github.com/crystaldba/postgres-mcp)) | The same layers, run inside the agent's tool. Silo runs them outside, where the agent cannot remove them |
| Prisma 6.15+ | Refuses destructive commands when it detects an agent, unless a consent variable is set | Inside the tool (see above) |
| Neon and Supabase branching | A copy-on-write branch per agent | The real answer for migrations. Silo can create the branch for a fork |

## Bad paths

| Path | What happens | Kind of outcome |
|---|---|---|
| The agent reads the real password | It never can: it is only in the helper. `env`, `.env` and `/proc` hold the placeholder | none |
| The agent connects to the real host's address directly, on 5432 | It goes through the same network stack. If the host is named by the rule, it is ended at the boundary like any other connection. If the agent finds the IP without a name lookup, the stack still maps the address it answered for that host. An address it never answered is another host: denied, or passed by your network rules. Either way, only the placeholder can be sent, and the real server refuses it (measured: `FATAL: password authentication failed`) | none for the secret. Known limit: two hosts on one IP behind one port can only be told apart by SNI |
| Another port, or another protocol (an HTTP admin API) | Not a database connection. That host's own rules apply. The Railway, Supabase HTTP and Neon HTTP cases are HTTP secrets | covered by HTTP secret limits (G2) |
| A different database on the same server (`dbname=postgres`) | The route names user and database. Another name finds no route and is refused at the boundary: `Silo has no database rule for user ...` | refused |
| `options=-c ...` in the URL | Dropped and recorded. Measured | none |
| `sslmode=disable` inside the computer | Works. The server leg always uses TLS. The leg inside the computer is a virtual link to the helper | none |
| The client refuses Silo's certificate (a driver with its own CA bundle, `sslrootcert` pointing elsewhere) | The connection fails and nothing is sent. The fix is the client's setting | fails closed. Cost: setup per driver. Silo puts the authority in the system store, `NODE_EXTRA_CA_CERTS`, `PGSSLROOTCERT` and `SSL_CERT_FILE`, as `registry-traffic-read` does for package managers |
| A destructive effect without the destructive word (`WHERE ... OR true`, a view, a trigger, a function) | Measured: passes the statement rules | the read credential is the guarantee. With a write credential, only "hold every write" covers it, and that is your choice |
| `set_config`, a DO block, or a function that switches the read-only session off | Measured: denied by the read route's rules. Allowed by mistake, the read role still refuses writes. Only SECURITY DEFINER functions remain, and `read_role_check.sql` lists them | rule, with a listed residue |
| A SECURITY DEFINER function the read role may call | Measured: it deleted 10 rows with the read role alone | listed at setup, denied as `opaque` on the read route |
| NOTIFY on a channel the application listens on (a job queue) | Allowed even in a read-only session. Measured | the read route should deny `write`, which includes NOTIFY. LISTEN is a read |
| Large objects, temporary tables | A read role can create them, which writes to disk. A read-only session refuses them. Measured | the read-only session covers it |
| Large reads, long queries, idle open transactions on production | A read role can slow production and make its migrations wait. Measured with an ALTER TABLE that hit its lock timeout | the boundary sets `statement_timeout` and `idle_in_transaction_session_timeout` at startup, and the agent cannot change them (`session`) |
| A hold inside a transaction | Its locks stay for the length of the hold | to find out: show the locks in the ask, or use a shorter limit inside a transaction |
| The fast-path function call (`F`, by OID), used by libpq's `lo_*` | `opaque`. Held or denied | rule |
| `COPY ... TO PROGRAM`, `pg_read_file` | `admin`. They also need superuser on the server. A read credential never has it | rule, plus the role |
| A pipeline, several statements in one Query | The worst kind applies to the whole Query. In a pipeline, the hold happens at its Execute. Measured with psycopg | rule |
| A prepared statement prepared through SQL `PREPARE` and run by `EXECUTE` | The prototype treats `EXECUTE name` as `opaque` unless it saw that statement prepared | rule; a cost to fix: track SQL-level PREPARE |
| The catalog changes outside Silo (the production app adds a function with a builtin's name) | Seen at the next refresh of the catalog. Until then, a call by that name counts as the builtin | known residue; with a read credential, Postgres still enforces the role |
| The parser and Postgres disagree (pglast here carries the parser of a newer Postgres than the server's 16) | A statement the server accepts but the parser refuses is `unparsed`, and held or denied | fails closed |
| A replication connection | Refused | none |
| The helper stops | The computer has no network (`network-stack`), so no database either | fails closed |
| An answer lost while the agent is gone | The record has the hold and how it ended | as for every hold |

## Recommendations

1. **Change `secret-placeholder`.** Replace "A value used in another way (a key that signs
   requests, an SSH key, a database password) cannot be swapped in; how those are handled is to
   find out" with: "A database password is swapped in by the boundary speaking the database's
   protocol for the hosts its secret names: it checks the placeholder in the login and logs in
   to the real server itself, over TLS, with the real password. A key that signs requests is
   handled the same way where the boundary logs in itself, such as an RDS IAM token. An SSH key,
   and signing for HTTP requests, are to find out."
2. **Change `secret-limits`.** Add: "For a database, only read means a credential the database
   itself refuses writes to: the boundary logs in with the read credential you gave, starts the
   session read-only, and denies statements that would change that. On a GraphQL host, only read
   means the request's operation is a query." I am sure of the first sentence. The GraphQL
   sentence covers PocketOS and should be checked against the G2 doc.
3. **New decision in `boundary`: `database-secret`.** Asks: "What does a database secret hold,
   and what may it do?" Answer: "A database secret names a host, a port, a protocol and a
   database. It holds a read credential, and may hold a write credential. You choose one of
   three settings: read, write with holds, or write. Read is offered first for a database you
   mark as production. With write and holds, a rule holds every statement whose parse is a
   DROP, a TRUNCATE, a DELETE or UPDATE without WHERE, an ALTER that drops, a DO block, a CALL,
   a call to a function defined in the database, or a change to the session's settings. A
   statement whose effect the parse cannot show is held, never allowed by an opinion. When you
   set up a read credential, Silo shows what it can still change."
4. **Add a row to `how-a-hold-ends`.** "On a database connection Silo reads, a refusal is the
   protocol's own error, sent where the statement would have failed, and the server's
   transaction is left as a failed statement leaves it."
5. **New feature in `going-back`: `database-branch-per-fork`.** Name: "A fork gets a branch of
   your database". What: "Where your database provider offers branches, a fork of a computer
   gets its own branch of the database a secret names, and its placeholder goes to the branch.
   Dropping the fork deletes the branch. A fork never gets the parent's write credential."
   Needs: `database-secret`, `what-a-fork-is-given`.
6. **Add the Prisma guard to the notes of `rules-inside-are-no-boundary`,** as its newest real
   case.
7. **Build order:** Postgres first: login swap, record of every statement, the read setting.
   Then holds. Then Neon's HTTP and WebSocket drivers. Then MySQL. Redis is a small addition.

## Challenges

- **To the brief's framing that "destructive is unknowable for a function call, so a precaution
  can only hold".** The trigger "this is a DO block" or "this calls a function defined in the
  database" is a fact from the parse. Holding on it is a rule, and it is exact about what it
  holds. Filing it as a precaution would make it weaker than it is. By `less-sure-less-power` a
  precaution could not deny, only hold, and a hold the parse earned could then be released by
  an opinion that looks at the body. Case: `DO $$ BEGIN PERFORM set_config(...) END $$` on the
  read route. A rule denies it. A classifier reading the body might call it harmless, since it
  writes no row, and it was the first step that let the 10 rows be deleted.
- **To `fork-identity`'s "the database an agent needs starts as it does in the parent".** That
  holds for a database inside the computer. For a remote database the sentence says nothing,
  and the evidence cases are all remote. Case: a fork made to try a migration runs
  `prisma migrate reset` against the parent's production URL. Proposed addition: "A database
  outside the computer is reached through the fork's own secret: a branch where the provider
  offers one, otherwise the read credential."

## Open questions

- How much the Zig boundary's relay adds per query. Measure it in the spike, with the same
  pgbench runs.
- Whether each driver ORMs use takes a `42501` refusal without retrying it: Prisma's query
  engine, SQLAlchemy, ActiveRecord, Go's pgx. I measured psql, node-postgres and psycopg only.
- Holds inside a transaction: show the locks held, or use a shorter limit?
- Railway's Postgres certificate: can it be checked, or must the rule pin it?
- Postgres 17 direct TLS (`sslnegotiation=direct`): the boundary must read a ClientHello first.
- The exact SQL of `drizzle-kit push --force`: drizzle-kit 0.31 stopped at an interactive rename
  prompt without a TTY, so I did not capture it. The Prisma SQL comes from strings in its schema
  engine 6.19.3 (`Resetting schema(s)`, `DROP SCHEMA "" CASCADE`, `CREATE SCHEMA "`). I did not
  capture it from traffic: that needs the consent variable, which is the user's to give, and
  Claude Code's classifier denied my attempt to set it.

## Files

- `prototypes/database-proxy/pgboundary.py`: the Postgres boundary (asyncio, about 600 lines).
- `prototypes/database-proxy/classify.py`: statement kinds, on Postgres's parser.
  `echo "DELETE FROM t" | python classify.py` prints one line per statement.
- `prototypes/database-proxy/demo.sh`: sets up the test server and runs the cases above.
- `prototypes/database-proxy/client_node.mjs`, `client_psycopg.py`: the driver tests.
- `prototypes/database-proxy/read_role_check.sql`: what a read credential can still change.
- `prototypes/database-proxy/redis_boundary.py`: the same pattern for Redis.
  Run: `python redis_boundary.py 56380 6379 <placeholder> <file with the real password>`.
