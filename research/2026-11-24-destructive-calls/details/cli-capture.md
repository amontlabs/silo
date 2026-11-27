# What real CLIs send for destructive commands, and how they take a held refusal

Experiment run on 2026-11-24 in a Linux container, as root.
Every fact below was seen on the wire or on the CLI's output in this run.
It is not read from provider docs, so it carries no doc URL: the source of each fact is the capture file named next to it.
Anything not observed is marked "not tested" or "unverified".

## How the experiment worked

- A test CA signed one certificate for all API hosts. The CA went into the system store and into `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`, `AWS_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, `GIT_SSL_CAINFO`.
- `/etc/hosts` pointed each CLI's API hosts at its own loopback address (127.0.0.2 to 127.0.0.12), so the servers for different CLIs ran side by side.
- `proto/capture/server.py` listened on 443, logged each request as one JSON line (Authorization and every header whose name holds token, key, secret, cookie or session replaced by `<redacted>`), and answered from canned JSON so the CLI reached its destructive step.
- In hold mode a matching request waited N seconds, then got a refusal. Styles tried: 403 JSON `{"error": "<sentence>"}`, 403 `text/plain`, 403 in the provider's own error shape ("native"), for GraphQL a 200 with an `errors` array, and for git a 200 receive-pack answer that refuses each ref (`ng`).
- The sentence was always "Held by Silo and not answered: deleting volume X needs your yes."
- Every CLI ran with `env -i` (no session proxy, no inherited credentials) and fake tokens such as `test-token-123`.
- Timeouts were measured with a hold of 120 s (400 s for aws).

Files: `proto/capture/server.py` (server), `run.sh` (`ca` makes the test CA, `setup` and `teardown` change hosts and the system store), `clienv` (clean environment), `timed` (runs and times one command), `holds.sh` (the refusal-style matrix), `canned/<cli>.json` (answers), `captures/<cli>.jsonl` (requests), `outputs/<name>.txt` (what each CLI printed, its exit code and seconds).

Versions: aws-cli 2.42.2, gh 2.89.0, git 2.43.0, railway 5.75.0, vercel 66.6.0, flyctl 0.5.16, wrangler 4.168.0, stripe 1.55.14, supabase 3.3.0, neonctl 9.9.5, heroku 11.14.0.
All eleven installed, under `/opt/clis`, which was removed afterwards; the scripts expect it there. aws cli v1 (pip) installed but did not start in the clean environment and was not used.

## The table

"Method+path rule" means a rule that reads only method, host and path (with query).
"GraphQL-field rule" means a rule that reads the root field of the mutation from the body.
"Timeout" is how long the CLI waited on a held request before it gave up on its own; "none in 120 s" means it was still waiting when the hold ended.
"Retries on 403" counts repeats of the held request after a 403: zero for every CLI.
"What the user sees" is the best refusal style for that CLI, copied from its output.

| CLI | Command | Request seen | Method+path rule | GraphQL-field rule | Timeout | Retries on 403 | What the user (agent) sees, best style |
|---|---|---|---|---|---|---|---|
| git | `git push --force https://github.com/o/r.git main` | `GET github.com/o/r.git/info/refs?service=git-receive-pack`, then `POST /o/r.git/git-receive-pack`, body pkt-line `<old> <new> refs/heads/main\0 report-status-v2 side-band-64k ...` then PACK | No: every push is the same POST. Needs the pkt-lines. A force push looks like any update; only the commit graph tells it apart | n/a | none in 120 s | 0 | `! [remote rejected] main -> main (Held by Silo ...)` and `remote: Held by Silo ...` (200 + `ng` answer) |
| git | `git push origin --delete feature` | same POST, pkt-line `<old> 0000000000000000000000000000000000000000 refs/heads/feature` | No (same path); the zero id in the pkt-line says delete | n/a | not measured separately | 0 | `! [remote rejected] feature (Held by Silo ...)` |
| gh | `gh repo delete o/r --yes` | `DELETE api.github.com/repos/o/r`, no read first | Yes | n/a | none in 120 s | 0 | `HTTP 403: Held by Silo ... (https://api.github.com/repos/o/r)` with body `{"message": ...}` |
| gh | `gh api -X DELETE repos/o/r/git/refs/heads/feature` | `DELETE /repos/o/r/git/refs/heads/feature` | Yes | n/a | none in 120 s (same client) | 0 | body printed, then `gh: Held by Silo ... (HTTP 403)` |
| aws | `aws s3 rb s3://silo-test-bucket --force` | `GET silo-test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=&encoding-type=url`, then one `DELETE /<key>` per object on parallel connections, then `DELETE /` | Yes (DELETE), but one request per object | n/a | 60 s per attempt, 3 attempts | 0 | `delete failed: s3://.../backups/db-2026-11-23.dump An error occurred (AccessDenied) when calling the DeleteObject operation: Held by Silo ...` (S3 XML `<Error>`) |
| aws | `aws s3 rm s3://silo-test-bucket --recursive` | same list, then one `DELETE /<key>` per object | Yes, one per object | n/a | as above | 0 | as above |
| aws | `aws rds delete-db-instance --db-instance-identifier prod-db --skip-final-snapshot` | `POST rds.us-east-1.amazonaws.com/`, form body `Action=DeleteDBInstance&Version=2014-10-31&DBInstanceIdentifier=prod-db&SkipFinalSnapshot=true` | No: path is `/`; needs `Action` from the body | n/a | 60 s per attempt, 3 attempts, then `aws: [ERROR]: Read timeout on endpoint URL` after 181 s | 0 | `An error occurred (AccessDenied) when calling the DeleteDBInstance operation: Held by Silo ...` (query-protocol `<ErrorResponse>`) |
| aws | `aws dynamodb delete-table --table-name orders` | `POST dynamodb.us-east-1.amazonaws.com/`, `X-Amz-Target: DynamoDB_20120810.DeleteTable`, body `{"TableName": "orders"}` | No: path is `/`; needs the `X-Amz-Target` header | n/a | 60 s per attempt (same client, not measured separately) | 0 | `An error occurred (AccessDeniedException) when calling the DeleteTable operation: Held by Silo ...` (JSON with `__type`) |
| railway | `railway volume delete --volume vol-1 --yes` | all `POST backboard.railway.com/graphql/v2`: `ProjectToken`, `Project` (twice), `EnvironmentInstances`, then `mutation VolumeDelete($id: String!) { volumeDelete(volumeId: $id) }` | No: every call is the same POST | Yes: root field `volumeDelete` | 90 s | 0 | `Held by Silo ...` (GraphQL `errors` array, as 200 or 403) |
| railway | `railway environment delete staging --yes` | `ProjectToken`, `Project`, then `environmentDelete(id: $id)` | No | Yes: `environmentDelete` | 90 s (same client) | not tested | not tested; same client as above |
| railway | `railway down --yes` | `ProjectToken`, `Project` (twice), `Deployments`, then `deploymentRemove(id: $id)` | No | Yes: `deploymentRemove` | 90 s (same client) | not tested | not tested |
| vercel | `vercel remove my-app --yes` | `GET /v2/user`, `GET /v9/projects/my-app`, `GET /v13/deployments/my-app`, then `DELETE api.vercel.com/projects/prj_1` (no version, by id) | Yes, if the rule covers `/projects/<id>` without a version | n/a | none in 120 s (same client) | 0 | (same client as below) |
| vercel | `vercel project rm my-app --confirm-name my-app --scope acme` | `GET /v2/user`, `GET /teams/acme`, then `DELETE /v2/projects/my-app` (by name) | Yes, if the rule covers name and id and both path versions | n/a | none in 120 s; then `Error: Response Error (403)` with the generic JSON | 0 | `Error: Held by Silo ... (403)` with body `{"error": {"code": ..., "message": ...}}` |
| vercel | `vercel env rm DATABASE_URL --environment production --yes` | `GET /v2/user`, `GET /v9/projects/prj_1`, `GET /v10/projects/prj_1/env?target=production&source=vercel-cli%3Aenv%3Arm`, `GET /projects/prj_1/custom-environments`, then `DELETE /v10/projects/prj_1/env/env_1` | Yes | n/a | none in 120 s (same client) | not tested | not tested |
| fly | `fly apps destroy my-app --yes` | `GET api.machines.dev/v1/apps/my-app`, `GET api.fly.io/api/v1/organizations/acme`, GraphQL `ListOrganizationAddOns` on `api.fly.io/graphql`, then `DELETE api.machines.dev/v1/apps/my-app` | Yes | Not needed: the delete is REST | none in 120 s (same client) | not tested | (same client as below) |
| fly | `fly volumes destroy vol_123 -a my-app --yes` | `DELETE api.machines.dev/v1/apps/my-app/volumes/vol_123`, no read first | Yes | n/a | none in 120 s | 0 | `Error: failed destroying volume: failed to destroy volume vol_123: Held by Silo ...` (any JSON with `error`) |
| fly | `fly machine destroy 3d8d1 -a my-app --force` | `GET /v1/apps/my-app/machines/3d8d1`, `GET /v1/apps/my-app`, then `DELETE /v1/apps/my-app/machines/3d8d1?kill=true` | Yes | n/a | none in 120 s (same client) | not tested | not tested |
| wrangler | `wrangler d1 delete mydb -y` | `GET api.cloudflare.com/client/v4/accounts/acc123/d1/database/mydb?fields=uuid%2Cname`, then `DELETE /client/v4/accounts/acc123/d1/database/<uuid>` | Yes | n/a | none in 120 s | 0 | `✘ [ERROR] A request to the Cloudflare API (/accounts/acc123/d1/database/<uuid>) failed.` then `Held by Silo ...` (JSON `{"error": ...}`) |
| wrangler | `wrangler r2 bucket delete mybucket` | `DELETE /client/v4/accounts/acc123/r2/buckets/mybucket`, no read first | Yes | n/a | none in 120 s (same client) | not tested | not tested |
| wrangler | `wrangler delete --name my-worker --force` | `GET .../workers/durable_objects/namespaces`, `DELETE .../workers/services/my-worker?force=true`, then `GET .../storage/kv/namespaces` | Yes | n/a | none in 120 s (same client) | not tested | not tested |
| stripe | `stripe customers delete cus_123 --confirm` | `DELETE api.stripe.com/v1/customers/cus_123` | Yes | n/a | none in 120 s | 0 | the raw body, and **exit code 0** in every style |
| stripe | `stripe refunds create --charge ch_123 --confirm` | `POST /v1/refunds`, form body `charge=ch_123` | Yes (POST to a create path) | n/a | none in 120 s (same client) | not tested | same as above |
| supabase | `supabase projects delete <ref> --yes` | `DELETE api.supabase.com/v1/projects/<ref>`, then `GET /v1/projects/<ref>` | Yes | n/a | 60 s per attempt; it **sent the DELETE 6 times**, then `failed to delete project: request timed out` after 365 s | 0 | `Failed to delete project <ref>: Held by Silo ...` (text/plain; JSON bodies are printed raw) |
| neon | `neonctl projects delete proj-123` | `DELETE console.neon.tech/api/v2/projects/proj-123`, sent at the same time as `GET /api/v2/auth` and `GET /api/v2/users/me`; no confirmation prompt | Yes | n/a | 60 s, no retry; prints `ERROR: Request timed out` | 0 | `ERROR: Held by Silo ...` (JSON with `error` or `message`) |
| neon | `neonctl branches delete br-main --project-id proj-123` | `GET /api/v2/projects/proj-123/branches?limit=100`, then `DELETE /api/v2/projects/proj-123/branches/br-main-123` (by id) | Yes | n/a | 60 s (same client) | not tested | not tested |
| heroku | `heroku apps:destroy -a my-app --confirm my-app` | `GET api.heroku.com/apps/my-app`, then `DELETE /apps/my-app` | Yes | n/a | 60 s, no retry; prints `Error: Request timed out` | 0 | `Error: Held by Silo ...` and `Error ID: forbidden` (body `{"id": "forbidden", "message": ...}`) |

