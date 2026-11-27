# Destructive calls: Cloudflare, Stripe, GitHub, Google Workspace, Slack

All sources were read on 2026-11-24 unless a line says otherwise. "Unverified" means I could not confirm it from an official page that day.
Outcomes use the brief's words: **gone**, **recoverable**, **outage**, **money**.

On all five hosts Silo already ends TLS, because it swaps the credential there. So it reads method, path, headers and body of every call below.

## What matters most for Silo

1. **The path is not enough on three of the five hosts.** GitHub GraphQL, Cloudflare D1 `/query`, Gmail `batchModify` and `git-receive-pack` all put the destructive part in the body. A rule on method and path alone sees `POST` to one endpoint for a read and for a wipe.
2. **The same destruction has several wire forms.** Example: `wrangler delete` sends `DELETE /accounts/{a}/workers/services/{name}?force=…`, not the documented `DELETE /accounts/{a}/workers/scripts/{name}`. KV bulk delete exists both as `POST …/bulk/delete` (docs) and `DELETE …/bulk` with a JSON body (what wrangler 4.168.0 sends). A rule list must name every form, or the facts must be one level above the path ("deletes a KV key").
3. **Silo can make several of these undoable before it lets them pass, with one cheap call of its own:** a GitHub ref before a force push or branch delete, a D1 Time Travel bookmark before a SQL write, a Cloudflare DNS export before a DNS change. Nothing comparable exists for R2, Stripe money calls, a GitHub repo visibility change or a Gmail permanent delete.
4. **Stripe has its own hold.** Agent-tagged restricted keys come with approval rules. A refund, a subscription cancel or a payout made with that key returns `approval_required`, and a person approves it in the Stripe app, for up to 14 days. That is the same idea as Silo's hold, built by the provider and with a longer window. Silo can recommend it and recognise the error.
5. **Live or test is visible on the wire for Stripe** (`sk_live_`, `rk_live_` vs `sk_test_`, `rk_test_`). Silo holds the real key, so it knows which mode a request is in without reading the body. A rule can hold live writes and allow sandbox ones.

---

## Cloudflare

Host: `api.cloudflare.com/client/v4`. R2 also speaks S3 at `https://<ACCOUNT_ID>.r2.cloudflarestorage.com` (region `auto`). Source: https://developers.cloudflare.com/r2/api/s3/api/

### Token scoping

API tokens carry permission groups, each Read or Edit (Write in the API). There is no separate Delete permission for any group I checked: DNS, Zone, Workers Scripts, Workers KV Storage, Workers R2 Storage, D1, Cloudflare Pages, API Tokens, Account Settings. Pages Edit explicitly covers deleting projects. Source: https://developers.cloudflare.com/fundamentals/api/reference/permissions/
Tokens can be limited to named accounts or zones, to client IPs, and given an expiry. Source: https://developers.cloudflare.com/fundamentals/api/get-started/create-token/
So a user can make a token that cannot touch DNS, but not one that can edit DNS records and not delete them. Silo's method and path rules are the only place to make that cut.

### Catalogue

