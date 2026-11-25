# Hosted databases and database wire protocols: destructive operations

Scope: hosted databases an agent reaches over HTTPS (Supabase, Neon, PlanetScale, Turso, MongoDB Atlas,
Upstash), the MySQL, MongoDB and Redis wire protocols, and every HTTPS endpoint that takes raw SQL.
The Postgres wire proxy and Cloudflare D1's own catalogue are covered by other threads.

All sources were read on 2026-11-24 unless a line says otherwise. "Unverified" means I did not find
it in an official source that day.

## What matters most

1. **SQL travels as HTTPS JSON in many places, and Silo already reads it.** Supabase's Management API
   has `POST /v1/projects/{ref}/database/query`, which runs any SQL with a personal access token.
   Neon's serverless driver posts `{query, params}` to `https://api.<region>.../sql`. PlanetScale's
   driver posts `{query, session}`. Turso posts `{requests:[{type:"execute",stmt:{sql}}]}`. D1,
   BigQuery and Snowflake do the same. For each of these, Silo swaps the credential, so it already
   ends TLS and sees the SQL. One SQL classifier, shared with the Postgres proxy, can serve all of
   them. Only the JSON path to the SQL text changes per host (table in the SQL-over-HTTP section).
2. **Neon's HTTP driver sends the whole connection string, password included, in a request header**
   (`Neon-Connection-String`). So Neon SQL over HTTP fits the existing secret swap on a header, with
   no wire proxy at all. PlanetScale's driver sends the password inside `Authorization: Basic
   base64(user:password)`, so a swap there must decode and re-encode the header.
3. **Supabase project deletion takes its backups with it.** Supabase says deleting a project removes
   "all associated data, including any backups stored in S3". Supabase lists daily backups only for
   Pro, Team and Enterprise and tells free projects to export their data themselves. This is the
   PocketOS pattern (backups inside the thing deleted). `DELETE /v1/projects/{ref}` should be held
   by default, and it is "gone".
4. **Some providers give a real undo, and Silo can lean on it.** Neon keeps deleted projects for
   7 days (`POST /projects/{id}/recover`) and keeps a history window (6 hours on Free, up to 30 days
   on Scale), so a `DROP TABLE` over Neon SQL is "recoverable" by instant restore. Turso keeps deleted
   databases for 5 days on paid plans and has delete protection. Atlas refuses to delete a cluster
   with termination protection on and has a `retainBackups` flag. PlanetScale blocks direct DDL on
   production branches with safe migrations on and lets a deployed schema change be reverted for
   30 minutes. An ask can say which of these applies, from facts Silo can read through the same
   provider API.
5. **The three wire protocols all let a boundary proxy read, hold and refuse one command while the
   session stays up**, once it ends TLS. MySQL starts in plaintext and upgrades with an SSLRequest,
   like Postgres. MongoDB and Redis on hosted services are TLS from the first byte. A refusal is a
   native error reply (MySQL `0xFF` ERR packet, MongoDB `{ok: 0, errmsg, code}`, Redis `-ERR` or
   `-NOPERM`), and the client keeps the connection. Each has one trap that breaks naive reading:
   MySQL executable comments `/*! ... */`, MongoDB `OP_COMPRESSED`, Redis `EVAL` scripts that call
   `FLUSHALL` from Lua.

## 1. Supabase

Supabase has four separate surfaces an agent can reach. Each has its own host and credential.

| Surface | Host | Credential |
|---|---|---|
| Management API | `api.supabase.com/v1` | personal access token (scoped or classic) or OAuth token |
| Data API (PostgREST) | `<ref>.supabase.co/rest/v1` | `apikey` header: publishable/anon or secret/service_role |
| Storage API | `<ref>.supabase.co/storage/v1` | same keys |
| Auth admin API | `<ref>.supabase.co/auth/v1/admin` | secret/service_role key |

Sources: https://supabase.com/docs/reference/api/introduction, https://supabase.com/docs/guides/api/api-keys.

### Token scoping

- Personal access tokens come in two kinds. A *scoped* token reaches only the organizations, projects
  and permissions chosen at creation. A *classic* token, shown as "Legacy", has all the account's
  rights. (https://supabase.com/docs/reference/api/introduction)
- Each endpoint names an OAuth scope and a fine-grained permission. Examples below.
- Data API keys: "A secret key bypasses every Row Level Security policy you have." The legacy
  `service_role` JWT does too. Publishable and legacy `anon` keys do not. Keys go in the `apikey`
  header. (https://supabase.com/docs/guides/api/api-keys)

### Operations

| Operation | Wire | Outcome | Permission | Undo before pass |
|---|---|---|---|---|
| Delete project | `DELETE api.supabase.com/v1/projects/{ref}` | **gone**, backups included | OAuth `projects:write`; fine-grained `project_admin_write` | No API I found makes an off-site backup. Silo could run its own `pg_dump` first, only with the DB password (unverified as practical). Hold. |
| Pause project | `POST /v1/projects/{ref}/pause` | **outage** | `projects:write`; `project_admin_write` | Not needed; "Restore a project" (`/v1-restore-a-project` in the docs) unpauses. Exact path unverified. |
| Run SQL | `POST /v1/projects/{ref}/database/query`, body `{"query": "...", "parameters": [...], "read_only": bool}` | depends on SQL: **gone** for DROP/TRUNCATE/DELETE on Free; **recoverable** to the last daily backup or PITR point on paid plans | OAuth `database:write`; fine-grained `database_read` or `database_write` | No pre-action via API. Silo can classify the SQL in `query`. |
| Read-only SQL | `POST /v1/projects/{ref}/database/query/read-only`, body `{"query", "parameters"}` | none; runs as `supabase_read_only_user` (Beta) | `database:read`; `database_read` | A rule can allow this path and hold the other one. |
| Restore PITR | `POST /v1/projects/{ref}/database/backups/restore-pitr`, body `{"recovery_time_target_unix": int}` | **gone** for every write after the target time. A restore is itself destructive. | `database:write`; `backups_write` | No. Hold. |
| Delete branch | `DELETE /v1/branches/{branch_id_or_ref}` | **gone** for that branch's data (recovery not documented) | OAuth `environment:write` | No. |
| Other deletes | "Delete a function", "Delete project api key", "Delete a sso provider", "Remove project signing key", "Rollback migrations", "Reset a branch" (titles from the API index; paths not read) | mostly **outage** (key, function, SSO removed); rollback and reset are **gone** for data | not read | |

Sources: https://supabase.com/docs/reference/api/v1-delete-a-project,
https://supabase.com/docs/reference/api/v1-pause-a-project,
https://supabase.com/docs/reference/api/v1-run-a-query,
https://supabase.com/docs/reference/api/v1-read-only-query,
https://supabase.com/docs/reference/api/v1-restore-pitr-backup,
https://supabase.com/docs/reference/api/v1-delete-a-branch.

Emitters: Supabase CLI (`supabase projects delete`, unverified name), the Supabase MCP server
(`execute_sql`, `apply_migration`, `pause_project`, `delete_branch`, `reset_branch`, `merge_branch`,
`rebase_branch`), and any `curl`. The MCP server has `?read_only=true` and `?project_ref=<id>`.
Its own docs say destructive-SQL detection "may not catch every destructive operation".
(https://supabase.com/docs/guides/getting-started/mcp)

### Backups and PITR per plan

- Free: no daily backups mentioned; Supabase advises exporting with `supabase db dump`.
- Pro: 7 days of daily backups. Team: 14 days. Enterprise: up to 30 days.
- PITR is a paid add-on on Pro, Team and Enterprise, with 7, 14 or 28 days. With PITR on, daily
  backups stop.
- Deleting a project removes "all associated data, including any backups stored in S3. This action
  is irreversible."
- Pausing: the page says nothing about backups.

Source: https://supabase.com/docs/guides/platform/backups.

### Data API (PostgREST)

- `DELETE <ref>.supabase.co/rest/v1/<table>?<filters>` deletes matching rows. Filters are query
  parameters such as `id=eq.1`.
- **A DELETE with no filter deletes the whole table.** PostgREST's docs: "it's very easy to delete
  the entire table by omitting the query parameter", "such as by switching a request from a GET to a
  DELETE". They recommend the `pg-safeupdate` extension and add that it "does not protect against
  malicious actions, since someone can add a url parameter that does not affect the result set".
  (https://github.com/PostgREST/postgrest-docs/blob/main/docs/integrations/pg-safeupdate.rst)
- `PATCH` without a filter updates every row the same way.
- `Prefer: handling=strict, max-affected=10` makes PostgREST refuse with HTTP 400 `PGRST124` when
  more rows would be hit. It is ignored without `handling=strict`. The client chooses to send it, so
  it does not bind a hostile agent. Silo could still add it as a header on the way out; whether a
  precaution may add a header is a question for the owner (it only protects).
  (https://github.com/PostgREST/postgrest-docs/blob/main/docs/references/api/preferences.rst)
- `limit` plus `order` on DELETE/PATCH caps rows affected.
  (https://github.com/PostgREST/postgrest-docs/blob/main/docs/references/api/tables_views.rst)
- `POST /rest/v1/rpc/<function>` calls a Postgres function. A function can drop or delete anything
  its owner can. Silo cannot see what an RPC does from the request.
- Supabase documents `pg-safeupdate` as an extension
  (https://supabase.com/docs/guides/database/extensions/pg-safeupdate). Whether it is on by default
  for Data API requests is **unverified**.
- What Silo reads as facts: method, table (path), presence or absence of filter parameters, the
  `Prefer` header, and which key (Silo swaps it, so it knows if it is the secret key). "DELETE or
  PATCH with no filter parameter" is a plain fact a rule can hold.
- Outcome: **gone** on Free, **recoverable** to the last backup or PITR point on paid plans.

### Storage API

From `@supabase/storage-js` 2.119.2 source (read 2026-11-24), under `<ref>.supabase.co/storage/v1`:

- `DELETE /object/{bucket}` with body `{"prefixes": ["path1", ...]}` deletes up to 1000 objects.
- `POST /bucket/{id}/empty` empties a bucket.
- `DELETE /bucket/{id}` deletes a bucket.
- Supabase: deleted files are "permanently removed and not recoverable".
  (https://supabase.com/docs/guides/storage/management/delete-objects)
- Outcome: **gone**. No soft delete documented. Database backups do not cover object contents
  (unverified, but Supabase's guide stresses SQL deletes of `storage.objects` leave files orphaned,
  which means the files live outside the database).

### Auth admin API

- `DELETE <ref>.supabase.co/auth/v1/admin/users/{id}`, body may carry `should_soft_delete`
  (from `@supabase/auth-js` 2.119.2 source). Needs the secret/service_role key.
- Soft delete is "not reversible" by Supabase's own wording and defaults to false.
  (https://supabase.com/docs/reference/javascript/auth-admin-deleteuser)
- Outcome: **gone** for the user record unless the auth schema is restored from a database backup
  (recoverable on paid plans with backups, by restoring the whole database, which loses later writes).

## 2. Neon

Console API base: `https://console.neon.tech/api/v2`.

