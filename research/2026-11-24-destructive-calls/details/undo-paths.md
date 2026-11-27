# Making a destructive call undoable: three paths, ready to decide

Research of 24 November 2026, by an agent, for the owner's question: may Silo make a destructive
call undoable before letting it pass? It also covers a fifth outcome, **exposed**.

Every source was read on 2026-11-24 unless a line says otherwise. Rows that rest on the other
catalogues cite them (`paas.md`, `data.md`, `clouds.md`, `saas.md`); those were fact-checked the
same day (`fact-check.md`). Nothing here was run against a real provider with a credential, so
**no pre-call time below was measured**. Times are quoted from the provider, or marked unverified.
The Railway schema and the GitHub GraphQL schema were read directly, with no token.

The three paths:

- **A. Name it.** Silo only allows, holds or denies. The ask names what the provider can bring
  back, and for how long.
- **B. Rewrite.** Silo changes the held request so the provider keeps its own undo, then lets it
  pass.
- **C. Pre-call.** Silo makes a call of its own first, with the user's credential, then lets the
  original pass unchanged.

## What matters most

1. **B is the best path where it exists, and it exists for few operations.** Of the 40 below, B
   gives a real undo for 11: RDS, Aurora, ElastiCache and Redshift final snapshots, Secrets
   Manager and KMS waiting windows, Cloud SQL final backup, Atlas `retainBackups`, Railway
   `projectDelete` to `projectScheduleDelete`, and Gmail and Drive delete to trash. The provider
   does the copying, waits for it, and expires it on its own clock. Silo makes no second call and
   leaves nothing behind.