| Operation | Outcome | Wire | Emitted by | Silo pre-step | Source |
|---|---|---|---|---|---|
| Delete zone | **outage**, and DNS records, page rules and settings are lost (zone can be re-added, records only if exported) | `DELETE /zones/{zone_id}` | dashboard, Terraform `cloudflare_zone` destroy | `GET /zones/{zone_id}/dns_records/export` (BIND file) before passing. Settings are not covered by that export | https://developers.cloudflare.com/api/resources/zones/methods/delete/ (recovery of a deleted zone: unverified, docs say nothing) |
| Delete DNS record | **outage** | `DELETE /zones/{zone_id}/dns_records/{id}` | `flarectl`, Terraform, SDKs | read the record (`GET …/dns_records/{id}`) and keep it; re-create with `POST` | https://developers.cloudflare.com/api/resources/dns/subresources/records/ |
| Overwrite DNS record | **outage** (omitted fields reset to defaults) | `PUT /zones/{zone_id}/dns_records/{id}` | same | same | same |
| Batch DNS change | **outage**, many records at once | `POST /zones/{zone_id}/dns_records/batch`, body keys `deletes`, `patches`, `puts`, `posts`, run in that order, one transaction | Terraform, SDKs | read the body: a non-empty `deletes` or `puts` is a destructive fact. Export first | https://developers.cloudflare.com/dns/manage-dns-records/how-to/batch-record-changes/ |
| DNS import | can add many records | `POST /zones/{zone_id}/dns_records/import` | | | records page above |
| Delete Worker | **outage** | documented: `DELETE /accounts/{a}/workers/scripts/{name}`. `wrangler delete` sends `DELETE /accounts/{a}/workers/services/{name}?force=true\|false` | wrangler 4.168.0 (read in its bundled `cli.js`) | `GET …/workers/scripts/{name}` downloads the script; versions endpoint `…/workers/scripts/{name}/versions` exists. Bindings and secrets are not in the download (unverified) | https://developers.cloudflare.com/api/resources/workers/subresources/scripts/ |
| Delete Worker secret | **outage** | `DELETE /accounts/{a}/workers/scripts/{name}/secrets/{key}` | `wrangler secret delete` | none: secret values cannot be read back | wrangler `cli.js` |
| Delete R2 object | **gone** | API: `DELETE /accounts/{a}/r2/buckets/{bucket}/objects/{key}` (wrangler). S3: `DELETE /{bucket}/{key}` and `POST /{bucket}?delete` (DeleteObjects, up to 1000 keys) on `<account>.r2.cloudflarestorage.com` | `wrangler r2 object delete`, `aws s3 rm`, `rclone` | `GET` the object and keep a copy. Only cheap for small objects | S3 table above; Workers API says up to 1000 keys per delete: https://developers.cloudflare.com/r2/api/workers/workers-api-reference/ |
| Delete R2 bucket | **gone** (bucket must be empty first, so the loss happened at the object deletes) | API: `DELETE /accounts/{a}/r2/buckets/{bucket}`. S3: `DELETE /{bucket}` | `wrangler r2 bucket delete` | none useful | S3 table above |
| R2 recovery | **none**: `PutBucketVersioning` and `GetBucketVersioning` are not implemented. No trash or undelete is documented | | | **Bucket locks** stop deletion and overwrite for a period or forever, and a bucket cannot be emptied while lock rules exist. The user can set one before giving the agent a token. Removing the lock is itself a call to hold: `PUT/DELETE /accounts/{a}/r2/buckets/{bucket}/lock` (path from wrangler) | https://developers.cloudflare.com/r2/buckets/bucket-locks/ |
| Delete D1 database | **recoverable** for 7 days | `DELETE /accounts/{a}/d1/database/{id}` | `wrangler d1 delete` | Silo can `POST …/d1/database/{id}/export` first. Undo: `POST /accounts/{a}/d1/database/{id}/undelete` within 7×24 h, optional `new_name`. After undelete, Time Travel cannot go back to before the undelete | https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/undelete/ |
| D1 SQL | **recoverable** (Time Travel: 30 days on Workers Paid, 7 on Free) | `POST /accounts/{a}/d1/database/{id}/query` with `{"sql": "..."}`, and `POST …/raw`, `POST …/import` | `wrangler d1 execute --remote` (sends `query`), `wrangler d1 migrations apply`, Drizzle/Prisma D1 HTTP drivers | **Yes**: `GET …/d1/database/{id}/time_travel/bookmark` returns the current bookmark. Silo stores it with the record. Undo is `POST …/time_travel/restore?bookmark=…`, which overwrites the database in place and returns a bookmark to undo the restore | https://developers.cloudflare.com/d1/reference/time-travel/ and https://developers.cloudflare.com/api/resources/d1/subresources/database/ |
| Delete KV namespace | **gone** (no undelete documented) | `DELETE /accounts/{a}/storage/kv/namespaces/{ns}` | `wrangler kv namespace delete` | listing and reading every key is possible but slow | https://developers.cloudflare.com/api/resources/kv/subresources/namespaces/ |
| KV bulk delete | **gone** | docs: `POST /accounts/{a}/storage/kv/namespaces/{ns}/bulk/delete`. wrangler 4.168.0: `DELETE …/bulk` with a JSON array of keys. Single: `DELETE …/values/{key}` | `wrangler kv bulk delete`, `wrangler kv key delete` | `POST …/bulk/get` (wrangler uses it) can read the values first | same; wrangler `cli.js` |
| KV bulk put | **gone** for overwritten values | `PUT …/bulk` | `wrangler kv bulk put` | same | same |
| Delete Pages project | **outage** (site down; deployments lost, unverified) | `DELETE /accounts/{a}/pages/projects/{name}` | `wrangler pages project delete` | none | wrangler `cli.js`; permissions page |
| Delete Pages deployment | **outage** if it is the live one | `DELETE …/pages/projects/{name}/deployments/{id}?force=…` | `wrangler pages deployment delete` | none | wrangler `cli.js` |
| Delete account API token | **outage** for whatever used it | `DELETE /accounts/{a}/tokens/{token_id}` (needs "Account API Tokens Write"); user tokens `DELETE /user/tokens/{id}` (unverified) | dashboard, SDKs | none | https://developers.cloudflare.com/api/resources/accounts/subresources/tokens/methods/delete/ |
| Create API token | widens access | `POST /accounts/{a}/tokens`, `POST /user/tokens` (unverified) | | hold always | |

