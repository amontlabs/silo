# Destructive calls on hosting control planes (PaaS)

Railway, Vercel, Fly.io, Heroku, Render, Netlify.
Everything here was read on 2026-11-24 unless a line says otherwise.
Outcomes use the brief's words: **gone**, **recoverable**, **outage**, **money**.

How the lists were made:

- Railway: the public GraphQL schema answers introspection with no token.
  The mutation list below is the full list of 255 root mutation fields, filtered by hand.
  Source: `POST https://backboard.railway.com/graphql/v2` with an introspection query, 2026-11-24.
  The same request to `backboard.railway.app` also answers (HTTP 200).
  The CLI mapping comes from the source of the Railway CLI `railwayapp` 5.75.0 on crates.io (published 2026-11-17).
- Vercel: the official OpenAPI file at `https://openapi.vercel.sh/` (356 paths).
- Fly.io: the Machines API OpenAPI file at `https://docs.fly.io/api/machines/openapi.json`,
  introspection of `https://api.fly.io/graphql` (it answers with no token, 62 mutations),
  and the flyctl v0.5.16 source from proxy.golang.org.
- Heroku: the Platform API JSON schema at `https://api.heroku.com/schema` (101 resources),
  and the `heroku` CLI 11.14.0 from npm for the Data API paths.
- Render: the OpenAPI file at `https://api-docs.render.com/openapi/render-public-api-1.json`.
- Netlify: the OpenAPI file at `https://open-api.netlify.com/swagger.json` (version 2.63.1).

## What matters most for Silo

1. On Railway, one GraphQL root field can delete data through four different mutations, not one.
   `volumeDelete` is the obvious one.
   `environmentPatchCommit` deletes a volume, a service, a bucket or a group when its `patch` variable holds `"isDeleted": true`.
   `environmentApplyChangeSet` takes a free JSON `input` that may also delete (unverified what it can hold).
   `environmentPatchCommitStaged` commits a patch that was staged earlier by `environmentStageChanges`; the commit request itself carries no sign of what it deletes.
   The CLI's own `railway bucket delete`, `railway scale` and the CLI's MCP tool `remove_bucket` use `environmentPatchCommit`, not a `*Delete` mutation.
   A rule that matches only `*Delete` root fields misses these.
2. A GraphQL rule must read root field names from the parsed document, not `operationName` and not the response key.
   Railway accepted an alias: `mutation { a: volumeDelete(volumeId:"x") }` came back with `path: ["a"]` (tested with no token, 2026-11-24).
   The PocketOS request had no `operationName` at all.
   Railway rejected batched JSON arrays, GET with `?query=`, `application/graphql` and form bodies when tested with no token.
   Whether it rejects them with a token is unverified.
3. "Make it undoable first" works poorly on Railway volumes.
   Railway's docs say "Wiping a volume deletes all backups."
   Backups are tied to the volume. Railway's post says the legacy API delete made PocketOS's backups look unavailable; Railway got the data back itself.
   A backup Silo takes right before `volumeDelete` would sit in the same place.
   Since late April 2026 the API soft-deletes for 48 hours, but the only documented restore is a link in an email to the owner.
   The schema has no restore-deleted-volume mutation.
   For a database with PITR on, `volumeInstancePITRRestore` creates a separate new service from the archive bucket, which does survive a `volumeDelete` of the source volume.
4. Fly.io tokens are macaroons that anyone holding one can narrow offline, with no call to Fly.
   The caveats carry per-resource action masks (`r`, `w`, `c`, `d`, `C`).
   Silo holds the real token, so it could add caveats (no delete, one app, short expiry) before it swaps the token into a request.
   Whether every Machines API route checks the `d` bit is unverified.
5. Token scoping differs a lot.
   Render (since 2026-11-18) and Fly.io can make tokens that cannot delete.
   Heroku can, through the OAuth `read` scope.
   Vercel personal tokens cannot be made read-only; only "Sign in with Vercel" app tokens have Read Only permissions.
   Railway account, workspace and project tokens cannot be made read-only; only OAuth `*:viewer` scopes can.
   Netlify personal tokens have no scopes.
6. Several CLIs move destructive work off HTTPS JSON:
   `railway ssh` and `railway volume files delete` use SSH to `ssh.railway.com`;
   flyctl tunnels WireGuard inside a websocket to `gateway.machines.dev:443` and reaches `_api.internal:4280` and Machines over it;
   `heroku pg:psql`, `render psql` and Fly MPG `connect` speak the Postgres wire protocol;
   `heroku run` uses a rendezvous socket (unverified host).
   Silo cannot read what goes through those as HTTP requests.

---

## Railway

Host: `backboard.railway.com` (also `backboard.railway.app`), `POST /graphql/v2`, JSON body `{"query": ..., "variables": ...}`.
Docs: https://docs.railway.com/integrations/api.md (read 2026-11-24).
Every row is `POST /graphql/v2`; the table names the root mutation field.

### Data deletion

