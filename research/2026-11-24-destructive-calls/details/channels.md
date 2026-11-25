# Channels other than plain REST

This file covers the ways an agent reaches Railway, Supabase, Neon, Vercel, Cloudflare, GitHub,
Stripe, AWS, Linear, Sentry, Fly, Heroku, Kubernetes and Docker without writing a plain REST call
itself. For each channel it asks: can Silo's boundary see the destructive call, and can it hold it?

Every source was read on 2026-11-24 unless a line says otherwise. Repositories were read at the
commit named next to them. "Unverified" means I could not confirm the point from a primary source.

## The short answer

| Channel | Where the destructive call is made | What Silo sees | Can a rule hold it on facts? |
|---|---|---|---|
| Remote MCP server (Supabase, Neon, GitHub, Stripe, Vercel, Cloudflare, Linear, Sentry, Railway, AWS) | On the vendor's servers | One `POST` per JSON-RPC message to the MCP host, with the tool name and arguments in the body (and, from protocol 2026-07-28, in `Mcp-Method` / `Mcp-Name` headers) | Yes, if Silo ends TLS on the MCP host. It does when the user gave Silo the token for that host |
| Remote MCP with "code mode" (Cloudflare `execute`, AWS `run_script`) | On the vendor's servers, by code the model wrote | A tool call whose argument is a program | Only the tool name is a fact. What the program deletes is an opinion |
| Local stdio MCP server inside the computer (GitHub local, Supabase npm, AWS API MCP server, Railway `mcp local`) | From inside the computer, as ordinary API calls | The vendor's REST or GraphQL request, as for any program | Yes, the REST catalogue applies unchanged |
| Provider-run tools (Anthropic MCP connector, OpenAI remote MCP tool) | On Anthropic's or OpenAI's servers | Only the model request that declares the server, and the model response that reports the call | Not the call itself. A rule can deny or hold the model request that declares the server or carries the approval |
| Hosted agents (Cursor Cloud Agents, ChatGPT and Claude.ai connectors) | On someone else's machines | Nothing | No |
| CLI that ends in REST (`heroku pg:reset`, most `flyctl`, `kubectl delete`, `supabase` Management API calls) | From inside the computer | The REST request | Yes |
| CLI that opens a database or SSH channel (`fly ssh`, `fly proxy`, `railway connect`, `railway run` + `psql`, `supabase db reset/push`, `docker` over `ssh://`, `kubectl exec`) | Inside a raw connection | Only the connection, plus the REST call that set it up | Only the setup call and the connection. What runs inside is opaque, except Postgres through the wire proxy another thread designs |

## 1. Vendor MCP servers

### What the MCP spec says about annotations

A tool may carry `annotations`. The schema defines four hints and their defaults
([schema.ts, 2025-06-18](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-06-18/schema.ts)):

| Hint | Meaning | Default when absent |
|---|---|---|
| `readOnlyHint` | the tool does not modify its environment | `false` |
| `destructiveHint` | the tool may perform destructive updates (only meaningful when not read-only) | `true` |
| `idempotentHint` | calling again with the same arguments has no further effect | `false` |
| `openWorldHint` | the tool reaches an open world of external entities | `true` |

The spec says: "clients **MUST** consider tool annotations to be untrusted unless they come from
trusted servers" ([tools, 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)).

Why Silo must not use annotations as facts:

- The server writes them, not the user. They say what the vendor thinks, in the vendor's words.
- They are wrong on real cases. Supabase marks `pause_project` as `destructiveHint: false`
  (`packages/mcp-server-supabase/src/tools/account-tools.ts`, commit `56093a6`). Pausing stops the
  project, which in our words is an **outage**.