### Cloudflare facts Silo can derive

- Destructive by path: any `DELETE` under `/zones/` or `/accounts/{a}/(d1|r2|storage/kv|workers|pages)/`.
- Destructive by body: `POST …/d1/database/{id}/(query|raw|import)` whose SQL is not a pure read. Silo can reuse the statement classifier of the Postgres proxy thread (D1 is SQLite: `DROP`, `DELETE`, `UPDATE`, `ALTER`, `INSERT OR REPLACE`). `POST …/dns_records/batch` with non-empty `deletes` or `puts`. S3 `POST /{bucket}?delete`.
- A D1 write is the one case on this page where Silo can offer "allow, I kept a bookmark" as the default.

---

## Stripe

Host: `api.stripe.com`. Auth is `Authorization: Bearer <key>` or Basic with the key as user.

### Live or test, and key scoping

- Prefixes: secret `sk_live_` / `sk_test_`, restricted `rk_live_` / `rk_test_`, publishable `pk_…`, organization `sk_org_…`. Source: https://docs.stripe.com/keys
- Restricted keys give each resource **None, Read or Write**. Write implies Read. `GET` is read, `POST` and `DELETE` are write. There is no separate delete permission: a key that can update customers can delete them. Connected-account access is a separate setting. Source: https://docs.stripe.com/keys/restricted-api-keys
- Access policies limit a key to IP ranges, ASNs, countries, and can block VPNs and Tor. Source: https://docs.stripe.com/keys
- **Approval rules.** A restricted key can be tagged as an agent key. Stripe then applies default approval rules to: refund created, subscription canceled, outbound payment, outbound transfer, payout created, currency conversion. The call returns an `approval_required` error and creates an approval request. A reviewer approves in the Dashboard or the mobile app. Approved actions complete automatically. Requests expire after 14 days. Rules can also cover payment intent created, credit note, customer balance transaction, bank account changes, admin invites, for any actor. Source: https://docs.stripe.com/account/approvals
- Silo consequence: a rule can say "for this secret, hold every non-GET when the key is live". Silo knows the mode from the real key it swaps in. With a restricted key, Stripe already refuses what the key lacks. With an agent key, Stripe's own approval covers the money calls, with a window much longer than Silo's 90 s.

### Idempotency

`Idempotency-Key` header on `POST`. Stripe keeps the first result for at least 24 hours and replays it on retry with the same key and the same parameters. `GET` and `DELETE` ignore it. Source: https://docs.stripe.com/api/idempotent_requests
Silo consequence: if a held refund times out and the agent retries, the retry carries the same key only if the SDK reuses it. Official SDKs generate one per call and reuse it on their own network retries (unverified for each SDK). A late approval that replays the original request with its original key cannot double the refund within 24 hours.