2. **B cannot be done without signing on AWS, and breaks Digest auth on Atlas.** A SigV4 signature
   covers the body, so a rewritten RDS request needs Silo to sign at the boundary (the re-signing
   proposal under review). Atlas's API key auth is HTTP Digest, whose hash covers the request URI
   ([RFC 7616 §3.4.1](https://www.rfc-editor.org/rfc/rfc7616#section-3.4.1)), so adding
   `?retainBackups=true` breaks it unless Silo answers the Digest challenge itself. Atlas service
   accounts (bearer tokens) have no such problem.
3. **Some rewrites change what the agent asked for in a way that breaks the next step.** A secret
   deleted with a 30-day window keeps its name, so a script that deletes and re-creates
   `prod/db-password` then fails. An RDS instance with a final snapshot stays in `deleting` until
   the snapshot is done. B must be offered per operation, with its known breakage in the rule's
   data, never as a blanket "make undoable".
4. **C fails exactly where it is most wanted.** The copy dies with the thing deleted on Railway
   (volume backups), Fly (app destroy deletes volume snapshots), Supabase (project delete removes
   backups), Netlify (DB snapshots), Cloud SQL and Atlas (on-demand backups go with the instance or
   cluster unless kept). On Neon, a backup branch made from the branch being deleted makes the
   delete **fail**: Neon refuses to delete a branch with children. C works well in five places:
   a GitHub backup ref, a D1 Time Travel bookmark, a Cloudflare DNS export, an EC2
   `DeleteOnTermination=false` before a terminate, and a Fly volume snapshot before a volume
   destroy. Each takes one call and, as far as the provider says, seconds.
5. **B and C are not protective in Silo's sense.** `kinds-of-action` says a protective action
   "grants nothing and destroys nothing. A wrong one costs only time." A snapshot costs money every
   month, and it keeps data the user may have meant to erase. So both need you or a rule, never a
   precaution. One kind of rewrite is protective: one that can only make the provider refuse more,
   such as PostgREST `Prefer: handling=strict, max-affected=N`.
6. **npm has a provider hold for publishing.** Staged publishing (generally available since May
   2026) keeps a version hidden until a maintainer approves it with 2FA, and stage-only tokens
   cannot publish directly. A stage-only token swapped in by Silo turns every `npm publish` into a
   held one, with no rewrite at all.
7. **Exposure cannot be undone by any path.** Making something public can be switched back, but a
   copy taken in between stays. Only npm offers a provider-side wait. The rest is a hold.

## The 40 operations

Columns: **A** what the provider brings back and for how long; **B** the exact rewrite, whether it
is honoured, its cost, what can fail; **C** the exact pre-call, how long it takes, its cost, and
whether the copy survives the delete; **Rec.** the path recommended.

"Signs" means the rewrite or pre-call needs Silo to sign AWS requests (re-signing, under review).
"Hold" in Rec. means path A: hold by default and name the provider's undo in the ask.

### Hosting control planes

| # | Operation (wire) | Outcome | A | B | C | Rec. |
|---|---|---|---|---|---|---|
| 1 | Railway `volumeDelete(volumeId)`, `POST backboard.railway.com/graphql/v2` | recoverable 48 h | Soft delete 48 h; restore only through a link emailed to the owner; no restore mutation in the schema. Railway also keeps offsite "disaster backups" (no documented window). ([volumes](https://docs.railway.com/volumes/reference.md), [changelog](https://railway.com/changelog/2026-05-01-undoable-deletes.md), `paas.md`) | None: no argument changes it. | `volumeInstanceBackupCreate`: **does not survive** ("Wiping a volume deletes all backups", [backups](https://docs.railway.com/volumes/backups.md)). Only if PITR is on: `volumeInstancePITRRestore(volumeInstanceId, targetTimestamp, newServiceName)` makes a new service from the archive bucket, which survives. Time unverified (restores data). Cost: a whole new service. | A, hold. C on an explicit yes only, for PITR databases. |
| 2 | Railway `environmentPatchCommit` with `patch.*.<id>.isDeleted=true` (volume, service, bucket) | volume: as #1 (48 h unverified on this path); service, bucket: gone | Volume: as #1, unverified. Bucket: none documented; a PITR archive lives in a bucket. | None. | As #1. | Hold. Silo must parse the `patch` variable (`paas.md`). |
| 3 | Railway `serviceDelete(id, environmentId)` | gone | None documented. | None. | None: no API copy of a service. Silo can read the service's variables first (`variables` query); the copy holds secrets. | Hold. |
| 4 | Railway `projectDelete(id)` | gone (whether it waits 48 h now is unverified) | `projectScheduleDelete` gives 48 h and `projectScheduleDeleteCancel` undoes it. | `mutation { projectDelete: projectScheduleDelete(id: "…") }`. Both return `Boolean!` (schema introspected today), and the alias keeps the response key the client expects. Honoured: it is Railway's own documented mutation ("Deletes a project with a 48 hour grace period"). Cost: none. Can fail: nothing known; the project lingers 48 h. | Not needed. | **B**. |
| 5 | Railway `variableDelete(input)`, `variableCollectionUpsert(input: {replace: true})` | value gone | None. | None. | `query { variables(projectId, environmentId, serviceId) }` (field name from `paas.md`). One read, under a second (unverified). Copy holds secret values. Survives: yes, it is Silo's. | C, read only, kept as a Silo secret, never in the record. |
| 6 | Vercel `DELETE /v9/projects/{idOrName}` | gone | None: deletes deployments, domains, env vars and settings ([docs](https://vercel.com/docs/projects/managing-projects)). | None. | `GET /v9/projects/{id}` and `GET /v10/projects/{id}/env?decrypt=true`. Secret-type values are "write-only after saving" ([docs](https://vercel.com/docs/environment-variables/sensitive-environment-variables)), so they cannot be kept. Copy holds Config values. | Hold. C read as the ask's content. |
| 7 | Vercel `DELETE /v9/projects/{id}/env/{envId}` (`vercel env rm`) | value gone, outage at next deploy | None. | None. | `GET /v1/projects/{id}/env/{envId}` returns the decrypted value for Config type only (spec, read today). One read. | C for Config values (kept as a Silo secret). Hold for Secret type. |
| 8 | Vercel Blob `POST https://vercel.com/api/blob/delete` `{"urls":[…]}` | gone | None ([docs](https://vercel.com/docs/cli/blob)). | None. | Copy each blob (`POST /api/blob/copy`, path unverified) or download it. Time grows with size. Survives if copied to another store. | Hold. |
| 9 | Vercel `DELETE /v1/global-config/{id}` | gone | "Deletion cannot be undone" ([docs](https://vercel.com/docs/cli/storage)). | None. | `GET /v1/global-config/{id}/items`: one read of a small store. Survives: yes. | **C**. |
| 10 | Fly `DELETE /v1/apps/{app}` (`fly apps destroy`) | gone | None: "When you destroy an app, you delete its volumes and volume snapshots" ([docs](https://docs.fly.io/apps/delete)). | None. | A volume snapshot **does not survive**. Forking a volume to another app: unverified. | Hold. |
| 11 | Fly `DELETE /v1/apps/{app}/volumes/{id}` | recoverable | `pending_destroy` 24 h (recovery by Fly, not self-service, unverified); existing snapshots kept until their retention (default 5 days) ([states](https://docs.fly.io/volumes/volume-states), [manage](https://docs.fly.io/volumes/volume-manage)). | None. | `POST /v1/apps/{app}/volumes/{id}/snapshots`. Time unverified. Cost $0.08/GB-month, first 10 GB free ([pricing](https://docs.fly.io/about/pricing)). Survives the volume destroy for its retention; does not survive an app destroy. | **C**, and note the volume ID in the record (needed to restore). |
| 12 | Fly Managed Postgres `DELETE /v1/postgres/{id}` | gone (docs: "not reversible") | `fly mpg list --deleted` exists; restore unverified. | None. | `POST /v1/postgres/{id}/fork`: a separate cluster, survives. Time and cost of a full cluster unverified. | Hold; C on an explicit yes. |
| 13 | Heroku `DELETE /apps/{app}`, `DELETE /apps/{app}/addons/{addon}` | gone on Essential; recoverable on Standard+ by fork within the rollback window | Standard+: fork back within the rollback window ([help](https://help.heroku.com/SPTSN5U6/can-i-recover-a-deprovisioned-production-tier-heroku-postgres-database)). Essential: none. | None. | `pg:backups:capture` (`POST api.data.heroku.com/client/v11/databases/{id}/backups`, path unverified). **Does not survive**: "We delete backups for deprovisioned add-ons after a short grace period" ([docs](https://devcenter.heroku.com/articles/heroku-postgres-backups)). Silo would also have to download the dump, a full copy of the data. | Hold. |
| 14 | Heroku `PUT api.data.heroku.com/client/v11/databases/{id}/reset` (`pg:reset`) | gone (Standard+ rollback unverified for reset) | As #13. | None. | Capture a backup first; it survives a reset, because the add-on stays. Time grows with size (unverified). | C on yes; hold otherwise. |
| 15 | Render `DELETE /v1/postgres/{id}` | gone ("all data will be lost") | PITR 3 days (Hobby) or 7 days (Pro); whether it outlives the delete is unverified ([docs](https://render.com/docs/postgresql-backups.md)). | None. | `POST /v1/postgres/{id}/recovery` makes a separate instance that survives; cost of a database. `POST /v1/postgres/{id}/export` is kept 7 days; survival after delete unverified. | Hold. |
| 16 | Netlify `DELETE /api/v1/sites/{site_id}/database` | gone, "all associated branches and snapshots" | None. | None. | `POST …/database/snapshot` **does not survive**. | Hold. |

### Hosted databases

| # | Operation (wire) | Outcome | A | B | C | Rec. |
|---|---|---|---|---|---|---|
| 17 | Supabase `DELETE api.supabase.com/v1/projects/{ref}` | gone, backups included ([docs](https://supabase.com/docs/guides/platform/backups)) | None. | None. | No API makes an off-site backup (`data.md`). A `pg_dump` needs the database password and takes minutes to hours. | Hold, or deny by default. |
| 18 | Supabase `POST /v1/projects/{ref}/database/query` with DROP, TRUNCATE, DELETE | Free: gone. Paid: recoverable to the last daily backup or PITR point | Pro 7 d, Team 14 d, Enterprise 30 d daily backups; PITR 7/14/28 d ([docs](https://supabase.com/docs/guides/platform/backups)). | Route a read-only statement to `/database/query/read-only` instead: protective, but only for reads. No rewrite keeps data. | None via API. | Hold (shared SQL classifier). |
| 19 | Supabase PostgREST `DELETE <ref>.supabase.co/rest/v1/<table>?<filter>` (also `PATCH`) | Free: gone. Paid: as #18 | As #18. | Add `Prefer: handling=strict, max-affected=N`. PostgREST then refuses with HTTP 400 `PGRST124` when more rows would be hit; `max-affected` "would be ignored with lenient handling" ([preferences](https://github.com/PostgREST/postgrest-docs/blob/main/docs/references/api/preferences.rst)). Silo must merge with a `Prefer` the client sent and drop a client `handling=lenient`. Cost: none. Can fail: a real bulk delete above N fails, which costs only time. The PostgREST version on Supabase: unverified. | None. | **B** (protective). Hold a request with no filter at all. |
| 20 | Supabase Storage `DELETE /storage/v1/object/{bucket}` `{"prefixes":[…]}`, `POST /storage/v1/bucket/{id}/empty` | gone ("permanently removed and not recoverable") | None. | None. | Download each named object first (up to 1000 per request). Time grows with size. | Hold. |
| 21 | Neon `DELETE console.neon.tech/api/v2/projects/{id}` | recoverable 7 days | `POST /projects/{id}/recover` within 7 days; HIPAA projects excluded ([docs](https://neon.com/docs/manage/projects)). | None needed. | None needed. | A. |
| 22 | Neon `DELETE /projects/{id}/branches/{branch_id}` | gone per the branch page ("a permanent action") | None per [branches](https://neon.com/docs/manage/branches.md). A search summary claims a 7-day branch recovery and a `hard_delete` parameter; neither is on the delete page: unverified. | If the `hard_delete` parameter exists, strip it: unverified. | **Not a child branch**: "You cannot delete a branch that has child branches" ([delete](https://neon.com/docs/reference/api/branches/delete-project-branch.md)), so the original delete would fail. Use a snapshot: `POST /projects/{id}/branches/{branch_id}/snapshot`. Cost $0.09/GB-month at full size; Free plan allows 1 manual snapshot, paid 100 ([backup-restore](https://neon.com/docs/guides/backup-restore)). Time unverified. Survives the branch delete: unverified. | Hold. C on yes, once survival is confirmed. |
| 23 | Neon SQL `DROP`/`TRUNCATE`/`DELETE` (`POST api.<region>…neon.tech/sql`, or wire) | recoverable within the history window | Instant restore, history window Free 6 h, Launch up to 7 d, Scale up to 30 d ([docs](https://neon.com/docs/introduction/history-window)). | None. | No call: Silo writes the current time in the record. Restore is `POST /projects/{id}/branches/{id}/restore` with `source_timestamp`; Neon "creates a backup branch automatically before each restore" (`data.md`). | **A**, with the timestamp in the record. |
| 24 | Turso `DELETE api.turso.tech/v1/organizations/{org}/databases/{db}` | recoverable 5 days on paid plans; else gone | Restore within 5 days on paid plans ([docs](https://docs.turso.tech/api-reference/databases/delete.md)). | None. | None. | Hold on Free; A on paid. |
| 25 | Atlas `DELETE cloud.mongodb.com/api/atlas/v2/groups/{g}/clusters/{c}` | gone unless backups kept | Snapshots kept only with `retainBackups=true` or a Backup Compliance Policy. | Add `?retainBackups=true` ([API](https://www.mongodb.com/docs/api/doc/atlas-admin-api-v2/operation/operation-deletegroupcluster.md): "retain backup snapshots for the deleted dedicated cluster"). Not for serverless. Cost: snapshot storage until they expire (price unverified). Can fail: Digest auth (see above); a Compliance Policy then blocks re-using the cluster name. | `POST …/clusters/{c}/backup/snapshots` (unverified path) **does not survive** a delete without `retainBackups`. | **B**. |
| 26 | PlanetScale `DELETE api.planetscale.com/v1/organizations/{o}/databases/{db}` | gone as far as found | Unverified. | None. | Backup API exists in the permission list; path unverified. | Hold. |
| 27 | Upstash `POST https://<db>.upstash.io` `["FLUSHALL"]`, `DELETE api.upstash.com/v2/redis/database/{id}` | FLUSHALL: recoverable to the last daily backup if on; DB delete: gone | Daily backups 1 or 3 days ([docs](https://upstash.com/docs/redis/features/backup)). | None. | A backup-create endpoint: unverified. Survival after DB delete: unverified. | Hold. |

### Clouds

| # | Operation (wire) | Outcome | A | B | C | Rec. |
|---|---|---|---|---|---|---|
| 28 | RDS `Action=DeleteDBInstance` with `SkipFinalSnapshot=true` (and default `DeleteAutomatedBackups`) | gone | Automated backups go with the instance; manual snapshots stay; AWS Support "might" recover up to six days ([guide](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/USER_DeleteInstance.html)). | Set `SkipFinalSnapshot=false`, add `FinalDBSnapshotIdentifier=silo-<id>`, set `DeleteAutomatedBackups=false`. Honoured: these are the API's own parameters ([API](https://docs.aws.amazon.com/AmazonRDS/latest/APIReference/API_DeleteDBInstance.html)). RDS waits on its own ("the status … is `deleting` until the DB snapshot is created"). Signs. Cost: snapshot storage (price not on today's pricing page: unverified). Can fail: not for read replicas ("Can't be specified when deleting a read replica"; they hold no unique data, so leave them); not in states `failed`, `incompatible-restore`, `incompatible-network`; `SnapshotQuotaExceeded`; `DBInstanceAutomatedBackupQuotaExceeded`; `DBSnapshotAlreadyExists` if the name repeats; `KMSKeyNotAccessibleFault`. On error the instance is not deleted, which is a safe failure. | `CreateDBSnapshot` first: minutes ("a few seconds to a few minutes" of I/O suspension on Single-AZ, `clouds.md`). Survives. Slower than B for the same result. | **B**. |
| 29 | RDS `DeleteDBCluster` (Aurora), ElastiCache `DeleteReplicationGroup`, Redshift `DeleteCluster` | gone without a final snapshot | As #28 for Aurora; none for the others (`clouds.md`). | Aurora: same three parameters. ElastiCache: add `FinalSnapshotIdentifier`. Redshift: `SkipFinalClusterSnapshot=false` plus `FinalClusterSnapshotIdentifier`. Signs. Failure modes as #28 (ElastiCache and Redshift details unverified). | Manual snapshot first; minutes. | **B**. |
| 30 | S3 `DELETE /{Key}` or `POST /{Bucket}?delete` on a bucket without versioning | gone | None without versioning ([guide](https://docs.aws.amazon.com/AmazonS3/latest/userguide/DeletingObjectVersions.html)). | None that keeps meaning. | `PUT /{Bucket}?versioning` `<Status>Enabled</Status>`: one call. Deletes then add delete markers. Survives. Cost: every later overwrite and delete keeps a version until a lifecycle rule removes it. Versioning can never be removed, only suspended. Signs. Not useful for a delete that names a `VersionId`. | C on an explicit yes, once per bucket. Hold deletes that name a version. |
| 31 | DynamoDB `X-Amz-Target: DynamoDB_20120810.DeleteTable` | gone without PITR; recoverable 35 days with PITR | With PITR on, a `$DeletedTableBackup` is kept 35 days at no cost (`clouds.md`). | None. | `CreateBackup {TableName, BackupName}`: "processed instantaneously and become available for restore within minutes" ([API](https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_CreateBackup.html)). $0.10/GB-month ([pricing](https://aws.amazon.com/dynamodb/pricing/on-demand/)). Survives (on-demand backups are separate). Silo must wait for `AVAILABLE`, because the table cannot be deleted while a backup is in progress (`clouds.md`). Signs. Untested alternative: turn PITR on first to get the free 35-day backup; whether it takes effect at once is unverified. | C on yes; it may need a hold longer than the client's timeout. |
| 32 | EC2 `Action=TerminateInstances` (root volume `DeleteOnTermination=true` by default) | gone for those volumes | Refused only while `DisableApiTermination` is set. | None. | `Action=ModifyInstanceAttribute&InstanceId=…&BlockDeviceMapping.1.DeviceName=…&BlockDeviceMapping.1.Ebs.DeleteOnTermination=false`, one call per instance ("Modifies the `DeleteOnTermination` attribute for volumes that are currently attached", [API](https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_ModifyInstanceAttribute.html)). Synchronous, no copy made. The volume survives the terminate as an unattached volume, billed as a volume until someone deletes it. Signs. | **C**. |
| 33 | EC2 `Action=DeleteVolume` | gone, or recoverable with a Recycle Bin rule | Recycle Bin keeps volumes that match a retention rule; billed as normal volumes ([docs](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/recycle-bin.html)). | None. | `CreateSnapshot`: point in time at the request, `pending` "several hours" ([guide](https://docs.aws.amazon.com/ebs/latest/userguide/ebs-creating-snapshot.html)). Whether the volume may be deleted while pending: unverified. Survives. | Hold. Suggest a Recycle Bin rule once, at setup. |
| 34 | Secrets Manager `X-Amz-Target: secretsmanager.DeleteSecret` with `ForceDeleteWithoutRecovery=true` | gone | 7 to 30 days with `RestoreSecret` when not forced ([API](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_DeleteSecret.html)). | Remove `ForceDeleteWithoutRecovery`, set `RecoveryWindowInDays=30` (both together is an error). Honoured. Signs. Cost: none found. Can fail: a primary with replicas cannot be deleted either way. **Breaks a delete-then-recreate**: the name stays taken, and `InvalidRequestException` lists "The secret is scheduled for deletion". | None. | **B**, with the name clash said to the agent. |
| 35 | KMS `X-Amz-Target: TrentService.ScheduleKeyDeletion` with `PendingWindowInDays=7` | recoverable 7 to 30 days, then data under the key gone | `CancelKeyDeletion` within the window (`clouds.md`). | Set `PendingWindowInDays=30`. Honoured. Signs. Cost: unverified. Meaning kept: the key is unusable at once in both cases. | None. | **B**. |
| 36 | CloudFormation `Action=DeleteStack` | gone for every resource without `DeletionPolicy: Retain/Snapshot` | Per resource policy (`clouds.md`). | None that works: `RetainResources` is accepted only for stacks in `DELETE_FAILED` (unverified on today's page). | `DescribeStackResources` to list what dies, for the ask. A real pre-call (update the template to `Retain`) takes a stack update: minutes, and changes the stack. | Hold, with the list. |
| 37 | Cloud SQL `DELETE sqladmin.googleapis.com/v1/projects/{p}/instances/{i}` | gone | "Cloud SQL deletes all resources for the instance, including on-demand backups and automated backups"; Customer Care "within 4 days" ([docs](https://docs.cloud.google.com/sql/docs/mysql/delete-instance)). | Add `?enableFinalBackup=true&finalBackupTtlDays=30` ([API](https://docs.cloud.google.com/sql/docs/mysql/admin-api/rest/v1/instances/delete)). Bearer token, so no signing problem. Retention 1 to 365 days. Cost: backup storage (price unverified). Can fail: an instance with replicas cannot be deleted at all. | `POST …/instances/{i}/backupRuns` **does not survive** the delete. | **B**. |
| 38 | Firestore `DELETE firestore.googleapis.com/v1/projects/{p}/databases/{db}` | gone (unverified) | Delete protection off by default; PITR 7 days; survival after delete unverified (`clouds.md`). | None. | `projects.databases.exportDocuments` to a bucket; minutes; survives (in GCS). Cost: storage and reads. Time unverified. | Hold. |

### SaaS and source control

| # | Operation (wire) | Outcome | A | B | C | Rec. |
|---|---|---|---|---|---|---|
| 39 | Cloudflare D1 SQL `POST /client/v4/accounts/{a}/d1/database/{id}/query` (DROP, DELETE, UPDATE) | recoverable 30 days (Paid) or 7 (Free) | Time Travel restore "overwrites the database in place"; history costs nothing extra ([docs](https://developers.cloudflare.com/d1/reference/time-travel/)). | None. | `GET …/d1/database/{id}/time_travel/bookmark` (path from `saas.md`; today's page documents only `wrangler d1 time-travel info`). One read, free. The bookmark is a pointer into Cloudflare's history: it survives the write, not a database delete (#40's undelete then cannot go back before itself). | **C**: the one case where "allow, bookmark kept" can be the default. |
| 40 | Cloudflare DNS `DELETE /zones/{z}/dns_records/{id}`, `POST /zones/{z}/dns_records/batch` with `deletes`/`puts`, `DELETE /zones/{z}` | outage; records lost on a zone delete | None documented. | None. | `GET /zones/{z}/dns_records/export` returns BIND; "Workers-managed records are never exported"; settings are not covered ([API](https://developers.cloudflare.com/api/resources/dns/subresources/records/methods/export/)). One read. Rate limit 1,200 requests per 5 minutes per user, shared with the user's own calls ([limits](https://developers.cloudflare.com/fundamentals/api/reference/limits/)). Copy may hold TXT tokens and DKIM keys. | **C**. |
| 41 | Cloudflare R2 `DELETE /{bucket}/{key}`, `POST /{bucket}?delete`; KV `DELETE …/storage/kv/namespaces/{ns}` | gone (no versioning on R2) | None (`saas.md`). | None. | GET each object or key first; heavy beyond a few. | Hold. Suggest bucket locks at setup. |
| 42 | GitHub force push or branch/tag delete: `POST github.com/{o}/{r}.git/git-receive-pack`, `DELETE /repos/{o}/{r}/git/refs/{ref}`, `PATCH …/git/refs/{ref}` `force:true`, GraphQL `deleteRef`/`updateRefs` | recoverable until GitHub garbage-collects (window undocumented) | Activity API lists `force_push` and `branch_deletion` with shas (`saas.md`). | None that keeps meaning. | `POST /repos/{o}/{r}/git/refs` `{"ref":"refs/silo/<id>","sha":"<old>"}`. "If it doesn't start with 'refs' and have at least two slashes, it will be rejected" ([refs](https://docs.github.com/en/rest/git/refs)); a custom namespace is unverified. One call; time not measured. Counts against 80 content-creating requests per minute and 500 per hour ([limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)); for a burst, one GraphQL `updateRefs` call creates many refs atomically. Survives the ref delete and the force push; **not** a repository delete. Visible to anyone who can read the repo; no expiry. A ruleset that restricts ref creation would refuse it (then hold). | **C**. |
| 43 | GitHub `DELETE /repos/{o}/{r}` | recoverable 90 days, self-service (not inside a non-empty fork network) | ([docs](https://docs.github.com/en/repositories/creating-and-managing-repositories/restoring-a-deleted-repository)) | None. | None needed; record team permissions (not restored). | A. |
| 44 | GitHub `DELETE /repos/{o}/{r}/releases/{id}` | gone for notes and assets | None. | None. | `GET` the release and download assets: time grows with size. | Hold; C for the notes only. |
| 45 | Stripe `DELETE /v1/customers/{id}`, `DELETE /v1/subscriptions/{id}` | customer: gone (card data cannot be re-created); subscription: cannot be reactivated | None ([customers](https://docs.stripe.com/api/customers/delete), [subscriptions](https://docs.stripe.com/api/subscriptions/cancel)). Agent-tagged keys: Stripe's own approval for subscription cancels, 14 days ([approvals](https://docs.stripe.com/account/approvals)). | Subscription: `POST /v1/subscriptions/{id}` `cancel_at_period_end=true`, undone by setting it back to false. **Changes meaning**: the customer keeps service until the period ends, and proration or invoicing at cancel does not happen. Customer: none. | `GET` the object and its subscriptions; the copy holds personal data and is not a real undo. | Hold; Stripe approvals where the key allows. |
| 46 | Gmail `DELETE /gmail/v1/users/{u}/messages/{id}`, `POST …/messages/batchDelete` | gone (consumer); Workspace admin restore 25 days (unverified for API deletes) | None for consumer accounts (`saas.md`). | `delete` to `POST …/messages/{id}/trash`; `batchDelete {ids}` to `POST …/messages/batchModify {ids, addLabelIds:["TRASH"]}`. Honoured: same scope suffices. Response: `batchModify` returns empty like `batchDelete`; `trash` returns a Message where `delete` returns empty. Meaning: mail stays 30 days in Trash, using quota. | None. | **B** as a rule option; hold otherwise. |
| 47 | Drive `DELETE /drive/v3/files/{id}` | gone for consumer accounts (skips Trash) | Workspace admin restore 25 days (`saas.md`). | `PATCH /drive/v3/files/{id}` `{"trashed": true}`: recoverable 30 days. Returns a File where `delete` returns empty. Whether trashing needs the same rights as deleting on shared drives: unverified. | None. | **B** as a rule option. |
| 48 | Slack `POST /api/chat.delete` | gone | None documented. | None. | `conversations.history` read first; it cannot be re-posted as the original author and time. | Hold. |

The list has 48 rows because some providers needed two. The 40 that matter most are rows 1–7,
9–25, 28–35, 37, 39–40, 42–43, 45–47.

### Count

| Path recommended | Rows |
|---|---|
| B | 4, 19, 25, 28, 29, 34, 35, 37, 46, 47 (and the npm case below) |
| C | 5, 7, 9, 11, 32, 39, 40, 42 |
| A (provider undo is enough) | 21, 23, 24 (paid), 43 |
| Hold, nothing better | the rest, with C or B offered on an explicit yes where a column names one |

## Bad paths

### Path A

- **The named undo is wrong.** The ask says "Railway keeps it 48 h" from the rule set's data. If
  the provider changes its policy, or the account's plan lacks the feature (Supabase Free, Turso
  Free, Neon history beyond the plan's window), the user says yes on a false promise. The rule set
  must carry the plan condition, and Silo should read it where an API gives it (Neon's history
  window, Atlas's backup policy). Otherwise the ask must say "on paid plans".
- **The undo needs someone else.** Railway's restore is a link in the owner's email; Fly's
  `pending_destroy` and AWS Support's six days need the provider. The ask must say who can undo,
  not only that someone can.
- **Fits the rules as they stand.** Hold, allow and deny are taken actions. Naming the undo is
  data the rule carries (`rule-carries-its-data`). Nothing new to decide.

### Path B

- **The rewrite changes meaning.** Secrets Manager keeps the name 30 days, so a delete-then-create
  fails. RDS stays `deleting` until the snapshot exists, so a script that polls for "gone" waits
  longer. Gmail's `trash` returns a Message where `delete` returned nothing. Stripe's
  `cancel_at_period_end` keeps the customer served. Each rewrite needs its breakage written in the
  rule set, and the agent must be told.
- **The provider refuses the rewrite.** RDS refuses a final snapshot for read replicas and for
  failed instances; Cloud SQL refuses to delete an instance with replicas either way. When the
  rewritten request fails, nothing is deleted. That is a safe failure, but the agent must not then
  send the original form: Silo should hold a retry of the unrewritten request.
- **Signatures.** AWS needs boundary signing; Atlas Digest needs Silo to answer the challenge.
  Without them, B is impossible on those hosts and the request must be held.
- **Cost unasked.** A final snapshot of a 500 GB database is billed every month until someone
  deletes it. A rule that rewrites must say the cost in the ask or the rule's text, and prefer the
  provider's expiry (`finalBackupTtlDays`, the recovery window) to an open-ended copy.
- **A copy the user wanted gone.** A user deleting data for a legal erasure request does not want
  a snapshot kept. B must never run on a precaution's opinion, and the ask must offer "delete it
  for good".
- **The agent deletes the copy next.** `DeleteDBSnapshot silo-…` is one more call. Silo knows the
  copy's name, a fact, so the same rule set should hold any delete of a copy it caused to exist.
- **The record must say:** the original operation and the parameters it sent, each change Silo
  made (old and new value), the copy's identifier, where it lives, when it expires, and the
  provider's answer. `no-silent-change-to-traffic` already requires that the change is recorded.
- **The agent is told:** a line in the response it already expects is not possible without a
  second change. So Silo says it in the record and the app, and the provider's own response shows
  it (RDS returns the snapshot name, Secrets Manager a `DeletionDate` 30 days out). A rewrite the
  agent does not know about can surprise its next step; the rule set should list the changed fields.
- **Rate limits.** None added: B sends the same one request.

### Path C

- **The pre-call fails.** The provider refuses (GitHub ruleset on ref creation, Neon snapshot
  quota of 1 on Free, DynamoDB `BackupInUseException`), or times out. Then Silo must not let the
  original pass as "made undoable". It falls back to a hold, and says why in the ask.
- **The pre-call makes the original fail.** A Neon child branch blocks the branch delete; a
  DynamoDB backup in progress blocks `DeleteTable` until it completes. Each C entry must name the
  call that waits for the copy to be ready, and the hold must last that long. That can exceed the
  client's own timeout (60 s for `aws`, `cli-capture.md`), so the late-answer design is needed.
- **The copy does not survive.** Rows 1, 10, 13, 16, 25 and 37 show copies that die with the
  thing deleted. A C entry without a stated, sourced survival answer must not be offered.
- **Cost unasked.** A DynamoDB backup costs $0.10/GB-month; a Neon snapshot $0.09; a Fly snapshot
  $0.08 beyond 10 GB; a kept EBS volume the full volume price. Same rule as B: on your yes or a
  rule you wrote, with the cost in the ask.
- **A copy left behind forever.** GitHub refs, EBS snapshots, kept volumes and DynamoDB backups
  have no expiry. Deleting them later is a destructive action by Silo, which needs you or a rule.
  So each C copy needs an end chosen at creation: a provider TTL where one exists, otherwise an
  entry in the app listing copies Silo made, for you to remove.
- **A copy holds secrets or personal data.** Vercel Config values, Railway variables, a Stripe
  customer, a DNS zone with DKIM keys. The record never holds a secret's real value, so such a copy
  must go into Silo's secret store, or not be made. A copy that lands in the provider (a ref, a
  snapshot) is readable by everyone who can read the original; a ref under `refs/silo/` in a
  public repo is public.
- **Rate limits.** Silo's call spends the user's budget. A `terraform destroy` or a loop of 200
  branch deletes makes 200 pre-calls; GitHub allows 80 content-creating requests a minute and
  Cloudflare 1,200 requests in 5 minutes. Batch where the API allows (GraphQL `updateRefs`), and
  otherwise one pre-call per burst ask.
- **The record must say:** Silo's own request (method, path, operation) as a separate entry,
  marked as Silo's, its result and the copy's identifier, then the original request.
- **The agent is told:** nothing changes in its response. The copy is shown in the app, and the
  copy's delete is held by the same rule.
- **Fits the rules?** C does not change the agent's request, so `what-silo-changes-in-traffic`
  is not touched. It adds traffic Silo originates with the user's credential, which no decision
  covers today. README recommendation 6 already proposes "one read call by Silo before the ask".
  A read pre-call (rows 5, 7, 9, 39, 40) is close to that. A write pre-call (a ref, a snapshot, a
  setting) is new.

### What kind each path is

`kinds-of-action` (taken) files every action as granting, destructive or protective, and a
protective one "grants nothing and destroys nothing. A wrong one costs only time."

| Path | Kind | Why | Who may trigger it |
|---|---|---|---|
| A | protective (a hold, plus data in the ask) | Nothing is sent that the agent did not send. | you, a rule, a precaution |
| B that can only make the provider refuse more (`max-affected`, a dry run) | protective | The worst case is a legitimate request refused, which costs time. | you, a rule, a precaution |
| B that makes the provider keep something | not protective; propose **granting** | It lets a destructive request pass (the allow is granting), and the copy costs money and keeps data, which is more than time. It destroys nothing. | you, or a rule (`grant-needs-a-fact-or-you`) |
| C read (keep a value Silo read) | protective if the copy goes to Silo's secret store; otherwise not | Costs a request; holds data. | you, a rule; a precaution only for reads that hold no secret |
| C write (ref, snapshot, setting) | not protective; propose **granting** | It creates a resource with the user's credential, which costs money and can block the original (Neon). | you, or a rule |

So the proposal in the README holds, with one change: "let it pass, made undoable" is offered as
an action of a rule or a choice in the ask, filed as granting, and a precaution never triggers it.
A rule triggers it on facts: the operation, its parameters (`SkipFinalSnapshot=true`), the host.
That fits `rule-reads-only-facts`. The ask gets a third answer beside yes and no: "Yes, made
undoable", with the cost and the copy's end date. Only the refusal-only rewrites (`max-affected`)
can join the protective actions a precaution may take.

### Challenge to a decision

`what-silo-changes-in-traffic` (in force) says Silo changes model traffic only by the secret
swap. It is written about model traffic. Nothing decides what Silo may change in other traffic,
yet `no-silent-change-to-traffic` assumes changes happen. Case: an RDS delete with
`SkipFinalSnapshot=true` reaches the boundary, and Silo, which already signs AWS requests under
the proposal under review, is the party building the final bytes. Proposed in its place: a
decision for non-model traffic that lists the changes allowed. The secret swap and re-signing, a
rewrite named in a rule the user accepted (path B), and a header that can only make the provider
refuse (`max-affected`). Each is recorded.

## The "exposed" outcome

Operations that lose nothing but make something readable by anyone. Switching back is possible
for most; a copy taken in between stays. No pre-call un-copies anything, so C does not apply. B
applies once (npm). The rest is a hold.

| Operation | Wire | Can the provider take it back? | Silo |
|---|---|---|---|
| GitHub repository made public | REST `PATCH api.github.com/repos/{o}/{r}` with `"visibility":"public"` or `"private":false` (`saas.md`). GraphQL: **no mutation changes an existing repository's visibility**; `UpdateRepositoryInput` has no `visibility` field (schema read today). GraphQL `createRepository` and `cloneTemplateRepository` take `visibility: PUBLIC` for a new one, and REST `POST /user/repos` takes `private:false`; pushing private code to such a repo exposes it too. | Partly. It can be made private again, but "Current forks will remain public", stars and watchers are erased, and Actions history and logs were visible meanwhile ([visibility](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility)). | Hold always; the body field is the fact. |
| Gist made public | `POST api.github.com/gists` with `"public": true`. `PATCH /gists/{id}` takes only `description` and `files`, so an existing secret gist cannot be made public through the API ([gists](https://docs.github.com/en/rest/gists/gists)). | Delete the gist; copies stay. | Hold `POST /gists` with `public:true`. |
| S3 public access block removed | `DELETE /?publicAccessBlock` on `{Bucket}.s3.amazonaws.com` ([API](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeletePublicAccessBlock.html)); `PUT /?publicAccessBlock` with any setting `false`; account level through S3 Control (path unverified). "Removing a block public access setting causes a bucket or object with a public policy or ACL to again be publicly accessible" ([BPA](https://docs.aws.amazon.com/AmazonS3/latest/userguide/access-control-block-public-access.html)). | Turn it back on; copies stay. | Hold. Signs. |
| S3 bucket policy or ACL public | `PUT /?policy` with a statement S3 rates public (`"Principal": "*"` without a fixed-value condition, same page); `PUT /?acl` or `PUT /{Key}?acl` with `x-amz-acl: public-read` or a grant to `AllUsers`/`AuthenticatedUsers`; `PUT /{Key}` with `x-amz-acl: public-read`. | Same. | Hold. Silo can apply S3's own published rule for "public" to the policy JSON, which is a fact. |
| GCS `allUsers` binding | `PUT storage.googleapis.com/storage/v1/b/{bucket}/iam` with a member `allUsers` or `allAuthenticatedUsers` ([API](https://docs.cloud.google.com/storage/docs/json_api/v1/buckets/setIamPolicy)); object ACLs likewise. Public access prevention overrides such bindings while enforced; they come back if it is lifted ([docs](https://docs.cloud.google.com/storage/docs/public-access-prevention)). | Remove the binding; copies stay. | Hold; also hold lifting public access prevention. |
| Supabase Storage bucket public | `PUT <ref>.supabase.co/storage/v1/bucket/{id}` `{"public": true, …}`; `POST /storage/v1/bucket` with `"public": true` (`@supabase/storage-js` 2.119.2 source). "Public buckets don't require an authorization token to download objects". | Set it back to false; CDN caching of public objects: unverified. | Hold. |
| R2 public bucket | `PUT /client/v4/accounts/{a}/r2/buckets/{b}/domains/managed` `{"enabled": true}` for r2.dev ([API](https://developers.cloudflare.com/api/resources/r2/subresources/buckets/subresources/domains/subresources/managed/methods/update/)); a custom domain connects through the dashboard or its API (path unverified). | Disable; copies stay ([docs](https://developers.cloudflare.com/r2/buckets/public-buckets/)). | Hold. |
| Vercel deployment protection off | `PATCH api.vercel.com/v9/projects/{idOrName}` with `"ssoProtection": null`, `"passwordProtection": null` or `"trustedIps": null` (all nullable in the OpenAPI spec read today); `PATCH /v1/projects/{idOrName}/protection-bypass` makes a shareable bypass secret (`paas.md`). | Turn it back on; copies stay. | Hold. |
| Firebase or Firestore rules opened | `POST firebaserules.googleapis.com/v1/projects/{p}/rulesets` with the source, then `PATCH /v1/projects/{p}/releases/{release}` with a new `rulesetName` ([API](https://firebase.google.com/docs/reference/rules/rest/v1/projects.releases/patch)); release names such as `cloud.firestore`: unverified. | Release the earlier ruleset; whether old rulesets are always kept: unverified. | Hold every release update. Whether a ruleset is "open" is a judgement on rules source, so a rule cannot read it as a fact; it can only hold all of them. |
| Cloudflare Access application deleted | `DELETE /client/v4/accounts/{a}/access/apps/{app_id}` or `/zones/{z}/access/apps/{app_id}` ([API](https://developers.cloudflare.com/api/resources/zero_trust/subresources/access/subresources/applications/methods/delete/)). The app behind it becomes reachable by anyone. | Re-create it; a new application gets a new audience tag (unverified), so services that check the tag break. C read: `GET` the application first. | Hold; C read so it can be re-created. |
| Netlify password removed | `PATCH api.netlify.com/api/v1/sites/{site_id}` with the site's `password` field (in the `site` model of the spec, 2.63.1); how removal is expressed (empty string or null) and newer team-level visitor access: unverified. | Set it again; copies stay. | Hold changes to `password`. |
| npm package published | `PUT registry.npmjs.org/{escapedName}` with a JSON body holding `versions`, `dist-tags`, `access` and `_attachments` (the tarball, base64) (`libnpmpublish` 12.0.1 source). `access: "public"` matters for scoped packages. | Only within 72 hours if nothing depends on it; after that only with under 300 downloads in the last week, one owner and no dependents; "Once `package@version` has been used, you can never use it again" ([policy](https://docs.npmjs.com/policies/unpublish)). Mirrors and caches already have it. | **B, or a token**: staged publishing sends the same body to `POST /-/stage/package/{escapedName}` (same source); the version stays hidden until a maintainer approves with 2FA ([staged](https://docs.npmjs.com/staged-publishing)); a new package still gets a public `0.0.0-stage` placeholder. Better than a rewrite: a stage-only token, which "npm rejects direct `npm publish` attempts" with ([changelog](https://github.blog/changelog/2026-09-18-stage-only-npm-tokens-for-safer-automation)), swapped in by Silo's existing secret swap. Whether the registry accepts a rewritten PUT as a stage: unverified. |
| Docker Hub repository public | `docker push` to `registry-1.docker.io` creates a missing repository "with the default repository privacy for that namespace" ([docs](https://docs.docker.com/docker-hub/repos/settings)). Create: `POST hub.docker.com/v2/namespaces/{ns}/repositories` ([API](https://docs.docker.com/reference/api/hub/latest/)); its privacy field, and an API to change privacy later, are not in today's reference: unverified. | Make it private; pulled images stay. | Hold a push that creates a repository in a public-default namespace; the namespace default is readable before. |
| Google Drive "anyone with the link" | `POST www.googleapis.com/drive/v3/files/{fileId}/permissions` with `{"type":"anyone","role":"reader"}` ([API](https://developers.google.com/workspace/drive/api/reference/rest/v3/permissions/create), [Permission](https://developers.google.com/workspace/drive/api/reference/rest/v3/permissions)); `allowFileDiscovery` applies to `domain` and `anyone`. | `DELETE /drive/v3/files/{id}/permissions/{permissionId}`; copies stay. | Hold `type: anyone`. |

Two things follow for the rule set. First, "exposed" belongs with "gone" for the default hold, as
the README proposes. Second, several exposures are new public places, not changed settings (a
public gist, a new public repository, an npm publish, a Docker push). They are a way out for data
through a host the computer is allowed to reach, so the same rule also covers exfiltration.

## Could not verify

- How long any pre-call takes: none was measured, because no real credential was used.
- Neon: whether deleted branches are recoverable and a `hard_delete` parameter exists (a search
  summary says so; the API page does not); whether a snapshot survives its branch's delete.
- Whether a GitHub ref under `refs/silo/` is accepted by the REST API.
- RDS, Cloud SQL, Atlas, KMS and Secrets Manager costs for what is kept.
- Whether turning DynamoDB PITR on right before a `DeleteTable` produces the 35-day backup.
- Whether an EBS volume can be deleted while its snapshot is pending.
- Railway: whether `projectDelete` already waits 48 hours; time of a PITR restore.
- Docker Hub's privacy API, Netlify's password field semantics, Firebase release names and ruleset
  retention, R2 custom domain path, npm's acceptance of a rewritten stage.
- CloudFormation `RetainResources` limits on today's page; ElastiCache and Redshift failure modes.

No credential was found in anything read.