## What this means for Silo

### 1. A 90 s hold outlives five of the eleven clients

| Waits at least 120 s | Gives up before 90 s | Gives up at 90 s |
|---|---|---|
| git, gh, fly, wrangler, stripe, vercel | aws (60 s, then sends again, 3 attempts, 181 s), supabase (60 s, then sends again, 6 attempts, 365 s), neon (60 s), heroku (60 s) | railway (90.0 s, measured) |

- Railway's own timeout is exactly 90 s, the same as Silo's default hold. Whichever fires first wins. In the run the CLI won and printed `Failed to fetch: error sending request for url (https://backboard.railway.com/graphql/v2)` / `operation timed out`, so the agent never read Silo's sentence. The PocketOS deletion went through Railway.
- When the client gives up first, the agent sees a timeout, not Silo's sentence. It will likely try again or try another way.
- **Challenge to the 90 s default:** the hold length should not be one number for every host. A refusal sent at 55 s reaches aws, supabase, neon and heroku; one at 85 s reaches railway. A rule for a host could default to "the client's timeout minus a margin", taken from a table like the one above. Then a late yes from the phone (the other thread) is what lets the work go on, not the hold itself.

### 2. Some clients send the same delete again after a timeout

- aws sent `DeleteDBInstance` three times, 60 s apart. Each attempt carries the same `amz-sdk-invocation-id` header and `amz-sdk-request: attempt=N; max=3`. The `User-Agent` also names the command: `md/command#rds.delete-db-instance`.
- So Silo can tell a retry from a new request on AWS: same invocation id means the same ask. It should attach the retry to the open ask and not show the user a second one.
- supabase sent `DELETE /v1/projects/<ref>` again after every 60 s timeout, six times in all, with no header that ties the attempts together (headers are identical; capture `captures/supabase.jsonl`). Silo would have to match on method, path and body within a short window.
- If Silo answers a yes while a retry is in flight, the delete must pass once, not once per attempt. For a delete this is harmless (the second gets "not found"); for a refund (money) it is not. Unverified for stripe, which did not retry in this run.