### Catalogue

Paths not marked otherwise were read on the linked page. Paths marked "(path not re-read)" are the standard v1 paths; I did not open their page on 2026-11-24.

| Operation | Outcome | Wire | Silo pre-step | Source |
|---|---|---|---|---|
| Create refund | **money** | `POST /v1/refunds` (path not re-read) | none. Stripe approval rule covers it | approvals page |
| Create payout | **money** | `POST /v1/payouts` (path not re-read) | none. Approval rule for agent keys only | approvals page |
| Create transfer (Connect) | **money** | `POST /v1/transfers` (path not re-read) | none. Reversal exists: `POST /v1/transfers/{id}/reversals` (unverified) | |
| Create or confirm payment intent, create charge | **money** (charges a customer) | `POST /v1/payment_intents`, `POST /v1/payment_intents/{id}/confirm`, `POST /v1/charges` (paths not re-read) | none. Approval rule can cover "payment intent created" | approvals page |
| Create invoice and pay or send it | **money** and reaches people (emails the customer) | `POST /v1/invoices/{id}/pay`, `POST /v1/invoices/{id}/send` (unverified) | none | |
| Delete customer | **gone** for card details and future use; the customer stays retrievable with `deleted: true`. Cancels all active subscriptions at once | `DELETE /v1/customers/{id}` | `GET /v1/customers/{id}` and its subscriptions; card data cannot be re-created | https://docs.stripe.com/api/customers/delete |
| Cancel subscription | **money** lost (customer is not billed again); the subscription cannot be reactivated, only re-created | `DELETE /v1/subscriptions/{id}` | `GET` it so it can be re-created; the customer may need to confirm a new payment | https://docs.stripe.com/api/subscriptions/cancel |
| Delete product | **gone**, but only possible when the product has no prices, so the blast radius is small | `DELETE /v1/products/{id}` | `GET` and re-create | https://docs.stripe.com/api/products/delete |
| Prices | cannot be deleted through the API, only archived with `active=false` (unverified) | `POST /v1/prices/{id}` | | |
| Delete coupon | **gone** for new redemptions; existing discounts keep working (unverified) | `DELETE /v1/coupons/{id}` (path not re-read) | `GET` and re-create with the same id | |
| Delete webhook endpoint | **outage**: events stop reaching the user's server; re-creating it gives a new signing secret | `DELETE /v1/webhook_endpoints/{id}` (path not re-read) | `GET` it, but the secret changes | |
| Delete connected account | **gone**. Test-mode accounts at any time. Live accounts only with zero balances, and never Standard accounts | `DELETE /v1/accounts/{id}` | none | https://docs.stripe.com/api/accounts/delete |
| Act on a connected account | any of the above, on someone else's account | header `Stripe-Account: acct_…` | the header is a fact a rule can read | |

Rule shape that follows: deny `DELETE` on `/v1/accounts/*` and `/v1/customers/*`; hold any non-GET when the key is live; let Stripe's approval handle refunds and payouts for agent keys.

---

## GitHub

Hosts: `api.github.com` (REST and `POST /graphql`), `github.com` (git smart HTTP), `uploads.github.com` (release assets).
Another thread covers a token limited to named repos. This section covers what a token can do inside those repos.

### Token scoping (fine-grained PATs)

Source: https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens

| Endpoint | Permission |
|---|---|
| `DELETE /repos/{o}/{r}` | Administration write |
| `PATCH /repos/{o}/{r}` (includes visibility) | Administration write |
| `DELETE /repos/{o}/{r}/git/refs/{ref}` | Contents write |
| `PATCH /repos/{o}/{r}/git/refs/{ref}` | Contents write (+ Workflows write) |
| `DELETE /repos/{o}/{r}/branches/{b}/protection` | Administration write |
| `DELETE /repos/{o}/{r}/releases/{id}` | Contents write |
| `DELETE …/actions/runs/{id}`, `…/actions/artifacts/{id}` | Actions write |
| `DELETE …/actions/secrets/{name}` | Secrets write |
| `DELETE /repos/{o}/{r}/environments/{name}` | Administration write |
| `DELETE /repos/{o}/{r}/hooks/{id}` | Webhooks write |
| `DELETE /orgs/{org}/members/{user}` | Members write (organization) |