| Root field (args) | Outcome | CLI / MCP that emits it | Can Silo make it undoable first? | Token that cannot do it |
|---|---|---|---|---|
| `volumeDelete(volumeId)` | **recoverable** for 48 h since the 2026-05-01 change, by the owner through an emailed "Restore" link. Then **gone**. Since 2026-04-29 Railway also delays deleting the volume's backups (see recovery). | `railway volume delete` (`rm`, `remove`; `--yes`, `--2fa-code`); CLI MCP tool `remove_volume` (asks again with `confirm: true`); also `railway scale` on HA clusters | Partly. A backup (`volumeInstanceBackupCreate`) lives on the same volume, so it does not help. For a DB with PITR: `volumeInstancePITRRestore(volumeInstanceId, targetTimestamp, newServiceName)` makes an independent copy first. | None of account, workspace or project tokens. OAuth `project:viewer` / `workspace:viewer` cannot (unverified that viewer is refused this mutation). |
| `environmentPatchCommit(environmentId, patch, commitMessage, skipDeploys)` with `patch.volumes.<id>.isDeleted = true` | same as `volumeDelete` (unverified that the patch path also gets the 48 h soft delete) | `railway environment edit`, `railway environment new`, `railway bucket delete`, `railway scale`, TCP proxy commands, CLI MCP `remove_bucket` | Same as above. Silo must parse the `patch` JSON variable. | same |
| same, with `patch.services.<id>.isDeleted = true` | **gone** for the service config and deployments (no documented grace period for services; one user report says none, unverified) | as above; IaC/config-as-code apply | No API to snapshot a service. | same |
| same, with `patch.buckets.<id>.isDeleted = true` | **gone** (unverified: no documented grace period for buckets). A PITR archive lives in a bucket, so this also removes the PITR history. | `railway bucket delete`, CLI MCP `remove_bucket` | No. | same |
| `environmentStageChanges(environmentId, input, merge)` then `environmentPatchCommitStaged(environmentId, commitMessage, skipDeploys)` | as the patch it holds | template apply in CLI (`controllers/template_apply.rs`) | Silo would need to read the staged patch first with the query `environmentStagedChanges` before letting the commit pass. | same |
| `environmentApplyChangeSet(environmentId, input: JSON, baseConfigEtag, waitForCompletion)` | unknown; schema says "Experimental: applies an intent-level RailwayChangeSet" | not found in CLI 5.75.0 | unverified | same |
| `serviceDelete(id, environmentId)` | **gone** for the service; whether attached volumes are deleted too is unverified. MCP text: "permanently delete the service and all its deployments". | `railway service delete`; `railway functions delete`; CLI MCP `remove_service` | No API copy of a service. If the service has a volume: back it up does not help (same volume). | same |
| `environmentDelete(id)` | **gone** for the environment and what it holds (unverified: Railway docs do not say what happens to volumes) | `railway environment delete` | No. | project token is limited to one environment; it can likely delete that one (unverified) |
| `projectDelete(id)` | Docs for the CLI say "permanent". The dashboard and `projectScheduleDelete` give 48 h. Whether bare `projectDelete` now also waits 48 h is unverified. | not used by CLI 5.75.0 (`railway delete` uses `projectScheduleDelete`) | No. | account or workspace token can; project token unverified |
| `projectScheduleDelete(id)` | **recoverable** 48 h with `projectScheduleDeleteCancel(id)`, then **gone** | `railway delete` (`--yes`, `--2fa-code`) | Already a grace period; Silo can call `projectScheduleDeleteCancel` itself after a late "no". | same |
| `projectScheduleDeleteForce(id)` | **gone**: "skips the grace period" | not in CLI | No. Hold always. | same |
| `workspaceDelete(id)` | **gone** "and all data associated with it" | not in CLI | No. | workspace admin only |
| `volumeInstanceBackupDelete(volumeInstanceId, volumeInstanceBackupId)` | **gone** for that backup | `railway postgres pitr` backup commands (`commands/database/pitr.rs`) | No (a backup of a backup lives in the same place). | none |
| `volumeInstanceBackupScheduleUpdate(volumeInstanceId, kinds)` | silent loss later: dropping a kind stops new backups (unverified whether it deletes existing scheduled backups) | `railway postgres pitr` | No; it is a setting. Hold. | none |
| `volumeInstanceBackupRestore(volumeInstanceId, volumeInstanceBackupId, wipeServiceIds, replicaServiceIds)` | overwrites live data with an older backup. Docs: the previous volume is kept unmounted, so **recoverable** by re-mounting it (unverified through API). `wipeServiceIds` name says it wipes. | `railway postgres pitr` | It keeps the old volume itself. | none |
| `disablePitrForHaCluster(input)` | HA: archive bucket kept. Single node: Railway stages a patch that "deletes the PITR bucket" (docs). Then **gone** history. | `railway postgres pitr` | No. | none |
| `sandboxDestroy(id, environmentId)`, `sandboxCheckpointDelete`, `cloudAgentDelete`, `cloudAgentCheckpointDelete` | **gone** for Railway sandboxes and cloud agents | `railway sandbox`, `railway code` | No. | none |
| `templateDelete(id, input)` | **gone** for a published template | `railway templates` | No. | none |
| `signalDelete(input)` | **gone**: "Delete a signal and its audit log" | not checked | No. | none |
| `userDelete` | **gone** account | not in CLI | No. | account token |

### Outage

| Root field | What stops | CLI |
|---|---|---|
| `deploymentRemove(id)`, `deploymentStop(id)`, `deploymentCancel(id)` | the running deployment | `railway down` (uses `deploymentRemove`) |
| `deploymentRollback(id)`, `deploymentRedeploy`, `deploymentRestart`, `serviceInstanceRedeploy` | restart or older code | `railway redeploy`, `railway restart` |
| `serviceInstanceUpdate(input: {numReplicas: 0, sleepApplication: true, ...})` | scale to zero, sleep | `railway scale` |
| `volumeInstanceUpdate(volumeId, environmentId, input: {serviceId: null})` | detaches the volume ("If not provided, the volume will be disconnected") | `railway volume detach` |
| `serviceDisconnect(id)` | removes the repo link | `railway service`, CLI MCP `disconnect_service_source` |
| `variableDelete(input)` | one variable gone; next deploy may fail. The old value is lost unless Silo read it first (`variables` query). | `railway variable delete` |
| `variableCollectionUpsert(input: {replace: true, ...})` | "removes all existing variables before upserting" | `railway variable set`, CLI MCP |
| `customDomainDelete`, `serviceDomainDelete`, `tcpProxyDelete`, `railwayDomainDnsRecordDelete`, `railwayDomainEmailForwardingRemoveAll`, `railwayDomainEmailForwardingRuleDelete`, `trustedDomainDelete` | DNS, domains, mail forwarding | `railway domain`, `railway tcp-proxy`, CLI MCP `delete_domain`, `remove_tcp_proxy` |
| `databasePasswordReset(input)`, `bucketCredentialsReset(...)` | apps lose their DB or bucket credentials until redeployed | `railway bucket` |
| `disableServiceCdn`, `purgeServiceCache`, `egressGatewayAssociationsClear`, `egressGatewayRollbackFromHA`, `setServiceUnderAttackMode`, `deploymentTriggerDelete` | edge, egress IP, auto-deploy | `railway cdn`, `railway outbound-network`, `railway waf` |
| `apiTokenDelete`, `projectTokenDelete`, `workspaceApiTokenDelete`, `clearanceTokenRevoke`, `sshPublicKeyDelete`, `passkeyDelete`, `twoFactorInfoDelete`, `providerAuthRemove`, `sessionDelete` | access and keys | `railway ssh keys` |
| `projectMemberRemove`, `projectLeave`, `workspaceUserRemove`, `workspaceLeave`, `accessGroupDelete`, `accessGroupMemberRemove`, `accessGroupProjectDetach`, `projectInvitationDelete`, `workspacePermissionChange`, `projectMemberUpdate` | people lose access | not in CLI |
| `projectTransfer`, `projectTransferInitiate`, `projectTransferConfirm` | the project moves to another workspace | not in CLI |
| `workspacePolicyItemUpdate`, `workspacePolicyDeploySourceAllowlistRemove`, `environmentClearanceDefaultUpdate`, `serviceInstanceClearanceUpdate`, `workspaceTwoFactorEnforcementUpdate` | turns guardrails off (protection off) | not in CLI |