### 3. On 403, no CLI retried; but what reaches the agent depends on the body shape

No client repeated the held request after a 403, in any style.
What it printed differs a lot:

| CLI | `{"error": sentence}` | `text/plain` sentence | Provider's own shape |
|---|---|---|---|
| gh | `HTTP 403 (url)`: sentence lost | `HTTP 403: 403 Forbidden`: lost (`gh api` prints the body) | `{"message": ...}`: shown |
| aws | rds: `Unable to parse response ... invalid XML received. Further retries may succeed:` then raw body; dynamodb: `An error occurred (403) ... :` with nothing after | rds: raw text after the parse error; dynamodb: shown | shown, with the error code: S3 `<Error><Code>AccessDenied</Code>`, RDS `<ErrorResponse><Error><Type>Sender</Type>...`, DynamoDB `{"__type": "AccessDeniedException", "message": ...}` |
| railway | `Failed to get data from GraphQL response`: lost | `error decoding response body`: lost | GraphQL `{"errors": [{"message": ...}]}`, as 200 or as 403: shown |
| vercel | `Response Error (403)`: lost | `Response Error (403)`: lost | `{"error": {"code": ..., "message": ...}}`: shown |
| fly | shown | shown, with `request returned non-2xx status: 403:` | same as `error` |
| wrangler | shown | shown, after `Received a malformed response from the API` | shown, but code 10000 makes wrangler diagnose the token: two more requests and "Please ensure it has the correct permissions" |
| stripe | raw body printed, exit 0 | sentence printed, exit 0 | raw body printed, exit 0 |
| neon | shown | `HTTP 403 Forbidden \| /api/v2/projects/proj-123`: lost | `{"message": ...}`: shown |
| supabase | raw body printed after `Failed to delete project <ref>:` | shown | raw body printed |
| heroku | raw object printed after `HTTP Error 403 for DELETE ...` | shown | `{"id": "forbidden", "message": ...}`: `Error: <sentence>` / `Error ID: forbidden` |
| git | `error: RPC failed; HTTP 403 curl 22 ...` then `Everything up-to-date`: lost and misleading | same | 200 receive-pack with `ng <ref> <sentence>`: `! [remote rejected] main -> main (<sentence>)` |