A token without Administration cannot delete the repo or change its visibility. **Contents write cannot be split**: any token that can push can delete branches, delete tags, force-push and delete releases. That is the gap Silo fills. Classic PATs and OAuth tokens need the `delete_repo` scope to delete a repo. Source: https://docs.github.com/en/rest/repos/repos#delete-a-repository

### REST catalogue

| Operation | Outcome | Wire | Silo pre-step | Source |
|---|---|---|---|---|
| Delete repo | **recoverable** for 90 days, self-service for user and org repos. Not self-service if the repo is in a non-empty fork network (then GitHub Support, paid plans only). Team permissions are not restored. A deleted repo can take up to an hour to become restorable | `DELETE /repos/{o}/{r}` | `gh repo delete`, Terraform `github_repository` | none needed for content; record team permissions | https://docs.github.com/en/repositories/creating-and-managing-repositories/restoring-a-deleted-repository |
| Make repo public | **gone** in the sense that matters: the code, Actions history and logs are public and can be forked. Stars and watchers are erased, push rulesets are disabled. Switching back does not un-publish what was copied | `PATCH /repos/{o}/{r}` with `"visibility": "public"` or `"private": false` | `gh repo edit --visibility public` | none. Body field is the fact. Hold always | https://docs.github.com/en/rest/repos/repos#update-a-repository and https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility |
| Rename or transfer repo | **outage** (redirects exist but clones and Actions may break, unverified) | `PATCH /repos/{o}/{r}` with `name`; `POST /repos/{o}/{r}/transfer` (unverified) | `gh repo rename` | | |
| Archive repo | **outage** (read-only, reversible) | `PATCH /repos/{o}/{r}` with `archived: true` | | | |
| Delete branch or tag | **recoverable**: commits stay until garbage collection; the old sha is in `GET /repos/{o}/{r}/activity?activity_type=branch_deletion`. A PR head branch can be restored from the closed PR | `DELETE /repos/{o}/{r}/git/refs/{ref}` | `gh api`, `git push --delete` (see git below) | **Yes**: `POST /repos/{o}/{r}/git/refs` with `{"ref": "refs/silo/<id>", "sha": "<old>"}` keeps the commits reachable. Creating a ref under `refs/silo/` should be accepted (docs require `refs/` and two slashes; unverified for custom namespaces) | https://docs.github.com/en/rest/git/refs ; https://docs.github.com/en/rest/repos/repos#list-repository-activities ; https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-branches-in-your-repository/deleting-and-restoring-branches-in-a-pull-request |
| Force-move a ref | **recoverable** like a branch delete; `activity_type=force_push` lists them | `PATCH /repos/{o}/{r}/git/refs/{ref}` with `"force": true` | | same backup ref | same |
| Remove branch protection | **outage** (protection off, nothing lost); it is the step before a force push | `DELETE /repos/{o}/{r}/branches/{b}/protection` | `gh api` | `GET …/branches/{b}/protection` and keep the JSON so it can be put back | permissions page |
| Delete ruleset | **outage** | `DELETE /repos/{o}/{r}/rulesets/{id}` (unverified path) | | `GET` and keep | |
| Delete release | **gone** for release notes and uploaded assets (tag stays) | `DELETE /repos/{o}/{r}/releases/{id}` | `gh release delete` | `GET` the release and download assets first, possible but heavy | permissions page |
| Delete workflow runs, artifacts, logs | **gone** (audit trail lost) | `DELETE …/actions/runs/{id}`, `…/actions/runs/{id}/logs`, `…/actions/artifacts/{id}` | `gh run delete` | none | permissions page |
| Delete Actions secret or environment | **outage** (values cannot be read back) | `DELETE …/actions/secrets/{name}`, `DELETE …/environments/{name}` | `gh secret delete` | none for values | permissions page |
| Delete webhook | **outage** | `DELETE /repos/{o}/{r}/hooks/{id}` | | `GET` and keep (the secret is not returned, unverified) | permissions page |
| Remove org member | **outage** for that person | `DELETE /orgs/{org}/members/{user}` | `gh api` | none | permissions page |