### Token scoping

- Personal and organization keys have broad rights; organization keys give "admin-level access to all
  organization resources".
- Project-scoped keys "cannot delete the project or manage who can access it", but "can still change
  and delete everything inside that project".
- No read-only key is documented. (https://neon.com/docs/manage/api-keys)
- The Neon MCP server has `?readonly=true`, `?projectId=<id>` and `?category=`, and a read-only
  OAuth scope chosen at sign-in. (https://neon.com/docs/ai/neon-mcp-server)

### Operations

| Operation | Wire | Outcome | Undo before pass |
|---|---|---|---|
| Delete project | `DELETE /projects/{project_id}` | **recoverable** for 7 days: `GET /projects?recoverable=true`, `POST /projects/{project_id}/recover`. Data API, GitHub integration and some credentials must be set up again. HIPAA projects cannot be recovered. Needs Admin on the project. | Not needed inside 7 days. |
| Delete branch | `DELETE /projects/{project_id}/branches/{branch_id}` | **gone** as far as the docs say ("Delete a branch and all its data"; no recovery documented). Data shared with the parent survives in the parent. | Create a branch from it first (`POST /projects/{project_id}/branches` with that branch as parent; path from the API reference, body fields unverified). Copy-on-write, so cheap. |
| Restore / reset branch | `POST /projects/{project_id}/branches/{branch_id}/restore`, body `source_branch_id`, optional `source_timestamp` or `source_lsn`, `preserve_under_name` | **recoverable**: Neon "creates a backup branch automatically before each restore", named `{branch_name}_old_{head_timestamp}`. "Reset from parent" is this call with the parent as source and no time. | Neon already does it. |
| SQL that destroys (DROP, TRUNCATE, DELETE) | Postgres wire, or WebSocket, or HTTP `/sql` (below) | **recoverable** inside the history window by instant restore; **gone** after it | Not needed inside the window. Or branch first. |

History window: Free 6 hours (capped at 1 GB), Launch 1 day default up to 7 days, Scale 1 day
default up to 30 days. (https://neon.com/docs/introduction/history-window)

Sources: https://neon.com/docs/manage/projects,
https://neon.com/docs/reference/api/branches/delete-project-branch,
https://neon.com/docs/introduction/branch-restore.

An ask for a held Neon `DROP TABLE` can truthfully say: "Neon can bring this back until <now + window>
by restoring the branch." Silo can read the window from the project settings (endpoint unverified).

### Neon SQL over HTTP and WebSocket

From `@neondatabase/serverless` 1.2.1 source (read 2026-11-24):

- HTTP: `POST https://<host with its first label replaced by "api.">/sql`. For an endpoint host
  `ep-x.<region>.aws.neon.tech` the request goes to `api.<region>.aws.neon.tech/sql` (derived from
  the code's regex `^[^.]+\.`; the exact host form is my reading).
- Headers: `Neon-Connection-String` (the full `postgresql://user:password@host/db` string),
  `Neon-Raw-Text-Output: true`, `Neon-Array-Mode: true`, and for a batch `Neon-Batch-Isolation-Level`,
  `Neon-Batch-Read-Only`, `Neon-Batch-Deferrable`. With JWT auth, `Authorization: Bearer` and the
  host prefix `apiauth.`.
- Body: one query `{"query": "...", "params": [...]}`, or a transaction `{"queries": [{query, params}, ...]}`.
- **The password is in a header, so Silo's secret swap can replace it there.** The SQL is in the
  JSON body, so the shared classifier reads `query` and every `queries[].query`.
- `Neon-Batch-Read-Only: true` makes the batch read-only on the server. It is chosen by the client.
- WebSocket mode: `wss://<host>/v2`, carrying the Postgres wire protocol inside WebSocket frames,
  with Postgres TLS turned off (`forceDisablePgSSL`) because the WebSocket is already TLS. So the
  Postgres proxy's parser applies, after Silo ends the WebSocket's TLS and unframes it.
- Limit: 64 MB per request and response. (https://neon.com/docs/serverless/serverless-driver)

## 3. Other hosted databases

### PlanetScale

API base `https://api.planetscale.com/v1`. Service tokens are granted per organization and per
database, with fine permissions. Production has its own: `delete_production_branch` is separate from
`delete_branch`, and `connect_production_branch` from `connect_branch`. Tokens can be limited to IP
ranges. A token cannot approve a deploy request it created when admin approval is required.
(https://planetscale.com/docs/api/reference/service-tokens.md)

| Operation | Wire | Outcome | Permission |
|---|---|---|---|
| Delete database | `DELETE /organizations/{organization}/databases/{database}` | **gone** as far as I found; whether backups survive is unverified | `delete_database` |
| Delete branch | `DELETE /organizations/{organization}/databases/{database}/branches/{branch}` | **gone** for that branch; production needs `delete_production_branch` | `delete_branch` |
| Deploy request (schema change) | deploy request endpoints, e.g. queue deploy request (paths not read) | **recoverable** for 30 minutes: "You can revert a deployment for up to 30 minutes after the deploying." | deploy request permissions |
| Direct DDL on production | MySQL wire or HTTP driver | refused when safe migrations is on: `ERROR 1105 (HY000): direct DDL is disabled` (error text from third-party reports, unverified on PlanetScale's site) | |
| DELETE/UPDATE rows | MySQL wire or HTTP driver | **recoverable** to the last backup (schedule and PITR per plan not read); safe migrations does not stop data deletes | `connect_*` |

Sources: https://planetscale.com/docs/api/reference/delete_database.md,
https://planetscale.com/docs/api/reference/delete_branch.md,
https://planetscale.com/docs/vitess/schema-changes/deploy-requests.md.
Backups can be protected from deletion ("Prevent backup deletion"), per
https://planetscale.com/changelog/protected-backups (not opened, from search).

HTTP driver, from `@planetscale/database` 2.0.0 source (read 2026-11-24):
`POST https://<host>/psdb.v1alpha1.Database/Execute`, body `{"query": "...", "session": {...}}`,
`Authorization: Basic base64(username:password)`. Also `/psdb.v1alpha1.Database/CreateSession`.
A swap must decode Basic, replace the password, re-encode. SQL is MySQL dialect.

Undo before pass: a backup API exists in the permission list ("backups"), so Silo could create a
backup before a held delete. Endpoint path unverified.

### Turso

- Delete database: `DELETE https://api.turso.tech/v1/organizations/{organizationSlug}/databases/{databaseName}`.
  "On paid plans, a deleted database can be restored for up to five days." Free plan: not stated.
  Outcome: **recoverable** 5 days on paid plans, otherwise treat as **gone**.
  (https://docs.turso.tech/api-reference/databases/delete.md)
- Delete group: `DELETE /v1/organizations/{organizationSlug}/groups/{groupName}` deletes the group
  and its databases; same 5-day restore on paid plans (from search summary of
  https://docs.turso.tech/api-reference/groups/delete.md, not opened).
- Delete protection: `PATCH .../databases/{databaseName}/configuration` or
  `.../groups/{groupName}/configuration` with `{"delete_protection": true}`; CLI
  `turso db config delete-protection enable <db>`. Published 2025-04-02.
  (https://turso.tech/blog/prevent-accidental-deletes-with-database-and-group-delete-protection)
  A rule should hold any PATCH that sets `delete_protection` to false: that is the step before a
  delete, as with Atlas termination protection.
- PITR exists for restoring after "dropping a table by mistake"
  (https://docs.turso.tech/features/point-in-time-recovery, from search; retention per plan not read).
- Token scoping on the Platform API: not read. Unverified.
- libSQL HTTP: `POST https://<db-host>/v2/pipeline`, `Authorization: Bearer <token>`, body
  `{"baton": ..., "requests": [{"type": "execute", "stmt": {"sql": "...", "args": [...]}}, {"type": "close"}]}`.
  (https://docs.turso.tech/sdk/http/reference.md) The Hrana spec also has `batch` and `sequence`
  request types; `sequence` takes a string of several statements (unverified: the Turso page does not
  describe them). A classifier must read every `stmt.sql`, every `batch.steps[].stmt.sql` and every
  `sequence.sql`.
- The `baton` keeps an interactive transaction open across HTTP requests. Holding one request holds
  that transaction; at 90 s Silo answers 403 and the server-side stream presumably expires
  (unverified).

### MongoDB Atlas

- Delete cluster: `DELETE https://cloud.mongodb.com/api/atlas/v2/groups/{groupId}/clusters/{clusterName}`
  with optional query `retainBackups=true|false`. "The cluster must have termination protection
  disabled in order to be deleted." Role: Project Owner. Auth: service accounts or digest.
  Returns 202. (https://www.mongodb.com/docs/api/doc/atlas-admin-api-v2/operation/operation-deletegroupcluster.md)
- Outcome: **recoverable** only if `retainBackups=true` (snapshots kept) or a Backup Compliance Policy
  is on; otherwise **gone**. The default when the parameter is left out is **unverified**.
- Termination protection is a cluster setting (`terminationProtectionEnabled`, set at create or by
  `PATCH .../clusters/{clusterName}`; the PATCH body field is unverified). A rule should hold the
  PATCH that turns it off, because that is the first of the two calls a deletion needs.
- Silo cannot add `retainBackups=true` under the current rules, because a precaution may deny, hold
  or pause but not change a request. Changing a query parameter that only protects is a case the
  owner may want to allow (challenge noted in the report).
- Atlas Data API (HTTPS CRUD on documents) is end of life per its docs page title
  (https://www.mongodb.com/docs/atlas/app-services/data-api/); date not read. Agents reach document
  data through the MongoDB wire protocol, below.
- Atlas Resource Policies can deny requests (`ATLAS_RESOURCE_POLICIES_VIOLATION_NOT_AUTHORIZED`
  appears in the error schema); whether they can forbid deletes is unverified.

### Upstash Redis

- REST API at `https://<db>.upstash.io`. Commands are paths, e.g. `/set/foo/bar`, or a JSON array in
  a POST body, e.g. `["FLUSHALL"]`. `/pipeline` takes an array of arrays (not atomic), `/multi-exec`
  the same atomically. Auth: `Authorization: Bearer <token>` or a `_token` query parameter.
- Each database has a Standard token and a Read Only token that "permits access to the read commands
  only". (https://upstash.com/docs/redis/features/restapi)
- So the destructive command is plain in the request: first path segment, or first element of each
  inner array. `FLUSHALL`, `FLUSHDB`, `DEL`, `UNLINK`, `EVAL`, `SCRIPT`, `FUNCTION` can be held by
  name. Case varies; compare case-insensitively.
- The `_token` query parameter is a second place the credential can travel. Silo's swap must handle
  it, or refuse it.
- Backups: daily backups kept 1 or 3 days, started from the console
  (https://upstash.com/docs/redis/features/backup). FLUSHALL is **recoverable** to the last daily
  backup if backups are on, otherwise **gone**.
- Delete database: `DELETE https://api.upstash.com/v2/redis/database/{id}`, Basic auth with email and
  API key (https://upstash.com/docs/devops/developer-api/redis/delete_database). Outcome **gone**;
  whether backups survive is unverified. A separate call deletes a backup:
  `DELETE /v2/redis/delete-backup/{id}/{backup_id}`.
- The Upstash MCP server has `redis_database_delete` and `redis_database_delete_backup` tools (from
  search results on glama.ai, not Upstash's site).

## 4. Wire protocols

The questions for each: can a boundary proxy read commands, can it hold one while the session stays
up, how does a refusal look, and can it swap the password.

### MySQL

- **Start of the connection.** The server speaks first with a plaintext greeting. If the server
  advertises `CLIENT_SSL`, the client sends an SSLRequest (a Handshake Response cut before the
  username), then TLS starts, then the real Handshake Response goes inside TLS.
  (https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_connection_phase_packets_protocol_ssl_request.html)
  This is the same shape as Postgres's SSLRequest. A proxy ends the client's TLS with its own
  certificate and opens its own TLS to the server.
- **Commands.** `COM_QUERY` is byte `0x03` followed by the SQL text to the end of the packet; with
  `CLIENT_QUERY_ATTRIBUTES` a parameter block comes first. "Execution starts immediately."
  (https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_com_query.html)
  `DROP TABLE t`, `TRUNCATE t`, `DELETE FROM t` appear as plain text in that packet.
  Prepared statements arrive as `COM_STMT_PREPARE` (text) then `COM_STMT_EXECUTE` (by id): the proxy
  must keep the id-to-text map. `COM_INIT_DB` changes database; `COM_FIELD_LIST` and others are
  harmless.
- **Multi-statements.** With `CLIENT_MULTI_STATEMENTS`, one COM_QUERY can hold `SELECT 1; DROP TABLE t`.
  A proxy can clear that flag in the handshake it forwards, so the server refuses multi-statements.
  That is a change to the stream, not a decision on a request.
- **Hold.** The protocol is one command, one reply, per connection. Holding a COM_QUERY just delays
  the reply. Client read timeouts vary by driver (unverified per driver).
- **Refusal.** An ERR packet: `0xFF`, 2-byte error code, `#`, 5-char SQLSTATE, message. Example from
  the docs: code 1096, `HY000`, "No tables used".
  (https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_basic_err_packet.html)
  The session stays usable. Silo could answer with a standard access-denied code such as 1142
  (`ER_TABLEACCESS_DENIED_ERROR`, SQLSTATE 42000; code from memory, unverified) and Silo's sentence
  as the message.
- **Password swap.** `caching_sha2_password` is the default in 8.4. Its fast path is a
  challenge-response and cannot be forwarded with a different password. Its full path, on a TLS
  connection, sends the password "as cleartext but cannot be snooped because the connection is
  secure". (https://dev.mysql.com/doc/refman/8.4/en/caching-sha2-pluggable-authentication.html)
  So a proxy that is the server to the client can always answer "full authentication required",
  receive the placeholder in clear over its own TLS, check it, and then sign in to the real server
  with the real password as a client. The swap works; it means two separate handshakes, not a relay.
  `mysql_native_password` is deprecated in 8.4 (disabled by default in 8.4 per my knowledge,
  unverified on that page).
- **Traps.** `/*! ... */` executable comments run their content on MySQL: `/*!50000 DROP TABLE t */`
  is a DROP. `#` starts a comment. `sql_safe_updates=1` makes the server refuse UPDATE/DELETE without
  a key in WHERE or a LIMIT; a session can turn it off with `SET`, so it binds nobody hostile.
  `PREPARE s FROM @q; EXECUTE s;` runs SQL built in a variable. Stored procedures (`CALL p()`) and
  events can drop. `LOAD DATA LOCAL INFILE` makes the server ask the client for a file. `REPLACE INTO`
  deletes then inserts.

### MongoDB

- **Start of the connection.** Atlas requires TLS from the first byte (`mongodb+srv://` turns TLS on;
  from knowledge, unverified that day). A proxy must end TLS to read anything.
- **Commands.** `OP_MSG` (opcode 2013): header (length, requestID, responseTo, opCode), `flagBits`,
  sections. Kind 0 is one BSON body; kind 1 is a document sequence with an identifier (for example
  `deletes` or `documents`) whose name must not also be in the body. `OP_QUERY` is gone since 5.1
  except for `hello`/`isMaster` at handshake.
  (https://www.mongodb.com/docs/manual/reference/mongodb-wire-protocol/)
  The command is the first key of the body; the database is `$db` (standard, not re-read on that page).
- **What destroys.**
  - `{dropDatabase: 1, $db: "x"}`, `{drop: "coll"}`, `{dropIndexes: ...}`.
  - `deleteMany` is a driver name. On the wire it is `{delete: "coll", deletes: [{q: {}, limit: 0}]}`.
    `limit: 0` means all matches; an empty `q` matches everything.
    (https://www.mongodb.com/docs/manual/reference/command/delete/) The `deletes` array may arrive as a
    kind 1 sequence, not in the body.
  - Others to classify: `update` with `multi: true` and an empty `q`; `findAndModify` with
    `remove: true`; `aggregate` with a final `$out` or `$merge` stage (overwrites a collection);
    `renameCollection` with `dropTarget: true`; `dropUser`, `dropAllUsersFromDatabase`;
    `bulkWrite` (8.0, cross-collection). (List from knowledge, not each verified.)
- **Compression trap.** `OP_COMPRESSED` (opcode 2012) wraps any message with snappy, zlib or zstd.
  Drivers negotiate it in `hello`. A proxy must decompress, or remove `compression` from the client's
  `hello` so none is agreed.
- **Hold.** Requests carry a `requestID`, replies a `responseTo`. Drivers pool connections and most
  send one command at a time per connection, so holding one is fine. With `moreToCome` set (writes
  with `w: 0`) the client expects no reply at all.
- **Refusal.** An `OP_MSG` reply with `responseTo` set and body `{ok: 0, errmsg: "...", code: 13,
  codeName: "Unauthorized"}` (shape and code from knowledge, unverified that day). The connection
  stays up.
- **Password swap.** SCRAM-SHA-256 via `saslStart`/`saslContinue`, often in `hello` as
  `speculativeAuthenticate`. The proxy can be the SCRAM server for the placeholder (it knows it) and
  run its own SCRAM to Atlas with the real password: two handshakes again. X.509 and OIDC/AWS-IAM
  auth cannot be swapped this way (same open question as SigV4 in the brief).

### Redis (RESP)

- **Start.** Redis 6+ supports native TLS from the first byte; hosted services (Upstash, Redis Cloud,
  ElastiCache with in-transit encryption) use it (unverified per provider). No STARTTLS.
- **Commands.** RESP arrays of bulk strings: `*1\r\n$8\r\nFLUSHALL\r\n`. Inline commands (plain text
  lines) are also accepted. The command name is the first element, any case.
- **What destroys.** `FLUSHALL [ASYNC|SYNC]` deletes every key in every database, "clears the RDB
  persistence file" and saves an empty one if `save` is on.
  (https://redis.io/docs/latest/commands/flushall/) Also `FLUSHDB`, `DEL`, `UNLINK`, `SWAPDB`,
  `RENAME`, `RESTORE ... REPLACE`, `MIGRATE`, `FUNCTION FLUSH`, `SCRIPT FLUSH`, `SHUTDOWN`,
  `CONFIG SET` (for example `dir`/`dbfilename` to overwrite files, or turning persistence off),
  `ACL SETUSER`/`ACL DELUSER`, `DEBUG`.
- **Scripting trap.** `EVAL "redis.call('FLUSHALL')" 0`, `EVALSHA <sha> 0` and `FCALL` run commands
  inside Lua. The proxy sees only the script text (EVAL) or a hash (EVALSHA). Hold all three, or
  allow them only for known script hashes.
- **ACL as the provider-side control.** `ACL SETUSER u on >pw +@all -@dangerous ~*` removes
  FLUSHALL, CONFIG, DEBUG, MIGRATE and others. Key patterns do not limit FLUSHALL, FLUSHDB or SWAPDB:
  a user with `~tenant1:* +@all` "can still run FLUSHALL"; Redis says to add
  `-flushall -flushdb -swapdb`. Refusal text: `NOPERM this user has no permissions to run the 'set'
  command`. (https://redis.io/docs/latest/operate/oss_and_stack/management/security/acl/)
- **Hold.** Replies must come back in order. Clients pipeline heavily, so holding one command holds
  every command behind it on that connection. The computer keeps running, but that client stalls.
  Inside `MULTI`, commands are queued and run at `EXEC`; a refusal at queue time makes `EXEC` fail
  with `EXECABORT` (standard Redis behaviour, unverified that day). A proxy should decide on `EXEC`
  by looking at the whole queued block.
- **Refusal.** An error reply `-ERR <text>\r\n` or `-NOPERM <text>\r\n`. The connection stays up.
- **Password swap.** `AUTH <password>`, `AUTH <user> <password>`, or `HELLO 3 AUTH <user> <password>`
  carry the password in clear inside TLS. Silo can replace it in place. This is the easiest of the
  three.

## 5. SQL over HTTPS: one classifier, many hosts

| Host and path | Where the SQL is | Several statements in one string? | Source |
|---|---|---|---|
| `api.supabase.com` `POST /v1/projects/{ref}/database/query` | `query` | likely yes (unverified) | https://supabase.com/docs/reference/api/v1-run-a-query |
| Neon `api.<region>...` `POST /sql` | `query`, or `queries[].query` | unverified | `@neondatabase/serverless` 1.2.1 source |
| PlanetScale `<host>` `POST /psdb.v1alpha1.Database/Execute` | `query` | unverified | `@planetscale/database` 2.0.0 source |
| Turso/libSQL `<db-host>` `POST /v2/pipeline` | `requests[].stmt.sql` (plus `batch` steps and `sequence.sql`, unverified) | `sequence` yes (unverified) | https://docs.turso.tech/sdk/http/reference.md |
| Cloudflare D1 `api.cloudflare.com` `POST /client/v4/accounts/{account_id}/d1/database/{database_id}/query` | `sql` | yes: "Supports multiple statements, joined by semicolons" | https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/query/ |
| Snowflake `<account>.snowflakecomputing.com` `POST /api/v2/statements` | `statement` | yes, separated by `;` (a count parameter may be needed, unverified) | https://docs.snowflake.com/en/developer-guide/sql-api/submitting-requests |
| BigQuery `bigquery.googleapis.com` `POST /bigquery/v2/projects/{projectId}/queries` | `query` | yes, multi-statement scripts (unverified) | https://docs.cloud.google.com/bigquery/docs/reference/rest/v2/jobs/query |
| Supabase Data API `POST /rest/v1/rpc/{fn}` | none: a function name | n/a | PostgREST docs |

BigQuery also takes `jobs.insert` with a job configuration whose `writeDisposition: WRITE_TRUNCATE`
replaces a table's contents with a SELECT's result (unverified that day). The destruction is in a
JSON field, not in the SQL. The classifier for that host must read both.

### Statement classes to hold

- `DROP` of anything holding data: `DATABASE`, `SCHEMA` (with or without `CASCADE`), `TABLE`, `VIEW`
  (outage), `MATERIALIZED VIEW`, `INDEX` (outage), `FUNCTION`, `TYPE ... CASCADE`, `OWNED BY`.
- `TRUNCATE` (any form; Postgres `CASCADE` reaches other tables).
- `DELETE` and `UPDATE` with no `WHERE`, or with a `WHERE` that is always true (`1=1`, `true`,
  `id = id`, `id IS NOT NULL` on a NOT NULL column). A parser can catch literals; it cannot catch
  every tautology. Treat "row count unknown" as the honest fact.
- `ALTER TABLE ... DROP COLUMN`, `DROP CONSTRAINT` (outage), `ALTER ... TYPE` with `USING` (rewrites
  data), `RENAME` (outage for the app).
- `MERGE ... WHEN MATCHED THEN DELETE`; MySQL `REPLACE INTO`; `INSERT ... ON CONFLICT DO UPDATE`
  without a narrow target (overwrites).
- `CREATE OR REPLACE TABLE` (Snowflake, BigQuery, SQLite via `DROP`+`CREATE`): replaces a table with
  a new one and its data is gone (Snowflake can `UNDROP` within Time Travel, unverified).
- Restores and role changes: `ALTER ROLE ... PASSWORD` or `REVOKE` (outage),
  `pg_terminate_backend` (outage), Postgres `lo_unlink` (large objects gone).
- SQLite/libSQL/D1: `DROP`, `DELETE`, `PRAGMA writable_schema=ON` followed by writes to
  `sqlite_master`, `ATTACH` of another file then destruction there, `VACUUM INTO` (writes a file).

### Traps a classifier must handle

1. **Several statements in one string.** `SELECT 1; DROP TABLE users`. Split on the dialect's
   grammar, never on `;` alone, because `;` appears inside strings and function bodies.
2. **Comments.** `--` to end of line, `/* */` (nested in Postgres, not in MySQL), MySQL `#`, MySQL
   executable `/*! ... */` and `/*+ ... */` optimizer hints. Strip ordinary comments; treat
   `/*! ... */` content as SQL.
3. **Quoting.** Postgres dollar quotes `$$ ... $$` and `$tag$ ... $tag$`, `E'...'` escapes, MySQL
   backslash escapes and backticks, double-quoted identifiers that look like keywords.
4. **CTEs with writes.** `WITH d AS (DELETE FROM t RETURNING *) SELECT count(*) FROM d` starts with
   WITH and reads like a SELECT.
5. **Procedural blocks.** Postgres `DO $$ BEGIN EXECUTE 'DROP TABLE t'; END $$`, `CREATE FUNCTION`
   followed by `SELECT f()`, `CALL p()`, triggers created now that delete later. `EXECUTE format(...)`
   builds SQL at run time. The SQL text that destroys may not be in the request at all.
6. **Functions that drop.** `SELECT my_cleanup()` or `SELECT * FROM rpc` can do anything the
   function's owner can. Only the database knows. A classifier can only say "calls a user function".
7. **Prepared and built SQL.** MySQL `SET @q = 'DROP TABLE t'; PREPARE s FROM @q; EXECUTE s`.
   Postgres `PREPARE` then `EXECUTE` by name across requests.
8. **Session switches.** `SET sql_safe_updates = 0`, `SET session_replication_role = replica`
   (disables triggers and FK checks), `SET ROLE`, `SET search_path` (changes which `t` is meant).
9. **Case, whitespace and Unicode.** Keywords in any case, odd whitespace, and identifiers in other
   scripts. Normalize before matching.
10. **Parameters.** `parameters`/`params`/`args` carry values, not SQL, in every API above. A
    destructive statement can still be hidden in a string value passed to a function that runs it.

### What this means for Silo's rules

- The parser's output ("this request contains a `DROP TABLE` on `users`") is close to a fact, but a
  parser can be wrong or fooled. The safest reading under the current rules: a rule may hold "SQL the
  parser could not fully classify", and an extension may add holds as a precaution. A request whose
  SQL fails to parse should be held, never allowed by default.
- The same classifier and the same ask wording should serve the Postgres proxy and every host in the
  table, with one adapter per host saying where the SQL sits in the JSON.

## Could not verify

- Whether Supabase turns `pg-safeupdate` on for Data API requests by default.
- Whether a deleted Neon branch can be recovered; the exact create-branch body.
- PlanetScale: whether backups survive database deletion; backup API path; the "direct DDL is
  disabled" text on PlanetScale's own site.
- Atlas: the default of `retainBackups`; the PATCH field for termination protection; the Data API
  end-of-life date.
- Turso: free-plan recovery; `batch`/`sequence` formats; Platform API token scoping.
- Upstash: whether backups survive database deletion; a backup-creation API.
- MongoDB error reply shape and code 13, MySQL error code 1142, Redis `EXECABORT`, BigQuery
  `WRITE_TRUNCATE`, Snowflake multi-statement count: from knowledge, not read that day.
