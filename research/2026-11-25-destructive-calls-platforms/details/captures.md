# The 121 commands, and what each sent

Each command below was written and labelled in `proto/capture/commands.tsv` (commit ff0ce02)
before any CLI ran. After that, only the syntax of nine commands was fixed (a flag the CLI wanted,
`--confirm-name` for Vercel); no label changed. Each CLI ran against `proto/capture/server2.py`, a
local server with a test certificate authority, `/etc/hosts` pointing the provider's API hosts at
it, and answers built from the provider's own schema. The CLIs and versions: Railway CLI 5.75.0,
flyctl 0.5.17 (built from source), Vercel CLI 66.6.0, Wrangler 4.168.0, Supabase CLI 3.4.0.

The label of a command goes to every write it sent. Reads, and Railway's `cliEventTrack`
telemetry, are labelled allow (`proto/capture/labels.py`). "blind" is the rule set frozen before
the captures (`proto/detector/rules-blind.json`), "tuned" adds the three rules and the Vercel
paths written after them (`proto/detector/rules.json`). Commands labelled allow that sent no write
are left out of the table; their reads are in the counts.

| Command | Label | Step on the wire | #353 | blind | tuned |
|---|---|---|---|---|---|
| `railway volume delete --volume pg-data --yes` | hold:recoverable | `mutation volumeDelete` | hold | hold | hold |
| `railway volume detach --volume pg-data --yes` | hold:outage | `mutation volumeInstanceUpdate` | allow | allow | hold |
| `railway volume update --volume pg-data --name pg-data-2` | allow | `mutation volumeUpdate` | allow | allow | allow |
| `railway environment delete staging --yes` | hold:gone | `mutation environmentDelete` | hold | hold | hold |
| `railway environment new preview` | allow | `mutation environmentCreate` | allow | allow | allow |
| `railway service delete --service api --yes` | hold:gone | `mutation serviceDelete` | hold | hold | hold |
| `railway variable delete DATABASE_URL --service api` | hold:gone | `mutation variableDelete` | hold | hold | hold |
| `railway variable set FOO=bar --service api` | either | `mutation variableCollectionUpsert` | allow | allow | allow |
| `railway domain delete api.example.com --service api --yes` | hold:outage | `mutation customDomainDelete` | hold | hold | hold |
| `railway tcp-proxy delete tcpproxy-1 --service api --yes` | hold:outage | `mutation tcpProxyDelete` | hold | hold | hold |
| `railway tcp-proxy create --port 5432 --service api` | either | `mutation environmentStageChanges` | allow | allow | allow |
| `railway bucket delete --bucket assets --yes` | hold:gone | `mutation environmentStageChanges` | hold | hold | hold |
| `railway bucket credentials --bucket assets --reset --yes` | hold:outage | `mutation bucketCredentialsReset` | hold | hold | hold |
| `railway bucket create assets2 --region ams` | allow | `mutation bucketCreate`<br>`mutation environmentStageChanges` | allow, allow | allow, allow | allow, allow |
| `railway project delete --project prod-app --yes` | hold:recoverable | `mutation projectScheduleDelete` | hold | hold | hold |
| `railway down --service api --yes` | hold:outage | `mutation deploymentRemove` | hold | hold | hold |
| `railway redeploy --service api --yes` | deploy | `mutation deploymentRedeploy` | allow | allow | allow |
| `railway restart --service api --yes` | either | `mutation deploymentRestart` | allow | allow | allow |
| `railway scale --service api us-west2=0` | hold:outage | `mutation environmentPatchCommit` | allow | allow | hold |
| `railway usage limit remove --yes` | hold:money | `mutation usageLimitRemove` | hold | hold | hold |
| `railway ssh keys add --name laptop` | hold:exposed | `mutation sshPublicKeyCreate` | allow | hold | hold |
| `railway flag delete checkout.v2` | either | `mutation signalDelete` | hold | hold | hold |
| `flyctl apps destroy my-app --yes` | hold:gone | `DELETE /v1/apps/my-app` | hold | hold | hold |
| `flyctl volumes destroy vol_123 -a my-app --yes` | hold:recoverable | `DELETE /v1/apps/my-app/volumes/vol_123` | hold | hold | hold |
| `flyctl machine destroy 3d8d1 -a my-app --force` | hold:gone | `DELETE /v1/apps/my-app/machines/3d8d1` | hold | hold | hold |
| `flyctl machine stop 3d8d1 -a my-app` | hold:outage | `POST /v1/apps/my-app/machines/3d8d1/stop` | allow | hold | hold |
| `flyctl machine kill 3d8d1 -a my-app` | hold:outage | `POST /v1/apps/my-app/machines/3d8d1/signal` | allow | hold | hold |
| `flyctl machine suspend 3d8d1 -a my-app` | hold:outage | `POST /v1/apps/my-app/machines/3d8d1/suspend` | allow | hold | hold |
| `flyctl machine cordon 3d8d1 -a my-app` | hold:outage | `POST /v1/apps/my-app/machines/3d8d1/cordon` | allow | hold | hold |
| `flyctl machine start 3d8d1 -a my-app` | allow | `POST /v1/apps/my-app/machines/3d8d1/start` | allow | allow | allow |
| `flyctl machine restart 3d8d1 -a my-app` | either | `POST /v1/apps/my-app/machines/3d8d1/restart` | allow | allow | allow |
| `flyctl secrets unset DATABASE_URL -a my-app --stage` | hold:gone | `POST /v1/apps/my-app/secrets` | allow | hold | hold |
| `flyctl secrets set FOO=bar -a my-app --stage` | either | `POST /v1/apps/my-app/secrets` | allow | allow | allow |
| `flyctl scale count 0 -a my-app --yes` | hold:outage | `DELETE /v1/apps/my-app/machines/3d8d1` | hold | hold | hold |
| `flyctl ips release 203.0.113.7 -a my-app` | hold:outage | `DELETE /v1/apps/my-app/ip_assignments/203.0.113.7` | hold | hold | hold |
| `flyctl certs remove example.com -a my-app --yes` | hold:outage | `DELETE /v1/apps/my-app/certificates/example.com` | hold | hold | hold |
| `flyctl certs add example.com -a my-app` | allow | `POST /v1/apps/my-app/certificates/acme` | allow | allow | allow |
| `flyctl tokens create deploy -a my-app` | hold:exposed | `mutation createLimitedAccessToken` | allow | hold | hold |
| `flyctl orgs invite acme bob@example.com` | hold:exposed | `mutation createOrganizationInvitation` | allow | hold | hold |
| `flyctl apps move my-app --org other --yes` | hold:exposed | not reached: flyctl needs its WireGuard agent before moving an app; only validateWireGuardPeers was sent | | | |
| `flyctl apps create new-app --org acme` | allow | `POST /v1/apps` | allow | allow | allow |
| `flyctl volumes create data -a my-app --region cdg --size 1 --yes` | allow | `POST /v1/apps/my-app/volumes` | allow | allow | allow |
| `flyctl volumes snapshots create vol_123 -a my-app` | allow | `POST /v1/apps/my-app/volumes/vol_123/snapshots` | allow | allow | allow |
| `flyctl volumes extend vol_123 -a my-app -s 20 --yes` | either | `PUT /v1/apps/my-app/volumes/vol_123/extend` | allow | allow | allow |
| `flyctl mpg destroy pg123 --yes` | hold:gone | not reached: flyctl needs a macaroon token for Managed Postgres; nothing was sent | | | |
| `vercel remove my-app --yes` | hold:gone | `DELETE /projects/prj_1` | hold | hold | hold |
| `vercel project rm my-app --confirm-name my-app --scope acme` | hold:gone | `DELETE /v2/projects/my-app` | hold | hold | hold |
| `vercel project add new-app` | allow | `POST /v1/projects` | allow | allow | allow |
| `vercel env rm DATABASE_URL production --yes` | hold:gone | `DELETE /v10/projects/prj_1/env/env_1` | hold | hold | hold |
| `vercel env add FOO production --value bar --yes` | allow | `POST /v10/projects/prj_1/env` | allow | allow | allow |
| `vercel domains rm example.com --yes` | hold:outage | `DELETE /v3/domains/example.com` | hold | hold | hold |
| `vercel dns rm rec_1 --yes` | hold:outage | `DELETE /v3/domains/example.com/records/rec_1` | hold | hold | hold |
| `vercel alias rm my-app.example.com --yes` | hold:outage | `DELETE /now/aliases/alias_1` | hold | hold | hold |
| `vercel certs rm cert_1` | hold:outage | not reached: the Vercel CLI asks for confirmation and has no flag to skip it | | | |
| `vercel promote dpl_2 --yes` | deploy | `POST /v10/projects/prj_1/promote/dpl_2` | allow | allow | allow |
| `vercel rollback dpl_1 --yes` | deploy | `POST /v9/projects/prj_1/rollback/dpl_2` | allow | allow | allow |
| `vercel teams invite bob@example.com --scope acme` | hold:exposed | `POST /teams/team_1/members` | allow | allow | hold |
| `vercel tokens add ci-token` | hold:exposed | `POST /v3/user/tokens` | allow | hold | hold |
| `vercel webhooks create https://hooks.example.net/x --event deployment.created` | hold:exposed | `POST /v1/webhooks` | allow | hold | hold |
| `vercel drains create --name ext --type logs --url https://logs.example.net/in` | hold:exposed | `POST /v1/drains` | allow | hold | hold |
| `vercel firewall system-bypass add 0.0.0.0/0 --yes` | hold:exposed | `POST /v1/security/firewall/bypass` | allow | hold | hold |
| `vercel cache purge --yes` | either | `POST /v1/edge-cache/purge-all`<br>`DELETE /v1/data-cache/purge-all` | allow, hold | allow, hold | allow, hold |
| `wrangler d1 delete mydb -y` | hold:gone | `DELETE /client/v4/accounts/acc123/d1/database/11111111-2222-3333-4444-5555555` | hold | hold | hold |
| `wrangler d1 execute mydb --remote --command "DROP TABLE users" -y` | hold:gone | `POST /client/v4/accounts/acc123/d1/database/11111111-2222-3333-4444-5555555` | hold | hold | hold |
| `wrangler d1 execute mydb --remote --command "SELECT * FROM users LIMIT 5" -y` | allow | `POST /client/v4/accounts/acc123/d1/database/11111111-2222-3333-4444-5555555` | allow | allow | allow |
| `wrangler d1 time-travel restore mydb --timestamp 2026-11-24T12:00:00Z` | hold:gone | `POST /client/v4/accounts/acc123/d1/database/11111111-2222-3333-4444-5555555` | allow | hold | hold |
| `wrangler d1 create newdb` | allow | `POST /client/v4/accounts/acc123/d1/database` | allow | allow | allow |
| `wrangler kv namespace delete --namespace-id ns123 -y` | hold:gone | `DELETE /client/v4/accounts/acc123/storage/kv/namespaces/ns123` | hold | hold | hold |
| `wrangler kv key delete session:1 --namespace-id ns123 --remote` | hold:gone | `DELETE /client/v4/accounts/acc123/storage/kv/namespaces/ns123/values/session%` | hold | hold | hold |
| `wrangler kv bulk delete keys.json --namespace-id ns123 --remote -f` | hold:gone | `DELETE /client/v4/accounts/acc123/storage/kv/namespaces/ns123/bulk` | hold | hold | hold |
| `wrangler kv key put session:1 value --namespace-id ns123 --remote` | either | `PUT /client/v4/accounts/acc123/storage/kv/namespaces/ns123/values/session%` | allow | allow | allow |
| `wrangler kv namespace create CACHE` | allow | `POST /client/v4/accounts/acc123/storage/kv/namespaces` | allow | allow | allow |
| `wrangler r2 bucket delete mybucket` | hold:gone | `DELETE /client/v4/accounts/acc123/r2/buckets/mybucket` | hold | hold | hold |
| `wrangler r2 object delete mybucket/backups/db.dump --remote` | hold:gone | `DELETE /client/v4/accounts/acc123/r2/buckets/mybucket/objects/backups/db.dump` | hold | hold | hold |
| `wrangler r2 bucket dev-url enable mybucket -y` | hold:exposed | `PUT /client/v4/accounts/acc123/r2/buckets/mybucket/domains/managed` | allow | hold | hold |
| `wrangler r2 bucket lifecycle add mybucket expire-all --expire-days 1 -y` | hold:gone | `PUT /client/v4/accounts/acc123/r2/buckets/mybucket/lifecycle` | allow | hold | hold |
| `wrangler r2 bucket create newbucket` | allow | `POST /client/v4/accounts/acc123/r2/buckets` | allow | allow | allow |
| `wrangler queues delete jobs` | hold:gone | `DELETE /client/v4/accounts/acc123/queues/q1` | hold | hold | hold |
| `wrangler queues purge jobs --force` | hold:gone | `POST /client/v4/accounts/acc123/queues/q1/purge` | allow | hold | hold |
| `wrangler vectorize delete idx --force` | hold:gone | `DELETE /client/v4/accounts/acc123/vectorize/v2/indexes/idx` | hold | hold | hold |
| `wrangler secret delete API_KEY --name my-worker` | hold:gone | `DELETE /client/v4/accounts/acc123/workers/scripts/my-worker/secrets/API_KEY` | hold | hold | hold |
| `wrangler secret put API_KEY --name my-worker` | either | `PUT /client/v4/accounts/acc123/workers/scripts/my-worker/secrets` | allow | allow | allow |
| `wrangler delete --name my-worker --force` | hold:gone | `DELETE /client/v4/accounts/acc123/workers/services/my-worker` | hold | hold | hold |
| `wrangler pages project delete my-site -y` | hold:gone | `DELETE /client/v4/accounts/acc123/pages/projects/my-site` | hold | hold | hold |
| `wrangler hyperdrive delete hd123` | hold:outage | `DELETE /client/v4/accounts/acc123/hyperdrive/configs/hd123` | hold | hold | hold |
| `wrangler workflows instances terminate my-wf inst1` | hold:outage | `PATCH /client/v4/accounts/acc123/workflows/my-wf/instances/inst1/status` | allow | hold | hold |
| `supabase projects delete abcdefghijklmnopqrst --yes` | hold:gone | `DELETE /v1/projects/abcdefghijklmnopqrst` | hold | hold | hold |
| `supabase branches delete br-1 --project-ref abcdefghijklmnopqrst --yes` | hold:gone | `DELETE /v1/branches/bbbbbbbbbbbbbbbbbbbb` | hold | hold | hold |
| `supabase branches create feature --project-ref abcdefghijklmnopqrst` | either | `POST /v1/projects/abcdefghijklmnopqrst/branches` | allow | allow | allow |
| `supabase functions delete hello --project-ref abcdefghijklmnopqrst --yes` | hold:gone | `DELETE /v1/projects/abcdefghijklmnopqrst/functions/hello` | hold | hold | hold |
| `supabase secrets unset FOO --project-ref abcdefghijklmnopqrst --yes` | hold:gone | `DELETE /v1/projects/abcdefghijklmnopqrst/secrets` | hold | hold | hold |
| `supabase secrets set FOO=bar --project-ref abcdefghijklmnopqrst` | either | `POST /v1/projects/abcdefghijklmnopqrst/secrets` | allow | allow | allow |
| `supabase db query "DROP TABLE users" --remote=abcdefghijklmnopqrst --yes` | hold:gone | `POST /v1/projects/abcdefghijklmnopqrst/database/query` | hold | hold | hold |
| `supabase db query "select count(*) from users" --remote=abcdefghijklmnopqrst --read-only` | allow | `POST /v1/projects/abcdefghijklmnopqrst/database/query` | allow | allow | allow |
| `supabase backups restore --timestamp 1764000000 --remote=abcdefghijklmnopqrst --yes` | hold:gone | `POST /v1/projects/abcdefghijklmnopqrst/database/backups/restore-pitr` | allow | hold | hold |
| `supabase network-restrictions update --db-allow-cidr 0.0.0.0/0 --project-ref abcdefghijklmnopqrst --experimental --yes` | hold:exposed | `POST /v1/projects/abcdefghijklmnopqrst/network-restrictions/apply` | allow | hold | hold |
| `supabase ssl-enforcement update --disable-db-ssl-enforcement --project-ref abcdefghijklmnopqrst --experimental --yes` | hold:exposed | `PUT /v1/projects/abcdefghijklmnopqrst/ssl-enforcement` | allow | hold | hold |
| `supabase postgres-config update --config max_connections=100 --project-ref abcdefghijklmnopqrst --experimental --yes` | either | `PUT /v1/projects/abcdefghijklmnopqrst/config/database/postgres` | allow | allow | allow |
| `supabase storage rm ss:///avatars/u1.png --linked --experimental --yes` | hold:gone | `DELETE /storage/v1/object/avatars` | allow | allow | hold |
| `supabase vanity-subdomains delete --project-ref abcdefghijklmnopqrst --experimental --yes` | hold:outage | `DELETE /v1/projects/abcdefghijklmnopqrst/vanity-subdomain` | hold | hold | hold |