### GraphQL

`POST https://api.github.com/graphql`. The operation is in the body. Silo has to parse the document to know the root fields (the brief's open recommendation already keeps the operation name; the root field list is the fact a rule needs, because the operation name is chosen by the client).

I parsed GitHub's public schema (`https://docs.github.com/public/fpt/schema.docs.graphql`, sha256 in the JSON). Script: `../proto/github_graphql_destructive.py`. Output: `../proto/github_graphql_destructive.json`.

- 285 mutations in total. 88 selected: 30 `delete*`, 19 `remove*`/`unlink*`, 4 revoke/regenerate, 5 cancel/abort, 3 archive, 3 close, 4 dismiss/reject, 2 merge, 2 transfer, 2 clear, 1 disable, plus 12 watched `update*`.
- **There is no `deleteRepository` mutation**, and `updateRepository`'s input has no visibility field. Deleting a repo or making it public is REST only. Archiving is in GraphQL (`archiveRepository`).
- Ref destruction in GraphQL: `deleteRef`, `updateRef` (`force: true`), and `updateRefs`, which takes a list of `RefUpdate {name, beforeOid, afterOid, force}`. An `afterOid` of forty zeros deletes the ref. One call can delete or force-move many refs atomically.
- Protection removal: `deleteBranchProtectionRule`, `deleteRepositoryRuleset`, and `updateBranchProtectionRule` with `allowsForcePushes` or `allowsDeletions` set to true.
- Data loss outside git: `deleteIssue`, `deleteDiscussion`, `deleteProjectV2`, `deletePackageVersion`, `deleteEnvironment`, `deleteDeployment`.
- People: `removeOutsideCollaborator`, `removeEnterpriseMember`, `removeEnterpriseAdmin`, `removeEnterpriseOrganization`, `removeEnterpriseIdentityProvider`.

A rule for GraphQL reads: root field names in the mutation, and for `updateRefs`/`updateRef`/`updateBranchProtectionRule`, a few input fields. Variables arrive in a separate JSON field; Silo must substitute them before reading inputs.

### git smart HTTP

A push is `POST https://github.com/{o}/{r}.git/git-receive-pack`. The body starts with pkt-lines `<old-id> <new-id> <refname>`, a flush, then a pack. Source: https://git-scm.com/docs/gitprotocol-pack

- **Delete** (`git push --delete`, `git push origin :branch`): new-id is forty zeros. The server must advertise `delete-refs`. A delete-only push sends no pack. Silo sees it from the command lines alone.
- **Create**: old-id is zeros.
- **Force push**: a line with two non-zero ids looks the same for a fast-forward and a rewrite. Silo knows `old` (it is on the line). It must find out whether `old` is an ancestor of `new`. Two ways:
  1. **Read the pack.** Walk parents from `new` through the commits in the pack. If the walk reaches `old`, it is a fast-forward. If it leaves the pack at a commit the server already has, ask GitHub `GET /repos/{o}/{r}/compare/{old}...{that-commit}`: status `ahead` or `identical` means fast-forward, `diverged` or `behind` means a rewrite. Source: https://docs.github.com/en/rest/commits/commits#compare-two-commits
  2. **Rewind to an existing commit** (`git push -f origin HEAD~3:main`): the pack is empty and `new` is already on GitHub, so `compare/{old}...{new}` answers directly (`behind`).
  The pack can be large; Silo only needs commit objects, which are near the start of a typical pack but can be deltified (unverified how often). A simpler fallback: treat any update of a protected ref as a force push unless proven otherwise.
- **Make it undoable before passing**: create `refs/silo/<id>` at `old` through `POST /repos/{o}/{r}/git/refs` using the same swapped token, then let the push through. A force push or delete then loses nothing. Cleaning those refs up later is Silo's job.
- Recovery without Silo: the activity API lists `force_push` and `branch_deletion` with before and after shas, and GitHub keeps unreachable commits until it garbage-collects them. The retention period is not documented (unverified).
- `push-options` and `atomic` exist; a push can carry many ref commands at once.

---

## Google Workspace

Hosts: `gmail.googleapis.com`, `www.googleapis.com` (Drive v3, Calendar v3). OAuth bearer tokens. A refresh token is a swappable secret; the access token it yields is short-lived (Silo would need to swap at the token endpoint, out of scope here).

### Gmail

| Operation | Outcome | Wire | Scope | Source |
|---|---|---|---|---|
| Delete message | **gone** for consumer Gmail ("cannot be undone"). Workspace: admin restore, below | `DELETE /gmail/v1/users/{u}/messages/{id}` | only `https://mail.google.com/` | https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/delete |
| Batch delete | **gone**, many at once | `POST /gmail/v1/users/{u}/messages/batchDelete`, body `{"ids": [...]}` | only `https://mail.google.com/` | https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/batchDelete |
| Delete thread | **gone** | `DELETE /gmail/v1/users/{u}/threads/{id}` (path not re-read) | `https://mail.google.com/` (unverified) | |
| Trash | **recoverable** 30 days from Trash | `POST …/messages/{id}/trash`, `POST …/threads/{id}/trash`, **and** `POST …/messages/batchModify` with `addLabelIds: ["TRASH"]` | `gmail.modify` | scopes page |
| Delete draft | **gone** (drafts do not go to Trash) | `DELETE …/drafts/{id}` | `gmail.compose`/`gmail.modify` (unverified) | gogcli source comment, see below |
| Create filter | can forward, trash or delete future mail silently | `POST /gmail/v1/users/{u}/settings/filters` | `gmail.settings.basic` | https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.settings.filters/create |
| Add forwarding address | exfiltration | `POST …/settings/forwardingAddresses` | `gmail.settings.sharing`, and only service accounts with domain-wide delegation | https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.settings.forwardingAddresses/create |
| Send | **money** (reaches people) | `POST …/messages/send` | `gmail.send` | scopes page |

Scopes: `gmail.modify` "does not allow immediate, permanent deletion of threads and messages, bypassing the trash". Only `https://mail.google.com/` does. Source: https://developers.google.com/workspace/gmail/api/auth/scopes
So the user's best defence is to never grant `https://mail.google.com/`. Silo can see the granted scope only at the OAuth exchange, not on each call.

Workspace admin restore: deleted mail stays in Trash 30 days; an admin can then restore mail permanently deleted in the last 25 days, by date range, from the Admin console. Not restored: Trash, spam, drafts, labels. Consumer accounts have no such restore. Source: https://knowledge.workspace.google.com/admin/support/troubleshooting/restore-a-users-permanently-deleted-email (search result summary; I did not open the page itself). Whether it covers mail removed by the API's `messages.delete`: unverified.

### OpenClaw inbox (Feb 2026)

- The agent's plan was "trash EVERYTHING in inbox older than Feb 15 that isn't already in my keep list". The screenshot shows it running `gog gmail search 'in:inbox' --max 20 -a` in a loop. Source: https://simonwillison.net/2026/feb/23/summer-yue (quoting Summer Yue's post). The AI Incident Database (incident 1542) does not name the API: https://incidentdatabase.ai/cite/1542/
- `gog` is gogcli. In its current source (commit 31c4e14, 2026-11-21), `gog gmail trash` calls `users.messages.batchModify` with `addLabelIds: ["TRASH"]`, `removeLabelIds: ["INBOX"]`, i.e. `POST /gmail/v1/users/me/messages/batchModify`. `gog gmail batch delete` calls `batchDelete` and needs `https://mail.google.com/`. Whether the February version did the same: unverified.
- Conclusion for Silo: the incident was almost certainly a loop of `POST …/messages/batchModify` with a TRASH label. On the path alone that looks like labelling mail. The fact a rule needs is "adds TRASH to N messages". It was recoverable from Trash for 30 days; reports do not say whether it was recovered.
- A count limit fits this case better than a per-call hold: twenty calls of 20 messages each. A rule like "hold the second trash call in a minute" would have stopped it after the first page.

### Drive

| Operation | Outcome | Wire | Scope | Source |
|---|---|---|---|---|
| Delete file | **gone** for consumer accounts: skips Trash. A folder takes all descendants the user owns. Workspace: admin restore for 25 days | `DELETE https://www.googleapis.com/drive/v3/files/{id}` | `drive`, `drive.file`, `drive.appdata` | https://developers.google.com/workspace/drive/api/reference/rest/v3/files/delete |
| Empty trash | **gone** (same admin window) | `DELETE https://www.googleapis.com/drive/v3/files/trash` | `drive` | https://developers.google.com/workspace/drive/api/reference/rest/v3/files/emptyTrash |
| Trash file | **recoverable** 30 days | `PATCH /drive/v3/files/{id}` with `{"trashed": true}` | | |
| Admin restore | Workspace only: files emptied from Trash in the last 25 days, by date range, users must reshare | | | https://knowledge.workspace.google.com/admin/drive/recover-deleted-files-and-folders-for-drive-users |

Silo pre-step: for `files.delete`, Silo could rewrite it as `PATCH trashed=true`. That changes the request, so it is a grant, not a protection; it would need a rule the user wrote, not a precaution. Downloading the file first works for small files only.

### Calendar

Delete event: `DELETE https://www.googleapis.com/calendar/v3/calendars/{calendarId}/events/{eventId}`, with `sendUpdates=all|externalOnly|none`. With `all` it emails every guest: **money** (reaches people) as well as loss. Scopes: `calendar`, `calendar.events`, `calendar.app.created`, `calendar.events.owned`. Recovery of a deleted event: unverified. Source: https://developers.google.com/workspace/calendar/api/v3/reference/events/delete

---

## Slack

Host `slack.com/api/`. All methods are `POST`; the method name is the path, so path rules work.

| Operation | Outcome | Wire | Scope | Source |
|---|---|---|---|---|
| Delete message | **gone** (no recovery documented) | `POST /api/chat.delete` | `chat:write`; a user token deletes only what that user could delete in Slack | https://docs.slack.dev/reference/methods/chat.delete |
| Archive channel | **outage** (unarchive exists as `conversations.unarchive`, unverified) | `POST /api/conversations.archive` | bot `channels:manage` and others; user `channels:write`, `groups:write`, `im:write`, `mpim:write` | https://docs.slack.dev/reference/methods/conversations.archive |
| Delete channel (admin) | **gone** (permanence not stated) | `POST /api/admin.conversations.delete` | `admin.conversations:write`, Enterprise plan only | https://docs.slack.dev/reference/methods/admin.conversations.delete |
| Delete file | **gone** (unverified) | `POST /api/files.delete` | | |
| Post message | **money** (reaches people) | `POST /api/chat.postMessage` | `chat:write` | |

Rule shape: deny `admin.*`; hold `chat.delete`, `files.delete`, `conversations.archive`. Slack scopes cannot separate posting from deleting (`chat:write` covers both).

---

## Not verified

- Recovery of a deleted Cloudflare zone, KV namespace, Pages project. Docs say nothing.
- Exact Stripe paths for refunds, payouts, transfers, coupons, webhook endpoints (standard v1 paths, not re-read). That prices cannot be deleted.
- Whether GitHub accepts refs under a custom `refs/silo/` namespace through the REST API. How long GitHub keeps unreachable commits.
- The exact Gmail call OpenClaw made in February 2026 (inferred from today's gogcli).
- Whether Workspace admin restore covers mail removed by `messages.delete`.
- Slack unarchive and file delete recovery.