- They are coarse. Stripe's `stripe_api_write` covers every `POST`, `PATCH`, `PUT` and `DELETE`
  of the API, refunds included ([docs.stripe.com/mcp](https://docs.stripe.com/mcp)). One hint
  cannot describe that tool.
- The tool list can change between two `tools/list` calls. The spec allows it to vary "by the
  authorization presented on the request".
- In Silo the computer is hostile. The client that reads the list runs inside it, so whatever the
  client does with hints is under the agent's control.

They are still a useful default. A Silo catalogue entry for an MCP host could start from the
vendor's `destructiveHint: true` list and let the user hold every tool on it. A tool missing from
Silo's catalogue that the server marks destructive (or leaves unmarked, which means destructive
by default) is a good candidate for "hold". This fits a precaution, not a rule: the hint is the
vendor's opinion. A rule can name tools only by their wire name on a given host.

### Read-only modes: two kinds

This is the most useful split for Silo.

1. **Read-only bound to the credential.** The provider enforces it whatever the client sends.
   Examples: Neon's OAuth consent with "Allow writes" unchecked; Linear's `read` OAuth scope or a
   Read-only API key; Stripe's restricted agent API keys; GitHub fine-grained PAT permissions.
2. **Read-only chosen by the client.** It is a URL path, query parameter or header the client
   sends. Examples: Supabase `?read_only=true`, GitHub `/readonly` or `X-MCP-Readonly`, Linear
   `/mcp/readonly`, Neon `?readonly=true` with an API key, Cloudflare and Sentry tool filters.
   A hostile agent removes the parameter and gets the write tools back.

Silo can turn kind 2 into kind 1 at the boundary: it sees every request to the MCP host, so a
rule can require the parameter or header (for example, deny any request to
`api.githubcopilot.com` whose path does not end in `/readonly` and that lacks
`X-MCP-Readonly: true`). That is a fact about the request, so it fits a rule. Silo could also add
the header itself, but rules today only allow, hold or deny, so "require" fits better than
"rewrite".

### Server by server

| Server | Hosted or local | Transport | Destructive tools (wire names) | Read-only / scoping | Annotations |
|---|---|---|---|---|---|
| Supabase | Hosted `https://mcp.supabase.com/mcp`; also the npm package `@supabase/mcp-server-supabase` run locally | Streamable HTTP; stdio for npm | `execute_sql`, `apply_migration`, `delete_branch`, `merge_branch`, `reset_branch`, `rebase_branch`, `deploy_edge_function`, `run_notebook`, `delete_notebook`, `update_storage_config`, `pause_project`, `create_project` (money), `create_branch` (money) | `read_only=true` runs SQL "as a read-only Postgres user"; `project_ref` scopes to one project and hides account tools; `features=` picks tool groups | Yes, every tool. `pause_project` and `restore_project` are marked `destructiveHint: false` |
| Neon | Hosted `https://mcp.neon.tech/mcp` | Stateless streamable HTTP; `/sse` returns 410 | `run_sql`, `run_sql_transaction`, `delete_project`, `delete_branch`, `reset_from_parent`, `restore_snapshot`, `delete_snapshot`, `delete_postgres_database`, `delete_postgres_role`, `delete_postgres_endpoint`, `suspend_postgres_endpoint`, `reset_postgres_role_password`, `revoke_credential`, `delete_auth_user`, `disable_auth`, `delete_storage_bucket`, `delete_storage_object(s_by_prefix)`, `delete_function`, `delete_data_api`, `disable_realtime`, others | `?readonly=true` or OAuth "Allow writes" off; `run_sql` then runs in a read-only transaction. An org API key limits to that org | Yes; generated tools default `destructiveHint` to `!readOnlyHint`. The server also tells the model in text: "NEVER invoke one autonomously" |
| GitHub | Hosted `https://api.githubcopilot.com/mcp/`; local `github-mcp-server` (Docker or binary) | Streamable HTTP; stdio locally | `delete_repository`, `delete_file`, `push_files`, `create_or_update_file`, `merge_pull_request`, `label_write` (delete method), `actions_run_trigger`, `update_dependabot_alert`, notification `delete` | Toolsets (`X-MCP-Toolsets`, `/x/<toolset>`), `X-MCP-Tools`, `X-MCP-Readonly` or `/readonly`, local `--read-only`. "Lockdown mode is a best-effort content filter, not a security boundary" | Yes, 163 hint fields in `pkg/github` (commit `9a72d8d`) |
| Stripe | Hosted `https://mcp.stripe.com` only; the local `@stripe/mcp` proxy is deprecated | Streamable HTTP | `stripe_api_write` (any POST/PATCH/PUT/DELETE in the supported list: refunds, subscription cancel, invoice void, coupon delete, webhook endpoints), `send_money`, `transfer_money`, `convert_currency` | Restricted "agent API keys" and per-environment OAuth grants. From 2027-01-11 Stripe MCP refuses full-access secret keys and non-agent restricted keys | Unverified |
| Vercel | Hosted `https://mcp.vercel.com`, OAuth only, approved clients or CIMD | Streamable HTTP | `delete_drive`, `pause_project`, `cancel_deployment`, `request_rollback`, `replace_domain_dns_records`, `update_record`, `put_firewall_config`, `update_firewall_config`, `edit_project_env`, `revoke_kms_signing_key`, `invalidate_by_tags`, `buy_domain(s)` (money). No `delete_project`: there is `get_project_deletion_link` | Team grant at OAuth time; no read-only mode found | Unverified (closed source) |
| Cloudflare | Hosted `https://mcp.cloudflare.com/mcp` (whole API) plus `*.mcp.cloudflare.com` domain servers | Streamable HTTP | Code mode: `execute` (JavaScript calling `cloudflare.request()`). With `?codemode=false`: one tool per endpoint, about 3,000, such as `d1_delete` (`DELETE /accounts/{account_id}/d1/database/{database_id}`) | OAuth scopes chosen at consent; API token permissions | Yes. `execute` is `destructiveHint: true`; generated tools carry hints and the REST method and path |
| Railway | Hosted `https://mcp.railway.com`; `railway mcp` (stdio bridge to hosted); `railway mcp local` (in-process, calls the API) | Streamable HTTP; stdio | Hosted: `delete-feature-flag`, `delete-automation`, `delete-automation-trigger`, `accept-deploy`, `redeploy`. Local only: `remove_service`, `remove_volume`, `remove_bucket`, `remove_tcp_proxy`, `delete_domain` | OAuth picks workspaces and projects; project tokens refused. No read-only mode | Docs say destructive tools are marked; local server previews and requires `confirm: true` |
| AWS | Hosted "AWS MCP Server" `https://aws-mcp.<region>.api.aws/mcp`; older local `awslabs/aws-api-mcp-server` | Streamable HTTP; SigV4 through local `mcp-proxy-for-aws` (stdio), or OAuth | `call_aws` (any of the AWS CLI's operations), `run_script` | IAM; context keys `aws:ViaAWSMCPService` and `aws:CalledViaAWSMCP` let a policy deny deletes made through the MCP server. Local server: `READ_OPERATIONS_ONLY`, `REQUIRE_MUTATION_CONSENT`, a `denyList` file | Unverified |
| Linear | Hosted `https://mcp.linear.app/mcp`; deprecated `/sse` | Streamable HTTP; HTTP+SSE | Docs name no tool. The Linear connector in this session lists `delete_comment`, `delete_attachment`, `delete_status_update`, `delete_diff_comment`, `retire_issue_label`, `merge_diff` (observed, not documented) | `/mcp/readonly` (client-chosen), `read` OAuth scope, Read-only API key (bound to the credential) | Unverified |
| Sentry | Hosted `https://mcp.sentry.dev/mcp`; local `sentry mcp` | Streamable HTTP; stdio | `delete_alert_rule`, `delete_metric_monitor`, `delete_uptime_monitor`, `delete_monitor_environment`, `remove_team_from_project`, `unlink_issue`, `update_issue`, `update_project`, `update_dsn` | `?skills=` / `?disable-skills=` (client-chosen); token scopes | Yes (`destructiveHint: true` on the tools above, commit `a070341`) |

Sources: Supabase [README](https://github.com/supabase-community/supabase-mcp) and
[docs](https://supabase.com/docs/guides/getting-started/mcp), tool files under
`packages/mcp-server-supabase/src/tools/`; Neon [README](https://github.com/neondatabase/mcp-server-neon),
`mcp/tools/definitions.ts`, `mcp/tools/grant-filter.ts` (commit `0852e43`), and the live list at
`https://mcp.neon.tech/api/list-tools`; GitHub [README](https://github.com/github/github-mcp-server),
[remote-server.md](https://github.com/github/github-mcp-server/blob/main/docs/remote-server.md);
Stripe [docs.stripe.com/mcp](https://docs.stripe.com/mcp), [agent-toolkit tools/README](https://github.com/stripe/agent-toolkit/blob/main/tools/README.md);
Vercel [overview](https://vercel.com/docs/agent-resources/vercel-mcp) and
[tools reference](https://vercel.com/docs/agent-resources/vercel-mcp/tools);
Cloudflare [cloudflare/mcp README](https://github.com/cloudflare/mcp) (commit `ad6cb10`),
`src/tools/execute.ts`, `src/generated/forge-mcp/d1/tools.ts`, and the
[domain servers list](https://github.com/cloudflare/mcp-server-cloudflare);
Railway [CLI MCP](https://docs.railway.com/cli/mcp) and [MCP server](https://docs.railway.com/ai/mcp-server);
AWS [setup guide](https://docs.aws.amazon.com/agent-toolkit/latest/userguide/getting-started-aws-mcp-server.html),
[aws-api-mcp-server README](https://github.com/awslabs/mcp/blob/main/src/aws-api-mcp-server/README.md),
[Understanding IAM for managed AWS MCP servers](https://aws.amazon.com/blogs/security/understanding-iam-for-managed-aws-mcp-servers/)
(context key names from the search summary of that post; I could not load its body, so the exact
condition syntax is unverified); Linear [docs](https://linear.app/docs/mcp); Sentry
[README](https://github.com/getsentry/sentry-mcp) and `packages/mcp-core/src/tools/catalog/`.

### Confirmations that the vendor builds in, and why they do not replace a hold

Several servers now ask for a confirmation before a destructive tool runs:

- GitHub `delete_repository` asks the user to type the repository's full name, through MCP
  elicitation (`pkg/github/repositories.go`). The confirmed request comes back in a second
  `tools/call` with the answer in it (multi round-trip requests, MRTR).
- Supabase can ask for a form confirmation on `execute_sql`, `apply_migration`, `create_project`,
  `create_branch`, `run_notebook`, `delete_notebook`. The code says: "Clients without form
  capability run raw SQL without a prompt" (`src/server.ts`).
- AWS's local server has `REQUIRE_MUTATION_CONSENT`, also through elicitation.
- Railway's local server returns a preview and then requires `confirm: true` in the arguments.
- Stripe's "Human confirmation" is different: Stripe asks the person to approve in the Stripe
  Dashboard through a link, then hands the agent an approval token. Approvals expire after 24 h.
- Vercel hands project deletion to a human through `get_project_deletion_link`.

Elicitation and `confirm: true` are answered by the MCP client. In Silo the client runs inside
the hostile computer, so the agent can answer them itself. They are not a human in the loop
here. Stripe's Dashboard approval and Vercel's deletion link are out of band and do involve a
person. Silo should still hold, because the vendor decides which actions get that treatment.

A useful fact for Silo: under MRTR the confirmed retry is a separate `tools/call` that carries
`inputResponses` (and `requestState`) in `params`. A rule can hold exactly that request: it is
the moment just before the destructive call, and it names the tool and the arguments.

## 2. MCP on the wire, and how Silo reads it

### Streamable HTTP, protocol 2026-07-28 (current)

([streamable HTTP, 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http))

- One endpoint. Every client message is its own `POST`. "The body of the HTTP POST **MUST** be a
  single JSON-RPC *request* or *notification*." No batches.
- No protocol sessions, no `GET` stream, no `Last-Event-ID` resume.
- The server answers with `application/json` or with a `text/event-stream` scoped to that request.
  Closing that stream cancels the request.
- Required headers mirror the body: `MCP-Protocol-Version`, `Mcp-Method` (from `method`) and
  `Mcp-Name` (from `params.name` for `tools/call`). A tool may also mark parameters with
  `x-mcp-header`, which the client must mirror as `Mcp-Param-{Name}`. Values that are not plain
  ASCII are sent as `=?base64?...?=`.
- The server must reject a request whose headers do not match the body with `400` and JSON-RPC
  error `-32020` (`HeaderMismatch`).

A `tools/call` looks like this:

```http
POST /mcp HTTP/1.1
Host: mcp.supabase.com
Authorization: Bearer <token>
Content-Type: application/json
Accept: application/json, text/event-stream
MCP-Protocol-Version: 2026-07-28
Mcp-Method: tools/call
Mcp-Name: delete_branch

{"jsonrpc":"2.0","id":7,"method":"tools/call",
 "params":{"name":"delete_branch","arguments":{"branch_id":"..."},
           "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28", "...": "..."}}}
```

(The host and tool are real; the exact argument name for Supabase's `delete_branch` is not
checked here.)

### Streamable HTTP, protocols 2025-03-26 to 2025-11-25

- Same single `POST` endpoint, but the server may hand out an `Mcp-Session-Id` at `initialize`,
  ended by `DELETE`; the client may open a `GET` SSE stream; streams are resumable.
- 2025-03-26 allowed JSON-RPC batches (a JSON array). 2025-06-18 removed them
  ([changelog](https://modelcontextprotocol.io/specification/2025-06-18/changelog)). A server
  written for 2025-03-26 may still accept an array.
- 2025-06-18 added the `MCP-Protocol-Version` header, but not `Mcp-Method` or `Mcp-Name`.

### HTTP+SSE, protocol 2024-11-05 (deprecated, still served by Linear at `/sse`)

The client opens a `GET` SSE stream. The first event, `endpoint`, gives a URL. The client then
`POST`s each JSON-RPC message to that URL; the `POST` answers `202` and the reply arrives on the
`GET` stream. For Silo the `POST` still carries the tool name and arguments in its body. The
answer is on another connection, which matters only for the record, not for a hold.

### How Silo should recognise a destructive MCP call

1. Treat a host as an MCP host when the user's rule says so (for example `mcp.supabase.com`), not
   by guessing. A rule key is the pair (host, wire tool name).
2. Parse the body. The body is what the server executes. The headers are a convenience: a hostile
   client could send a false `Mcp-Name`. The spec says servers must reject a mismatch, but Silo
   should not depend on that. Silo should decide on the body, and deny when a mirrored header
   disagrees with it.
3. Accept a single JSON object or, for old servers, an array. For an array, the request takes the
   strictest outcome of its elements (deny beats hold beats allow), and a hold shows every element.
4. Refuse what Silo cannot parse with certainty: duplicate JSON keys (`"name"` twice is read
   differently by different parsers), invalid UTF-8, an unknown `Content-Encoding`, a body cut by
   a size limit. Deny, because a request no rule allows is denied.
5. A rule needs the arguments too, not only the name. Stripe's `stripe_api_write`, Cloudflare's
   `execute`, AWS's `call_aws`, Neon's `run_sql` and Supabase's `execute_sql` all hide very
   different actions behind one name. For the first, the argument names the API method; for SQL,
   the argument is SQL text, which the Postgres proxy thread's rules could reuse; for code, see
   below.
6. Tool names differ between the model and the wire. Claude Code shows the model
   `mcp__supabase__execute_sql`; the wire carries `execute_sql`. Rules should use the wire name.

### Code mode: the call is inside a program

Cloudflare's main server exposes `search`, `execute`, `docs` and `whoami`. `execute` takes a
JavaScript program that calls `cloudflare.request()`, and Cloudflare runs it on its side
([README](https://github.com/cloudflare/mcp)). AWS's hosted server has `run_script` (unverified
beyond a third-party write-up). For these, Silo sees only that a program was sent. Whether it
deletes is an opinion. Two options:

- A rule on the fact: hold every `execute` call to `mcp.cloudflare.com`. Noisy, but honest.
- A rule that forces the server's other mode: deny requests to `mcp.cloudflare.com/mcp` unless
  the query has `codemode=false`. Then each endpoint is its own tool, named after the REST method
  (`d1_delete` for `DELETE /accounts/{account_id}/d1/database/{database_id}`), and the REST
  catalogue maps onto tool names. Cloudflare warns this costs far more context.

### A local stdio server is just a program

When the MCP server runs inside the computer (local `github-mcp-server`, the Supabase npm package,
`awslabs/aws-api-mcp-server`, `railway mcp local`, Sentry's `sentry mcp`), the MCP messages never
leave the computer. What leaves is the vendor's ordinary API traffic. Examples from the code:

- Supabase npm `execute_sql` sends `POST https://api.supabase.com/v1/projects/{ref}/database/query`
  with body `{query, parameters, read_only}`; `delete_branch` sends
  `DELETE /v1/branches/{branch_id_or_ref}`; `reset_branch` sends
  `POST /v1/branches/{branch_id_or_ref}/reset`; `pause_project` sends
  `POST /v1/projects/{ref}/pause` (`src/platform/api-platform.ts`).
- AWS's local server runs AWS CLI operations with boto3, so the traffic is SigV4-signed AWS API
  calls. That falls under the open question of signing keys, not under MCP.

So the REST catalogue covers local servers with no MCP work. One caution: the `read_only` flag of
Supabase's local server is a field in the request body, chosen inside the computer. A rule can
require `read_only: true` in that body; that is a fact.

### Sessions and tokens

- `Mcp-Session-Id` (2025 protocols) is a routing id, not a credential. The 2025 security best
  practices say servers must not use sessions for authentication
  ([2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/security_best_practices);
  not re-read today, unverified). Silo has no reason to swap it.
  The 2026-07-28 protocol drops it.
- Remote MCP servers authenticate with `Authorization: Bearer`. The token is either a static
  token the user creates (GitHub PAT, Supabase PAT, Neon API key, Stripe agent API key, Linear API
  key, Sentry user token, Cloudflare API token) or an OAuth access token from the MCP
  authorization flow.
- A static token works with Silo's secrets as they are: the computer's MCP config holds the
  placeholder in a header (Claude Code's `.mcp.json` `headers`, Codex's `bearer_token_env_var`
  and VS Code's `headers` are documented in the Stripe and GitHub setup pages above; Cursor is
  unverified), and
  the boundary swaps the real value only on the way to that host. Silo then ends TLS on the MCP
  host and reads every `tools/call`. This is the path to recommend.
- An OAuth token is minted to the client inside the computer, together with a refresh token, and
  stored there (in a credentials file or keychain of the client; location varies by client,
  unverified). The agent can read and reuse it. Silo cannot swap what it never had. To cover
  OAuth-only servers (Vercel; Railway's hosted server, which refuses project tokens), Silo would
  have to run the OAuth flow itself as the client, keep both tokens outside the computer, refresh
  them, and hand the computer a placeholder. Vercel also only accepts reviewed clients or clients
  identified by a Client ID Metadata Document. This is feasible but is new work; it is not
  something today's secrets do.
- AWS's hosted server with SigV4: the local `mcp-proxy-for-aws` signs each `POST` with the AWS
  keys inside the computer. Silo can read the body but cannot swap a signing key. With AWS's
  OAuth option, the token is a bearer token again.

### Holding an MCP request

- A held `tools/call` is an ordinary `POST` that waits. The client sees a slow tool. Client tool
  timeouts may end the call before Silo's 90 s; this varies by client (unverified per client).
- A refusal as plain `403` may be misread. The MCP authorization spec uses `403` with
  `insufficient_scope` for a step-up challenge, and Cloudflare's server says clients then
  re-authorize. A client may also mark the server as failed. A refusal shaped as an HTTP `200`
  with a JSON-RPC error, or a tool result with `isError: true` carrying Silo's sentence, would
  reach the model as text it can act on. This is a case against the general "403 + a sentence"
  refusal for MCP hosts; I suggest MCP hosts get a JSON-RPC-shaped refusal with the same sentence.

## 3. Provider-run tools

Here the provider's servers make the call. Nothing crosses Silo's boundary except the model
request and response, which Silo already reads in full (Silo ends TLS on model traffic).

### Anthropic MCP connector (Messages API)

([MCP connector](https://platform.claude.com/docs/en/agents-and-tools/mcp-connector), beta headers
`mcp-client-2025-11-20` and `mcp-client-2026-09-15`)

The request declares the server and the tools:

```json
{
  "mcp_servers": [
    {"type": "url", "url": "https://mcp.supabase.com/mcp", "name": "supabase",
     "authorization_token": "<token>"}
  ],
  "tools": [
    {"type": "mcp_toolset", "mcp_server_name": "supabase",
     "default_config": {"enabled": true},
     "configs": {"delete_branch": {"enabled": false}}}
  ]
}
```

- `url` must be `https://`; Streamable HTTP and SSE are supported; stdio is not.
- `configs` per tool supports `enabled` and `defer_loading`. The docs recommend denylisting
  "write or destructive tools ... when you want a human confirmation step before state changes".
  I found no approval field: an enabled tool runs during the request.
- With `mcp-client-2026-09-15` an `mcp_toolset` may carry `tools`, a pinned copy of the tool list.
- The response carries `mcp_tool_use` (`name`, `server_name`, `input`) and `mcp_tool_result`. By
  the time Silo sees `mcp_tool_use`, the call has been made or is being made. Whether cutting the
  stream stops it is unverified.

What a Silo rule can do with facts from the model request:

- deny a model request that declares an `mcp_servers[].url` the user has not allowed;
- deny or hold a model request where a tool the user marked destructive for that server is
  enabled (by `default_config.enabled` not being `false` and no `configs.<tool>.enabled: false`);
- swap `authorization_token`: it is a field in a JSON body sent to `api.anthropic.com`, which Silo
  already reads. A placeholder there could be swapped for the real token on the way to Anthropic.
  This would be a new kind of swap (into a body, toward a model host, for use by a third host),
  and the real token then sits with Anthropic for the request.

Holding a model request is coarse: the ask cannot say "a yes lets delete_branch run", only "a yes
lets a model turn run that may call delete_branch". The record of the response then shows what was
called.

### OpenAI remote MCP tool (Responses API)

([connectors and MCP](https://developers.openai.com/api/docs/guides/tools-connectors-mcp))

- The tool entry has `type: "mcp"`, `server_label`, `server_url` (or `connector_id`, deprecated
  for models released after 2026-09-01), `authorization`, `allowed_tools`, `require_approval`.
- "By default, OpenAI will request your approval before any data is shared with a connector or
  remote MCP server." `require_approval` takes `"always"`, `"never"`, or an object listing tools
  that skip approval.
- When approval is needed the response carries an `mcp_approval_request` (id `mcpr_...`, tool
  `name`, `arguments`, `server_label`). The caller approves with a new request that includes an
  `mcp_approval_response` item (`approve: true`, `approval_request_id`), usually with
  `previous_response_id`. The call result appears as an `mcp_call` item.
- The `authorization` value is not stored and must be sent on every request.

This gives Silo a clean place to hold on facts. The approval answer is a model request that leaves
the computer. Its body names the approval request; the earlier response, which Silo recorded,
names the tool and its arguments. A rule can say: hold any Responses request carrying
`mcp_approval_response` with `approve: true` for tool X on server Y. The ask can show the exact
tool and arguments. A yes lets the approval reach OpenAI, and only then does OpenAI call the tool.
The agent itself is the "caller" that would otherwise approve, so without Silo this approval is
not a human step.

A rule should also deny requests that declare an `mcp` tool with `require_approval: "never"`, or
an object that skips approval for a tool the user marked destructive.

### Code execution and web fetch

- Anthropic's code execution container "has no internet access" and "No outbound network requests
  allowed" ([code execution](https://platform.claude.com/docs/en/agents-and-tools/tool-use/code-execution-tool)).
  It cannot delete anything outside itself.
- Anthropic's web fetch only retrieves content, only from URLs that already appeared in the
  conversation, and refuses URLs that seem to contain a credential not given by the user
  ([web fetch](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-fetch-tool)).
  Only an API that deletes on a `GET` with a credential in the URL could be hit, which is rare.
  Exfiltration is the larger risk there, and it is out of scope for this file.
- OpenAI's code interpreter network access: unverified.

### Hosted agents

- Cursor Cloud Agents (formerly Background Agents) run "in isolated VMs in the cloud" with
  internet access by default ([Cursor docs](https://cursor.com/docs/cloud-agents)). Their calls
  never cross Silo.
- ChatGPT and Claude.ai connectors call MCP servers from the provider's side. Stripe and AWS
  document installing their connectors there. Out of reach for Silo.
- Claude Code sessions that run on claude.ai rather than in a Silo computer: out of reach.
  Whether connectors attached to a Claude Code session in a Silo computer are called from the
  computer or through Anthropic's servers: unverified. If through Anthropic's servers, they fall
  under the MCP connector case above.

Silo can only help with these by keeping the credential out of them: a token the user only gives
to Silo is not available to a hosted agent.

## 4. Model traffic as a second place to see the call

Silo reads the model's tool calls. In the PocketOS case the model emitted a Bash tool call whose
command was the `curl -X POST https://backboard.railway.app/graphql/v2` with `volumeDelete`. Silo
could have read that text before the program ran it, and then read the actual request a moment
later.

| | At the tool call (model response) | At the network request (boundary) |
|---|---|---|
| When | Before the program runs anything | After the agent decided, before the provider acts |
| What | Text the model wrote: a command, a script, an MCP tool name and input | The exact bytes that reach the provider: host, method, path, body |
| Exactness | The command may be obfuscated (`base64 -d \| sh`), may run a script written earlier, may use variables set elsewhere, may call a binary whose traffic the text does not show (`railway volume delete`, `terraform destroy`) | Exact for every request Silo ends TLS on. Blind inside raw TCP, SSH, WireGuard |
| Coverage | Only tool calls of model providers Silo reads. A program the agent started earlier, a cron job or a git hook makes calls with no tool call | Every request from the computer |
| Can it hold? | Silo could delay delivering the model response, which pauses the agent | Yes, the request waits |

Recommendation:

- **The rule (fact) is the network request.** It is what actually reaches the provider, and a
  rule on it cannot be dodged by rewording a command.
- **The tool call is a precaution (opinion).** A parser or classifier reads the command text and,
  when it sees something like `volumeDelete`, `terraform destroy`, `DROP`, `db reset`, or an MCP
  tool marked destructive, it may pause or hold. It may only protect, never let anything through.
- The two work best together. The tool call gives context the network request lacks: the
  agent's stated reason, the command it ran, the file it read the token from. Silo can attach that
  context to the later network-level ask, labelled as what the agent wrote, not as a fact. When the
  precaution fired and no matching network request follows, that is worth recording too (the
  command may have gone through a channel Silo cannot read).
- One exception where model traffic is the fact: for provider-run tools (section 3), the model
  request is the only thing that leaves the computer before the call. There, the declared
  `mcp_servers`, `require_approval` and `mcp_approval_response` are facts about that request, and a
  rule may act on them.

## 5. Other non-REST channels

| Command | What leaves the computer | Silo can read the destructive part? | Notes |
|---|---|---|---|
| `heroku pg:reset DATABASE --confirm APP` | `PUT https://api.data.heroku.com/client/v11/databases/{id}/reset` with body `{extensions}` | Yes, REST | Source: [`src/commands/pg/reset.ts`](https://github.com/heroku/cli/blob/main/src/commands/pg/reset.ts), host from `HEROKU_DATA_HOST` or default `api.data.heroku.com` in [`heroku-cli-util`](https://github.com/heroku/heroku-cli-util/blob/main/src/utils/pg/host.ts). `--confirm` is local text, typed by the agent. Bearer token, so swappable. `heroku pg:psql` then talks Postgres directly (unverified) |
| `supabase db reset --remote` (`--linked` deprecated) and `supabase db push` | First, when no DB password is given, `POST https://api.supabase.com/v1/projects/{ref}/cli/login-role` to mint a temporary role; then a direct Postgres connection (direct host or pooler) | The setup call yes; the `DROP`s travel on the Postgres wire | [CLI reference](https://supabase.com/docs/reference/cli/supabase-db-reset); `apps/cli/src/commands/db/reset/reset.handler.ts` and `command-internal/db-config.layer.ts` (operation `v1-create-login-role`) in [supabase/cli](https://github.com/supabase/cli) (commit `ac6b3b6`). The prompt is skipped with `--yes`, which "agents must pass". The Postgres wire proxy of the other thread is the place to hold the `DROP`s. Holding the login-role call is a fact-based early hold for "a direct database session to project X is being opened" |
| `railway run <cmd>` | A Railway API call that fetches the service's variables; then whatever `<cmd>` does with them | The fetch yes; then raw TCP to the database's public proxy | [docs](https://docs.railway.com/cli/run). It puts the real `DATABASE_URL`, with its password, into the computer. That bypasses secret swapping entirely. A rule should hold or deny the read of variables. The exact GraphQL operation name is unverified |
| `railway connect` | Fetches `DATABASE_PUBLIC_URL` and similar, then runs `psql`, `mysql`, `redis-cli` or `mongosh` against the public TCP proxy, or tunnels over SSH | Postgres only, through the wire proxy; others opaque | [docs](https://docs.railway.com/cli/connect) |
| `fly ssh console`, `fly proxy`, `fly postgres connect` | A user-mode WireGuard tunnel run by `flyctl agent`, over UDP or over WebSockets on HTTPS (`fly wireguard websockets enable`) | No. Everything inside is opaque | [User-mode WireGuard](https://fly.io/blog/our-user-mode-wireguard-year/), [SSH and user-mode IP WireGuard](https://fly.io/blog/ssh-and-user-mode-ip-wireguard/), [WireGuard peers](https://docs.fly.io/blueprints/connect-private-network-wireguard). The tunnel set-up (peer creation, SSH certificate) goes through the Fly API first; the exact operations are unverified. Silo should deny the tunnel unless a rule allows it. Other `flyctl` commands (apps destroy, volumes destroy, machines) are API calls covered by the REST catalogue |
| `kubectl delete ns prod` | `DELETE https://<apiserver>/api/v1/namespaces/prod` | Yes, REST, if Silo can end TLS | [API concepts](https://kubernetes.io/docs/reference/using-api/api-concepts/). Watches are long `GET ?watch` streams; a hold does not touch them. Bearer tokens can be swapped. Client certificates (mutual TLS) cannot be swapped as-is: Silo would have to be the TLS client and hold the key, the same open question as other signing keys. Exec plugins (EKS `aws eks get-token`) mint the token inside the computer from AWS keys. `kubectl exec`, `attach`, `port-forward` upgrade to a stream channel (SPDY or WebSocket), opaque like SSH. The API server supports `dryRun=All`, which Silo could use to show what a delete would remove |
| `docker -H tcp://host:2376 volume rm data` | `DELETE /volumes/{name}` (also `DELETE /containers/{id}`, `POST /volumes/prune`) on the Engine API | Yes over TCP, but the TLS uses client certificates | [Protect the Docker daemon socket](https://docs.docker.com/engine/security/protect-access/). With `DOCKER_HOST=ssh://user@host` the same API travels inside SSH: opaque. The local unix socket never crosses the boundary |
| `terraform destroy`, `aws ...`, `kiro` on AWS | SigV4-signed AWS API calls | Readable, not swappable | Covered in the clouds file; signing keys are the open question |

Summary of section 5:

- **Reduce to a REST call Silo can read:** `heroku pg:reset`, most `flyctl` commands, `kubectl`
  object deletes, Docker Engine API over TCP, the Supabase Management API part of `db reset` and
  `db push`, the variables fetch of `railway run` and `railway connect`.
- **Go over a raw database or SSH-like channel:** `fly ssh`, `fly proxy`, `fly postgres connect`,
  `railway connect`, the program run by `railway run`, the SQL of `supabase db reset` and
  `db push`, `heroku pg:psql`, `kubectl exec`, Docker over `ssh://`.
- For the second group, Silo's facts are the setup call and the connection itself (host, port,
  bytes). The setup call is the better place to hold, because it names the project and comes
  before any data moves.

## What I could not verify

- Stripe, Vercel, Linear and AWS hosted servers: whether their tools carry MCP annotations.
- The argument names of Supabase's `delete_branch` on the hosted server, and of Stripe's
  `stripe_api_write`.
- AWS `run_script` and the exact IAM condition syntax for `aws:ViaAWSMCPService` and
  `aws:CalledViaAWSMCP`.
- Whether cutting an Anthropic MCP connector response stream stops a tool call that has started.
- Where Claude Code connectors from claude.ai are called from when Claude Code runs in a Silo
  computer.
- Where MCP clients store OAuth tokens, per client; client tool-call timeouts per client.
- The Railway GraphQL operation that `railway run` uses to fetch variables; the Fly API operations
  that set up WireGuard peers and SSH certificates.
- Whether `heroku pg:psql` connects directly with credentials fetched from the API.
- OpenAI code interpreter network access.