- **Finding:** one generic refusal body loses the sentence on gh, railway, vercel, git and half of aws. Silo needs a refusal shaped per host, in the provider's own error format. The table above is the list of shapes that worked.
- The shape is fixed per host and per API style, so this is a small table in Silo, not per-command code. For wrangler use `{"success": false, "errors": [{"message": ...}]}` without code 10000, so it does not blame the token. Which code wrangler treats as neutral is unverified.
- **Stripe exits 0 on a refusal.** An agent that checks the exit code reads the refusal as success. Silo cannot fix this from the boundary. It is worth saying in the ask and the record, and the sentence should start with words an agent will not mistake for a result.
- For git, the native answer is not a 403 at all. It is an HTTP 200 receive-pack result that refuses each ref. It needs the report-status format and side-band framing (`proto/capture/server.py`, `git_refusal`).

### 4. Rules on method and path miss the most dangerous calls

- Railway: every call, read or delete, is `POST /graphql/v2`. Only the root field (`volumeDelete`, `environmentDelete`, `deploymentRemove`) tells them apart. The CLI's binary holds about 196 operations; destructive ones besides these three include `serviceDelete`, `variableDelete`, `volumeInstanceBackupDelete`, `customDomainDelete`, `projectScheduleDelete`, `bucketCredentialsReset`, `sandboxDestroy`, `templateDelete` (read from the strings of the CLI binary, not called).
- The CLI always sends `operationName`, but it is chosen by the client. The PocketOS curl sent none. A rule must read the root field from the query text. Aliases (`x: volumeDelete(...)`) and several operations in one request would hide the field from a naive parser. Not tested here; it follows from the GraphQL syntax.
- AWS query and JSON protocols: the path is `/` for RDS and DynamoDB. The operation is in the body (`Action=DeleteDBInstance`) or in `X-Amz-Target`. SigV4 signs these, so Silo can read but not change them.
- git: all pushes are one POST. A delete shows as a zero new id. A force push cannot be told from a normal push by the commands alone: Silo would have to check whether the old commit is an ancestor of the new one, and the pack carries only new objects.
- git sends a probe first for a large push. When the pack exceeds `http.postBuffer` (1 MiB by default), the first POST carries only `0000`, and the real commands follow in a second, chunked POST. A rule that holds the first POST holds nothing; Silo must answer the probe and decide on the second (seen in `outputs/git-*-largepush-probe.txt`).
- Vercel deletes one project two ways: `DELETE /projects/<id>` (no version, from `vercel remove`) and `DELETE /v2/projects/<name>` (from `vercel project rm`). A rule written as `DELETE /v9/projects/*` misses both.