### Money

`usageLimitRemove`, `usageLimitSet`, `agentUsageLimitSet` change spend caps.
`customerTogglePayoutsToCredits` changes template payouts.
Creating services, volumes, replicas (`serviceCreate`, `volumeCreate`, `templateDeployV2`, `serviceInstanceUpdate` with more replicas or regions) charges usage.

### Railway recovery

Volumes: "When a volume is deleted, it is queued for deletion and will be permanently deleted within 48 hours. You can restore the volume during this period using the restoration link sent via email."
(https://docs.railway.com/volumes/reference.md, read 2026-11-24.)
Backups: "Wiping a volume deletes all backups."
Daily backups are kept 6 days, weekly 27 days, monthly 89 days.
(https://docs.railway.com/volumes/backups.md.)
Before the PocketOS incident the API path skipped the grace period that the dashboard had.
Railway's changelog of 2026-05-01 says: "`volumeDelete` removed volumes immediately, with no recovery path" and "Volume deletions over the API now soft-delete for 48 hours."
(https://railway.com/changelog/2026-05-01-undoable-deletes.md.)
Railway's post of 2026-04-29 says the legacy API path "performed a cascading delete on the model, making the backups look unavailable in the UI", that it now also delays deletion of the backups, that Railway keeps separate offsite "disaster backups" whatever the user configured, and that the customer (unnamed in the post; PocketOS in the press) is "back up with all their data".
(https://blog.railway.com/p/your-ai-wants-to-nuke-your-database.)
A Railway staff reply of 2026-04-27 confirms the two-day grace period on the mutation.
(https://station.railway.com/questions/any-changes-to-protect-database-backups-3b31d4d3.)
Disaster backups are not a product feature with a documented window or a self-service restore (unverified beyond that post).
The schema has `VolumeInstance.isPendingDeletion` and `deletedAt` but no mutation that restores a deleted volume.
So during the 48 hours only the owner's email link brings the volume back.
Projects: `projectScheduleDelete` waits 48 hours and `projectScheduleDeleteCancel` undoes it; `projectScheduleDeleteForce` skips the wait.
Services: no documented grace period (unverified).
PITR (Postgres and MySQL) archives to a Railway storage bucket; restore always makes a new service; the MySQL archive is kept 7 days, the Postgres one about 4 weeks.
(https://docs.railway.com/volumes/point-in-time-recovery.md.)

### Railway token scoping

Four kinds (https://docs.railway.com/integrations/api.md):
account token ("any API action you're authorized to do across all your resources and workspaces", header `Authorization: Bearer`),
workspace token (one workspace, `Authorization: Bearer`),
project token (one environment of one project, header `Project-Access-Token`),
OAuth token (scopes chosen by the user).
None of the first three can be limited to reading or barred from deleting.
OAuth scopes are `workspace:viewer`, `workspace:member`, `workspace:admin`, `project:viewer`, `project:member`, and the scope caps the user's role.
(https://docs.railway.com/integrations/oauth/scopes-and-user-consent.md.)
Project roles: Owner, Editor, Viewer; "Editors can not do destructive actions such as deleting services or deleting the project itself."
(https://docs.railway.com/projects/project-members.)
Workspace roles: Admin, Member, Deployer (https://docs.railway.com/projects/workspaces.md).
Whether an Editor can call `volumeDelete` is unverified.
Railway's own post names the PocketOS token as account-scoped and says the token flow "didn't make it obvious which one to pick".
Railway's remote MCP at `mcp.railway.com` uses short-lived tokens from a browser login with chosen workspaces and projects (same post).

### Railway channels Silo might not read

- `wss://backboard.railway.com/graphql/v2` (graphql-ws) carries subscriptions such as logs.
  The schema's subscriptions are all reads.
  Whether the server would also run a mutation sent over the websocket is unverified; Silo should hold or deny mutations there too.
- `ssh <instanceId>@ssh.railway.com` (SSH, `russh`) for `railway ssh`, and SFTP over it for `railway volume files ...`.
  `railway volume files delete` deletes files inside a live volume.
  The CLI refuses it when it sees an agent environment variable (`CLAUDECODE`, `CURSOR_AGENT`, ...), which a hostile agent can unset.
- `wss://ssh.railway.com:2226/ws/exec` for `railway sandbox` exec.
- `railway api '<any GraphQL>'` is a raw passthrough; it reaches the same HTTPS endpoint, so Silo sees it.
- `railway mcp` (the CLI's local MCP server) runs inside the computer and calls the same HTTPS GraphQL, so Silo sees the mutations, not the tool names.

---

## Vercel

Host: `api.vercel.com`, REST JSON, `Authorization: Bearer <token>`, `?teamId=` for full-account tokens.
Spec: https://openapi.vercel.sh/ (read 2026-11-24).
Vercel Blob data goes to another host: the `@vercel/blob` 2.9.0 SDK sends `POST https://vercel.com/api/blob/delete` with `{"urls": [...]}` and `Authorization: Bearer <BLOB_READ_WRITE_TOKEN>` (overridable with `VERCEL_BLOB_API_URL`).
The CLI is now a native binary (npm `vercel` 66.6.0 only downloads it), so the CLI mapping below comes from the CLI docs, not its code.
`vercel api <path> -X DELETE` is a raw passthrough; it asks before any DELETE unless `--dangerously-skip-permissions` (https://vercel.com/docs/cli/api.md).

### Data and resources

| Operation | Wire | Outcome | CLI | Undoable first? |
|---|---|---|---|---|
| Delete project | `DELETE /v9/projects/{idOrName}` | **gone**: "Deleting your project will also delete the deployments, domains, environment variables, and settings within it." No documented restore. | `vercel project rm/delete <name>` (`--confirm-name`) | No API export of a whole project. Silo could read and keep env vars and settings first (`GET /v9/projects/{id}`, `GET /v10/projects/{id}/env?decrypt=true`, unverified for sensitive vars). |
| Delete deployment | `DELETE /v13/deployments/{id}` (or `?url=`) | **recoverable** from Settings, Security, Recently Deleted: 1 day if under 1 day old, as long as its age if under 30 days, 30 days otherwise. Dashboard only. | `vercel remove/rm <deployment>` | Already soft. A deployment under a day old has only 1 day. |
| Delete Blob store | `DELETE /storage/stores/blob/{id}` | **gone**: "This action is permanent and cannot be undone." | `vercel blob delete-store`, `vercel storage delete` (both always ask interactively, no `--yes`) | No snapshot API for Blob. |
| Delete blobs | `POST https://vercel.com/api/blob/delete` body `{"urls":[...]}` | **gone** (no soft delete documented) | `vercel blob delete`, `vercel blob empty-store --yes`, SDK `del()` | Silo could copy each blob first (`POST /api/blob/copy`, unverified path) if the request names few URLs. |
| Delete Global Config (formerly Edge Config) | `DELETE /v1/global-config/{edgeConfigId}` | **gone** (no restore documented) | `vercel global-config delete <name> --yes` | Silo can read all items first (`GET /v1/global-config/{id}/items`) and keep them. |
| Delete Global Config items | `PATCH /v1/global-config/{edgeConfigId}/items` body `{"items":[{"operation":"delete","key":...}]}` | **gone** for those keys | `vercel global-config` items | Read the keys first, keep them. The method is PATCH: a rule on DELETE misses it. |
| Remove env var | `DELETE /v9/projects/{idOrName}/env/{id}`, `DELETE /v1/projects/{idOrName}/env` (batch), `DELETE /v1/env` (shared), `PATCH /v1/env/{id}/unlink/{projectId}` | **gone** value; **outage** at next deploy | `vercel env rm` | Read the value first if it is not "sensitive" (sensitive values cannot be read back, unverified). |
| Delete Marketplace resource (Neon Postgres, Upstash Redis, former Vercel Postgres and KV) | user-side endpoint not in the public spec (unverified). `DELETE /v1/installations/{integrationConfigurationId}/resources/{resourceId}` exists but is the partner-side route. | **gone** or as the partner allows. A Vercel community thread shows a Neon deletion "pending" state (unverified). | `vercel integration resource delete <name> [--disconnect-all] [--yes]` | Not through Vercel. Through the partner's own API (Neon branches) if Silo holds that key. |
| Uninstall integration | `DELETE /v1/integrations/configuration/{id}` | **outage**; CLI requires resources deleted first | `vercel integration remove <name> --yes` | No. |
| Delete drive (Sandbox) | `DELETE /v1/drives/{nameOrId}`, `DELETE /v2/sandboxes/drives/{nameOrId}` | **gone** (unverified) | sandbox CLI | `POST /v1/drives/{name}/fork` first. |
| Delete sandbox snapshot, sandbox | `DELETE /v2/sandboxes/snapshots/{snapshotId}`, `DELETE /v2/sandboxes/{name}` | **gone** | `vercel sandbox` | No. |
| Delete container image / repository | `DELETE /v1/vcr/repository/{idOrName}`, `.../images/{imageId}`, `DELETE /v2/{teamSlug}/{projectSlug}/{repositoryName}/manifests/{reference}`, `.../blobs/{digest}` | **gone** | not checked | No. |
| Delete feature flag, segment | `DELETE /v1/projects/{p}/feature-flags/flags/{flag}`, `.../segments/{seg}` | **gone** config | `vercel flags` (unverified) | Read first. |
| Delete form-like data: log drains, webhooks, checks | `DELETE /v1/drains/{id}`, `DELETE /v1/webhooks/{id}`, `DELETE /v2/projects/{p}/checks/{id}` | **outage** | various | Read config first. |
| Delete team | `DELETE /v1/teams/{teamId}` | **gone** (everything in it) | not checked | No. |
| Delete account | `DELETE /v1/user` | **gone** (requests deletion by email, unverified) | not checked | No. |
| Delete immutable static asset | not found in the spec (unverified) | **gone**: "You cannot undo this operation." 410 for seven days. | `vercel project dangerously-delete-immutable-static-asset` (interactive only) | No. |
| Delete all remote cache artifacts | `DELETE /v8/artifacts` | **outage** (rebuilds, cost) | Turborepo | No need. |

### Outage

| Operation | Wire | CLI |
|---|---|---|
| Remove domain from project | `DELETE /v9/projects/{idOrName}/domains/{domain}` | `vercel domains rm` |
| Remove domain from account | `DELETE /v6/domains/{domain}` | `vercel domains rm` |
| Move domain | `POST /v1/projects/{idOrName}/domains/{domain}/move`, `PATCH /v3/domains/{domain}` | `vercel domains move` |
| Delete DNS record | `DELETE /v2/domains/{domain}/records/{recordId}` | `vercel dns rm` |
| Delete alias | `DELETE /v2/aliases/{aliasId}` | `vercel alias rm` |
| Remove cert | `DELETE /v8/certs/{id}` | `vercel certs rm` |
| Pause project | `POST /v1/projects/{projectId}/pause` (undo: `/unpause`) | dashboard |
| Roll back / promote | `POST /v1/projects/{projectId}/rollback/{deploymentId}`, `POST /v10/projects/{projectId}/promote/{deploymentId}` | `vercel rollback`, `vercel promote` |
| Cancel deployment | `PATCH /v12/deployments/{id}/cancel` | |
| Firewall config, bypass | `DELETE /v1/security/firewall/config/{configVersion}`, `DELETE /v1/security/firewall/bypass`, `PATCH /v1/projects/{idOrName}/protection-bypass`, `PATCH /aliases/{id}/protection-bypass` | protection off |
| Remove custom environment | `DELETE /v9/projects/{idOrName}/custom-environments/{env}` | |
| Delete routes, redirects | `DELETE /v1/projects/{projectId}/routes`, `DELETE /v1/bulk-redirects` | |
| Purge CDN | `POST /v1/edge-cache/dangerously-delete-by-tags`, `.../dangerously-delete-by-src-images`, `invalidate-by-*` | `vercel cache` |
| Delete token | `DELETE /v3/user/tokens/{tokenId}` | `vercel tokens` |
| Members | `DELETE /v1/teams/{teamId}/members/{uid}`, `DELETE /v1/projects/{idOrName}/members/{uid}`, `DELETE /v1/access-groups/...` | |
| Transfer project | `POST /projects/{idOrName}/transfer-request` | |
| AI Gateway rules, BYOK keys, budgets | `DELETE /ai-gateway/...` | |
| Revoke credentials | `POST /v1/installations/{id}/credentials/revoke`, `POST /v1/kms/issuers/{issuerId}/keys/{keyId}/revoke` | |

### Money

`POST /v1/registrar/domains/buy`, `POST /v1/registrar/domains/{domain}/buy`, `POST /v1/registrar/domains/{domain}/renew`, `POST /v1/registrar/domains/{domain}/transfer`, `POST /v7/domains` (add or transfer), `POST /v1/billing/buy` ("Purchase credits"), and creating paid Marketplace resources.

### Vercel recovery

Deployments: recovery period as in the table (changelog of 2026-10-16, https://vercel.com/changelog/deleted-deployments-are-permanently-removed-sooner, and https://vercel.com/docs/deployment-retention).
Projects: no restore documented; deleting a project deletes its deployments, domains, env vars and settings (https://vercel.com/docs/projects/managing-projects).
Blob stores: "permanent and cannot be undone" (https://vercel.com/docs/cli/blob).
Global Config stores: "Deletion cannot be undone" (https://vercel.com/docs/cli/storage).
A revoked Blob read-write token can be restored from the store's Settings (https://vercel.com/docs/vercel-blob/security).
Marketplace databases follow the partner's rules (unverified).

### Vercel token scoping

Personal access tokens (prefix `vcp_`) have three reaches: Full Account, Team, Project.
"A project-scoped token denies any request to another project, to a user-level resource, or to a team-level resource."
Inside its reach a token reads and writes everything; there is no read-only option (https://vercel.com/docs/accounts/access-tokens, last updated 2026-11-14).
So a token made "to deploy" can delete the same project.
"Sign in with Vercel" OAuth app tokens have per-resource permissions (Read Only, Read and Write, Manage) such as Projects, Deployments, Domains, Blob (Blob is Read and Write only) (https://vercel.com/docs/sign-in-with-vercel/scopes-and-permissions).
Blob store tokens (`BLOB_READ_WRITE_TOKEN`) are read-write for the whole store.
Vercel Connect (GA 2026-08-25) gives short-lived tokens for third-party services, not for the Vercel API itself (https://vercel.com/blog/the-end-of-credential-sprawl-for-agents).
Vercel MCP at `mcp.vercel.com` uses OAuth; its documented tools are mostly reads (https://vercel.com/docs/agent-resources/vercel-mcp/tools).

### Vercel channels

All found calls are HTTPS JSON.
Blob uploads may go straight to a storage host (unverified).

---

## Fly.io

Two APIs.
Machines API: `https://api.machines.dev/v1/...`, REST JSON, `Authorization: Bearer <token>` or `FlyV1 <macaroon>` (spec https://docs.fly.io/api/machines/openapi.json).
GraphQL: `POST https://api.fly.io/graphql`.
flyctl also calls `https://api.fly.io/api/v1/...` (the "uiex" REST API) for Managed Postgres.
Volume deletion moved out of GraphQL: the 62 mutations of `api.fly.io/graphql` on 2026-11-24 have no volume or machine mutation.

### Machines API

| Operation | Wire | Outcome | flyctl | Undoable first? |
|---|---|---|---|---|
| Destroy app | `DELETE /v1/apps/{app_name}` (also GraphQL `deleteApp`) | **gone**: "When you destroy an app, you delete its volumes and volume snapshots." | `fly apps destroy` | Snapshots die with the app. Silo would need to copy data out first, which needs a Machine (`fly volumes fork` to another app, unverified across apps). Hold always. |
| Destroy volume | `DELETE /v1/apps/{app_name}/volumes/{volume_id}` | **recoverable**: soft deleted (`pending_destroy`) for 24 h; snapshots stay until their retention ends (default 5 days, 1 to 60); restore by creating a volume from a snapshot (`fly volumes create --snapshot-id`). The deleted volume's ID is listed for 24 h only. | `fly volumes destroy` | Yes: `POST /v1/apps/{app}/volumes/{id}/snapshots` first, and note the volume ID. |
| Lower snapshot retention, stop daily snapshots | `PUT /v1/apps/{app_name}/volumes/{volume_id}` body `{"snapshot_retention": 1, "auto_backup_enabled": false}` | later loss; existing snapshots keep their old retention ("it won't affect existing snapshots") | `fly volumes update --snapshot-retention` | Hold. |
| Destroy Machine | `DELETE /v1/apps/{app_name}/machines/{machine_id}?force=true` | the Machine's root filesystem is **gone**; volumes stay | `fly machine destroy --force` | Not for rootfs. |
| Exec in Machine | `POST /v1/apps/{app_name}/machines/{machine_id}/exec` body `{"cmd": ...}` | anything, for example `rm -rf /data` on a mounted volume | `fly machine exec` | Snapshot the volume first. Silo can read the command in the body. |
| Stop, suspend, restart, signal, cordon | `POST .../machines/{id}/stop`, `/suspend`, `/restart`, `/signal`, `/cordon` | **outage** | `fly machine stop` etc. | |
| Delete Managed Postgres cluster | `DELETE /v1/postgres/{postgres_cluster_id}` ("marked for deletion and removed asynchronously"); flyctl uses `DELETE https://api.fly.io/api/v1/organizations/{org}/postgresv2/{id}` | Docs: "permanently destroys ... and all its data ... not reversible". flyctl can list deleted clusters (`fly mpg list --deleted`); whether they or their backups can be restored is unverified. | `fly mpg destroy` | `POST /v1/postgres/{id}/fork` or `/backups` first. A fork is a separate cluster and survives. |
| Drop database in cluster | `DELETE /v1/postgres/{id}/databases/{database_name}` | **gone** unless restored from backup or PITR into a new cluster | `fly mpg databases` | `POST /v1/postgres/{id}/backups` first. |
| Delete Postgres user, detach cluster | `DELETE /v1/postgres/{id}/users/{username}`, `DELETE /v1/postgres/{id}/attachments/{app_name}` | **outage** | `fly mpg users`, `fly mpg detach` | |
| Read SQL | `POST /v1/postgres/{id}/databases/{db}/query` | none: "runs as pg_read_all_data inside a read-only transaction" | | |
| Delete secret, secret key | `DELETE /v1/apps/{app_name}/secrets/{name}`, `DELETE .../secretkeys/{name}` (GraphQL `unsetSecrets`) | value **gone** (Fly never shows secret values); a deleted KMS key makes its ciphertext unreadable | `fly secrets unset`, `fly secrets keys delete` | No, values cannot be read back. |
| Remove certificate, IP | `DELETE /v1/apps/{app}/certificates/{hostname}`, `DELETE /v1/apps/{app}/ip_assignments/{ip}` (GraphQL `releaseIpAddress`) | **outage**; a released IP is not got back (unverified) | `fly certs remove`, `fly ips release` | |

### GraphQL mutations that delete or detach

`deleteApp`, `deleteAddOn` (Upstash Redis, Tigris and other extensions; **gone**, unverified), `deleteOrganization`, `deleteOrganizationMembership`, `deleteCertificate`, `deleteDeploymentSource`, `deleteLimitedAccessToken`, `deleteRemoteBuilder`, `deleteThirdPartyConfiguration`, `deleteOrganizationInvitation`, `detachPostgresCluster`, `revokePostgresClusterUserAccess`, `releaseIpAddress`, `releaseEgressIpAddress`, `removeWireGuardPeer`, `removeAllowedReplaySourceOrgs`, `unsetSecrets`, `resetAddOnPassword`, `moveApp`, `cancelBuild`.
Money: `createAddOn`, `createOrganization`, `allocateIpAddress` (dedicated IPv4 is billed).

### Fly.io recovery

"When you destroy a volume (whether by flyctl commands or the Machines API), the volume stays in the pending_destroy state for 24 hours before being permanently deleted." Fly keeps it "so that we can recover them if needed"; whether a user can recover it alone is unverified (https://docs.fly.io/volumes/volume-states).
"Snapshots taken before deletion may still be restorable for a limited time, provided you have the volume ID (default retention is 5 days)" and the deleted volume's ID can be listed for 24 hours (https://docs.fly.io/volumes/volume-manage).
Destroying an app deletes its volumes and snapshots (https://docs.fly.io/apps/delete).
Managed Postgres backups after a cluster is destroyed: unverified.

### Fly.io token scoping

Tokens are macaroons (https://docs.fly.io/security/tokens).
Kinds: app deploy token (`fly tokens create deploy`), SSH-only (`fly tokens create ssh`), machine-exec with a command allowlist (`fly tokens create machine-exec --command`), org token, read-only org token (`fly tokens create readonly`, also `--from-existing`).
Default lifetime is 20 years unless `--expiry` is given.
`fly tokens attenuate` appends caveats offline to any token.
The caveat library (`github.com/superfly/macaroon` v0.3.3) has resource caveats `Apps`, `Volumes`, `Machines`, `FeatureSet`, with action masks read, write, create, delete, control.
For Silo this means: the boundary can narrow the user's token itself, per rule, before it swaps it in.
An app deploy token can still destroy that app's volumes (unverified; it can "manage" the app).

### Fly.io channels

- WireGuard. flyctl by default wraps WireGuard in a websocket to `gateway.machines.dev:443` (`wg/tunnel.go`, `wg/ws.go`), or plain UDP WireGuard.
  Inside the tunnel flyctl reaches `_api.internal:4280` (a Machines API endpoint on the private network) and Machines directly.
  Silo sees only the websocket.
  The flyctl source says the macaroon also rides in the websocket upgrade's `Authorization` header so that "a transparent proxy in front of the gateway can add or replace the credential there", which fits Silo's secret swap; what then flows inside the tunnel stays unread.
- `fly ssh console`, `fly proxy`, `fly mpg connect` / `fly mpg proxy` go through that tunnel (SSH, Postgres wire).
- Inside a Fly Machine, `/.fly/api` is a Unix socket to the Machines API; not relevant to a Silo computer.

---

## Heroku

Platform API: `https://api.heroku.com`, header `Accept: application/vnd.heroku+json; version=3`, `Authorization: Bearer <token>`.
Schema read from `https://api.heroku.com/schema` on 2026-11-24.
Heroku Postgres actions use the Data API at `https://api.data.heroku.com` (CLI default; `HEROKU_DATA_HOST` overrides it), found in `@heroku/heroku-cli-util` 10.10.4.

| Operation | Wire | Outcome | CLI | Undoable first? |
|---|---|---|---|---|
| Delete app | `DELETE /apps/{app}` | add-ons go with it; databases **recoverable** only on Standard tier or higher, by forking from continuous protection within the rollback window, using the old `DATABASE_URL` from an earlier release ([help article](https://help.heroku.com/SPTSN5U6/can-i-recover-a-deprovisioned-production-tier-heroku-postgres-database)); app code and config unverified | `heroku apps:destroy --confirm` | Capture a backup first (`POST https://api.data.heroku.com/client/v11/databases/{id}/backups`, unverified path; CLI `pg:backups:capture`). Backups of deprovisioned add-ons are deleted "after a short grace period", so Silo would have to download it too. |
| Delete add-on (Heroku Postgres, Key-Value Store, others) | `DELETE /apps/{app}/addons/{addon}` | as above for Postgres; Essential tier has no rollback, so **gone** | `heroku addons:destroy`, `heroku data:pg:destroy` (Postgres Advanced) | Backup and download first. |
| Reset database | `PUT https://api.data.heroku.com/client/v11/databases/{id}/reset` | **gone** data; Standard+ can roll back within the window (unverified for reset) | `heroku pg:reset --confirm` | Capture a backup first. |
| Restore backup over database | `POST https://api.data.heroku.com/client/v11/databases/{id}/restores` | "deletes all data from the target database before restoring" | `heroku pg:backups:restore` | Capture a backup first. |
| Delete backup | `DELETE https://api.data.heroku.com/client/v11/apps/{app}/transfers/{num}` | **gone** | `heroku pg:backups:delete` | No. |
| Unschedule backups | `DELETE https://api.data.heroku.com/client/v11/databases/{id}/transfer-schedules/{uuid}` | later loss | `heroku pg:backups:unschedule` | Hold. |
| Destroy or rotate DB credentials | `DELETE https://api.data.heroku.com/postgres/v0/databases/{name}/credentials/{cred}`, `POST .../credentials_rotation` | **outage** | `heroku pg:credentials:destroy`, `:rotate` | |
| Kill connections, unfollow | `POST .../client/v11/databases/{id}/connection_reset`, `PUT .../unfollow` | **outage** | `heroku pg:killall`, `heroku pg:unfollow` | |
| Remove config var | `PATCH /apps/{app}/config-vars` with a key set to `null` | value **gone**, restart | `heroku config:unset` | Read config vars first (`GET /apps/{app}/config-vars`). Releases also keep old config (rollback, see below). |
| Scale to zero | `PATCH /apps/{app}/formation` body `{"updates":[{"type":"web","quantity":0}]}` | **outage** | `heroku ps:scale web=0` | Undo is another PATCH. |
| Stop dynos | `POST /apps/{app}/dynos/{dyno}/actions/stop`, `POST /apps/{app}/formations/{type}/actions/stop` | **outage** | `heroku ps:stop` | |
| Restart dynos | `DELETE /apps/{app}/dynos`, `DELETE /apps/{app}/dynos/{dyno}`, `DELETE /apps/{app}/formations/{type}` | short **outage** only. These are DELETEs that destroy nothing: a rule "hold every DELETE" holds restarts too. | `heroku ps:restart` | |
| Roll back release | `POST /apps/{app}/releases` body `{"release": ...}` | older code and config | `heroku releases:rollback` | |
| Delete domain, SNI endpoint, log drain, webhook, pipeline, review app | `DELETE /apps/{app}/domains/{d}`, `DELETE /apps/{app}/sni-endpoints/{id}`, `DELETE /apps/{app}/log-drains/{id}`, `DELETE /apps/{app}/webhooks/{id}`, `DELETE /pipelines/{id}`, `DELETE /review-apps/{id}` | **outage** | `heroku domains:remove`, `heroku certs:remove`, `heroku drains:remove` | |
| Delete space, peering, VPN | `DELETE /spaces/{space}`, `DELETE /spaces/{s}/peerings/{pcx}`, `DELETE /spaces/{s}/vpn-connections/{id}` | **gone** for the space and its apps (unverified) | `heroku spaces:destroy` | |
| Transfer app | `POST /account/app-transfers`, `PATCH /teams/apps/{app}` | ownership moves | `heroku apps:transfer` | |
| Team, account | `DELETE /teams/{team}`, `DELETE /account` (needs the account password header) | **gone** | | |
| Revoke tokens | `DELETE /oauth/authorizations/{id}`, `DELETE /oauth/tokens/{id}`, `POST /oauth/authorizations/{id}/actions/regenerate-tokens` | **outage** | `heroku authorizations:revoke` | |

Money: `POST /apps/{app}/addons` (paid plans), `PATCH /apps/{app}/formation` with bigger sizes or counts.

### Heroku recovery

Backups: "We delete backups for deprovisioned add-ons after a short grace period" (https://devcenter.heroku.com/articles/heroku-postgres-backups).
Manual backups kept: Essential 5, Standard 25, Premium and Enterprise 50; scheduled daily backups 7 days.
Continuous protection snapshots and rollbacks exist on Standard tier and higher; Essential has no rollback (https://devcenter.heroku.com/articles/heroku-postgres-data-safety-and-continuous-protection).
The rollback window "varies by database plan" (https://devcenter.heroku.com/articles/heroku-postgres-rollback); the exact windows were not on that page.
A deprovisioned Standard+ database may be forked back within the rollback period (help article above).
Restoring a deleted app: no self-service path found (unverified).

### Heroku token scoping

OAuth scopes: `global`, `identity`, `read`, `write`, `read-protected`, `write-protected` (https://devcenter.heroku.com/articles/oauth).
`read` cannot change anything and cannot see config vars.
`heroku authorizations:create --scope read --expires-in <seconds>` makes such a token (CLI 11.14.0 flags).
The default CLI token is `global`.
Personal direct authorizations do not expire unless `--expires-in` is set; team ones expire after one year.
No scope exists for "write but not delete".

### Heroku channels

`heroku pg:psql`, `pg:kill` (runs `pg_terminate_backend` through psql) and `pg:push/pull` speak the Postgres wire protocol.
`heroku run` and `heroku ps:exec` use a rendezvous or SSH connection, not HTTPS JSON (hosts unverified).
`heroku mcp:start` runs `@heroku/mcp-server` 1.2.15 inside the computer; its tools (`ps_scale`, `pg_psql`, `pg_kill`, `pg_backups`, ...) call the CLI, so Silo sees the same calls as above.

---

## Render

Host: `https://api.render.com/v1`, REST JSON, `Authorization: Bearer <key>`.
Spec: https://api-docs.render.com/openapi/render-public-api-1.json (read 2026-11-24).

| Operation | Wire | Outcome | Undoable first? |
|---|---|---|---|
| Delete Postgres | `DELETE /v1/postgres/{postgresId}` | "This operation is irreversible, and all data will be lost." Whether PITR or exports outlive it is unverified. | `POST /v1/postgres/{id}/export` first (logical backup, kept 7 days) and download it, or `POST /v1/postgres/{id}/recovery` to make a recovery instance, which is a separate database. |
| Point-in-time recovery | `POST /v1/postgres/{postgresId}/recovery` | creates a new instance; harmless to the source | |
| Delete disk | `DELETE /v1/disks/{diskId}` | "All data on the disk will be lost." Daily snapshots are kept 7 days; whether they outlive the disk is unverified. | No API creates a disk snapshot on demand (only list and restore). |
| Restore disk snapshot | `POST /v1/disks/{diskId}/snapshots/restore` | "all changes to your disk that occurred after the snapshot are lost" | No. |
| Delete service | `DELETE /v1/services/{serviceId}` | **gone** (with its disk, unverified) | No. |
| Delete Key Value / Redis | `DELETE /v1/key-value/{id}`, `DELETE /v1/redis/{id}` | **gone** (unverified) | No. |
| Delete env group, env var, secret file | `DELETE /v1/env-groups/{id}`, `DELETE /v1/services/{id}/env-vars/{key}`, `DELETE /v1/services/{id}/secret-files/{key}`, and `PUT /v1/services/{serviceId}/env-vars`, which "Replace[s] all environment variables" | value **gone** | Read them first (`GET`). |
| Delete project, environment | `DELETE /v1/projects/{id}`, `DELETE /v1/environments/{id}` | API refuses unless empty (409) | |
| Remove resources from environment | `DELETE /v1/environments/{environmentId}/resources` | moves, unverified | |
| Delete Postgres user | `DELETE /v1/postgres/{id}/credentials/{username}` | **outage** | |
| Suspend, restart, scale, failover | `POST /v1/services/{id}/suspend`, `/restart`, `/scale`, `/rollback`; `POST /v1/postgres/{id}/suspend`, `/restart`, `/failover`; `POST /v1/key-value/{id}/suspend` | **outage** | |
| Custom domain, routes, headers, webhooks, log streams, registry credentials, dedicated IPs | `DELETE /v1/services/{id}/custom-domains/{d}`, `.../routes/{id}`, `.../headers/{id}`, `DELETE /v1/webhooks/{id}`, `DELETE /v1/logs/streams/...`, `DELETE /v1/registrycredentials/{id}`, `DELETE /v1/dedicated-ips/{id}` | **outage** | |
| Workspace member | `DELETE /v1/owners/{ownerId}/members/{userId}` | **outage** | |
| Sandboxes, workflows, build sources | `DELETE /v1/sandbox-groups/.../snapshots/{id}`, `DELETE /v1/workflows/{id}`, `DELETE /v1/build-sources/{id}` | **gone** | |

CLI: the Render CLI (`render`) exists (https://render.com/docs/cli-reference.md); its command-to-endpoint mapping was not checked.
MCP: `mcp.render.com/mcp`; Render says it "supports potentially destructive operations, including modifying a service's environment variables and triggering deploys" (https://render.com/docs/mcp-server.md).

### Render recovery

PITR for paid databases: past 3 days on Hobby, past 7 days on Pro or higher; none on the Free plan; recovery always creates a new instance (https://render.com/docs/postgresql-backups.md).
Logical exports are kept seven days.
Disk snapshots: daily, kept seven days, restore only from the dashboard (https://render.com/docs/disks.md).
Render support offers help with restores (same page).

### Render token scoping

Since 2026-11-18 new API keys belong to one workspace and carry fine-grained permissions per resource type (Read, Write, Read & write); "Read only" can be set for all resources at once (https://render.com/changelog/fine-grained-api-keys-now-generally-available.md, https://render.com/docs/api-keys.md).
"Create, update, and delete databases" needs Read & write on Postgres instances, so a key with Read on Postgres cannot delete one.
Delete is not separate from update: a key that may change a service may also delete it.
Classic keys made earlier reach every workspace with the member role's full rights.

---

## Netlify

Host: `https://api.netlify.com/api/v1`, REST JSON, `Authorization: Bearer <token>`.
Spec: https://open-api.netlify.com/swagger.json (version 2.63.1, read 2026-11-24).

| Operation | Wire | Outcome | Undoable first? |
|---|---|---|---|
| Delete Netlify Database | `DELETE /api/v1/sites/{site_id}/database` | **gone**: "Deletes the database and all associated branches and snapshots" | A snapshot (`POST /sites/{site_id}/database/snapshot`) would die with it. Silo would need a logical dump through the Postgres wire, which is the other thread's proxy. |
| Delete database branch, snapshot | `DELETE /sites/{site_id}/database/branch/{branch_id}`, `DELETE /sites/{site_id}/database/snapshot/{snapshot_id}` | **gone** | Snapshot the branch first. |
| Reset branch, restore snapshot | `POST /sites/{site_id}/database/branch/{id}/reset`, `POST /sites/{site_id}/database/snapshot/{id}/restore` (defaults to production) | overwrites data | Snapshot first (`POST /sites/{site_id}/database/snapshot`). |
| Delete site | `DELETE /sites/{site_id}` | **gone** (no restore found, unverified) | No. |
| Delete form | `DELETE /sites/{site_id}/forms/{form_id}` | **gone**: "Deletes a form and all of its submissions" | Read submissions first (`GET /forms/{id}/submissions`). |
| Delete submission | `DELETE /submissions/{submission_id}` | **gone** | Read first. |
| Delete deploy | `DELETE /deploys/{deploy_id}`, `DELETE /sites/{site_id}/deploys/{deploy_id}` | "Marks a deploy for deletion" (window unverified) | |
| Delete Blobs | `DELETE https://api.netlify.com/api/v1/blobs/{site}/{store}/{key}` (path built by `@netlify/blobs` 11.1.11 as `/api/v1/blobs${urlPath}`; exact segments and whether it then follows a signed URL to another host are unverified) | **gone** | Read the blob first. |
| Delete env var | `DELETE /accounts/{account_id}/env/{key}`, `.../value/{id}` | value **gone** | Read first. |
| Delete DNS zone, record | `DELETE /dns_zones/{zone_id}`, `DELETE /dns_zones/{zone_id}/dns_records/{id}`, `PUT /dns_zones/{zone_id}/transfer` | **outage** | Read the zone first. |
| Disable site, unlink repo, rollback, lock | `PUT /sites/{site_id}/disable`, `PUT /sites/{site_id}/unlink_repo` (also deletes deploy keys, webhooks, build hooks), `PUT /sites/{site_id}/rollback`, `POST /deploys/{id}/lock` | **outage** | |
| Hooks, deploy keys, snippets, add-on instances | `DELETE /hooks/{id}`, `DELETE /deploy_keys/{id}`, `DELETE /sites/{id}/build_hooks/{id}`, `DELETE /sites/{id}/snippets/{id}`, `DELETE /sites/{id}/services/{addon}/instances/{id}` | **outage** | |
| Cancel account, remove member | `DELETE /accounts/{account_id}`, `DELETE /{account_slug}/members/{member_id}` | **gone** / **outage** | |

### Netlify token scoping

Personal access tokens have a name, an expiry and, for SAML SSO teams, a choice to reach that team; no other scope (https://docs.netlify.com/api-and-cli-guides/api-guides/get-started-with-api.md, https://netlify.com/auth.md).
No read-only token was found (unverified that none exists).
The Netlify MCP server uses OAuth at `mcp.netlify.com`.

---

## What Silo could do with this

These are inputs for the design, not decisions.

- For each host above, a list of facts a rule can match: method plus path template for REST; for GraphQL, the root field names of the parsed mutation document, plus selected variables (`patch.*.isDeleted` on Railway).
- Railway needs a GraphQL rule that also looks inside the `patch` JSON of `environmentPatchCommit` and `environmentStageChanges`, and treats `environmentPatchCommitStaged` and `environmentApplyChangeSet` as opaque deletes.
- "Hold every DELETE" is a poor default on Heroku (dyno restart is DELETE) and misses Vercel Global Config item deletes (PATCH), Railway (all POST), Render env replace (PUT), and Heroku config unset (PATCH).
- Undo-first that would really work today: Fly volume snapshot before `DELETE .../volumes/{id}`; Fly MPG fork before cluster delete; Railway PITR restore to a new service before `volumeDelete` on a PITR-enabled DB; Render Postgres recovery instance before delete; Vercel Global Config read-and-keep; env var read-and-keep on every provider where values are readable.
- Undo-first that does not work: a backup on the same resource that the call deletes (Railway volume backups, Netlify DB snapshots, Fly app snapshots on app destroy).

## Could not verify

- Whether Railway's 48 h soft delete also covers volumes deleted through `environmentPatchCommit`, and whether `projectDelete` (not scheduled) still deletes at once.
- Whether Railway project tokens and Editor-role OAuth tokens can call `volumeDelete`.
- Whether Railway runs mutations sent over its graphql-ws websocket.
- Whether Railway deletes a service's volume when `serviceDelete` runs.
- The user-side REST endpoint behind `vercel integration resource delete`.
- Whether Fly app deploy tokens can delete volumes, and whether each Machines API route checks the macaroon `d` bit.
- Whether Fly Managed Postgres backups survive `DELETE /v1/postgres/{id}`.
- Render disk snapshots and Postgres exports after the resource is deleted.
- Netlify Blobs exact delete path and Netlify site deletion recovery.
- Heroku Data API backup capture path (the CLI file for `pg:backups:capture` was not read), and the hosts behind `heroku run`.
- No credential was found in anything read.