### 5. Many deletes are one request per object

- `aws s3 rm --recursive` and `aws s3 rb --force` send one `DELETE /<key>` per object, several at once, on parallel connections. They do not use the batch `POST /?delete`. A bucket of 10 000 objects is 10 000 held requests.
- Silo needs to group these into one ask ("delete every object under s3://bucket/", with a count), and a yes must cover the group, not one key.

### 6. Preliminary reads give Silo time and context, but not everywhere

- Most CLIs read before they delete: railway reads the project and the volume list, vercel reads the user and the project, fly reads the machine and the app, wrangler resolves the D1 name to an id, heroku reads the app.
- These reads carry what an ask needs in Silo's words: railway's `EnvironmentInstances` answer names the volume (`pg-data`), its mount path and size; wrangler's lookup maps `mydb` to its uuid; vercel's lookup maps `my-app` to `prj_1`. The ask can say "volume pg-data (900 MB, mounted at /var/lib/postgresql/data on service api)" instead of an id.
- Some go straight to the delete: `gh repo delete`, `fly volumes destroy`, `wrangler r2 bucket delete`, `stripe`, `supabase`, and `neonctl projects delete`, which sends the DELETE in parallel with its auth check and asks no confirmation.

### 7. HTTP/2 and other wire details

- gh, git and flyctl chose HTTP/2 (railway, vercel, wrangler, stripe, supabase, neon, heroku and aws did not) when the server offered it. Since Silo ends TLS on these hosts, it picks the protocol in ALPN. Offering only `http/1.1` worked for all eleven CLIs. That is one way to defer HTTP/2 support at the boundary.
- aws's parallel S3 connections, stripe and heroku offered no ALPN at all.
- No CLI compressed a request body. git sent one chunked body (the large push). No websockets or upgrades appeared in these commands.
- Railway reads its project token from the header `project-access-token`, not `Authorization`. Silo's secret swap and Silo's record redaction must cover that header. The capture server first missed it; it now redacts by header name.
- Clients describe themselves. Railway's telemetry mutation `cliEventTrack`, sent to the same GraphQL endpoint after each command, reports `caller: "claude_code"`, `isCi` and the error text. flyctl's User-Agent says `interactive=false; parent=shell`. aws names the command. Vercel prints messages for agents ("Once a person approves, run: ..."). These are facts Silo can show in an ask; they are claims by the client, so a rule should not grant on them.

## Not verified

- Behaviour against the real provider servers: every answer here was canned. Real servers may answer the preliminary reads differently, which could change the requests that follow.
- aws cli v1 and other SDKs (boto3, the JS SDK, terraform) were not run. Terraform uses the Go SDK, whose timeout and retry may differ from the CLI.
- Railway environment delete and down, vercel env rm, fly apps destroy and machine destroy, wrangler r2 and delete, neon branches delete and stripe refunds were captured, but their refusal output was not tested (their main command for the same client was).
- Whether `heroku` and `neonctl` 60 s timeouts can be raised by a flag or variable; aws can (`--cli-read-timeout`).
- GraphQL aliases and batched operations against a field rule (reasoning only).
