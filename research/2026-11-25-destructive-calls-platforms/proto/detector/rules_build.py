"""Builds rules.json: #353's rule set with the APIs of Railway, Fly.io, Vercel, Supabase and
Cloudflare rewritten from each provider's whole documented surface (../surface/labels/).

The other APIs of #353 (GitHub, Heroku, Stripe, Neon, Turso, Gmail, AWS, MCP) are copied unchanged.
#353's rules for the five APIs are kept, with their ids, sentences and outcomes, and new rules are
added after them. A new outcome word, "exposed", marks calls that give access or data to someone
new (the challenge in #353 details/detector.md).

Run: python3 rules_build.py   (writes rules.json next to it)
"""
import copy, json, os

HERE = os.path.dirname(os.path.abspath(__file__))
OLD = os.path.join(HERE, "..", "..", "..", "2026-11-24-destructive-calls", "proto", "detector")
OLDP = "../../../2026-11-24-destructive-calls/proto/detector/"


def gql(rid, field, outcome, say, arg=None, action="hold"):
    m = {"type": "mutation", "field": field}
    if arg:
        m["arg"] = arg
    r = {"id": rid, "match": {"graphql": m}, "outcome": outcome, "say": say}
    if action != "hold":
        r["action"] = action
    return r


def gql_allow(rid, fields, why):
    return {"id": rid, "match": {"graphql": {"type": "mutation", "field_in": sorted(fields)}},
            "outcome": None, "action": "allow", "say": why}


def rest(rid, method, paths, outcome, say, body=None, query=None, action="hold"):
    m = {"method": method, "path": paths}
    if body:
        m["body"] = body
    if query:
        m["query"] = query
    r = {"id": rid, "match": {"rest": m}, "outcome": outcome, "say": say}
    if action != "hold":
        r["action"] = action
    return r


def allow(rid, method, paths, why):
    return rest(rid, method, paths, None, why, action="allow")


# ------------------------------------------------------------------ Railway (GraphQL)
RW = [
    # gone
    gql("railway.cloudAgentDelete", "cloudAgentDelete", "gone", "deletes the Railway cloud agent {id}"),
    gql("railway.cloudAgentCheckpointDelete", "cloudAgentCheckpointDelete", "gone", "deletes a Railway cloud agent checkpoint {id}"),
    gql("railway.disablePitrForHaCluster", "disablePitrForHaCluster", "gone", "switches off point-in-time recovery of a Railway database cluster, dropping its recovery history"),
    gql("railway.signalDelete", "signalDelete", "gone", "deletes a Railway signal {input.id|id} and its audit log"),
    gql("railway.signalReplace", "signalReplace", "gone", "replaces a Railway signal's type and default and clears all its rules"),
    gql("railway.templateDelete", "templateDelete", "gone", "deletes the Railway template {id}"),
    gql("railway.updateServiceEdgeRules", "updateServiceEdgeRules", "gone", "replaces every edge rule of the Railway service {serviceId|input.serviceId}"),
    gql("railway.userDelete", "userDelete", "gone", "deletes the Railway account of the token's user"),
    gql("railway.projectScheduleDelete", "projectScheduleDelete", "recoverable", "schedules the Railway project {id} for deletion in 48 hours"),
    gql("railway.variableCollectionUpsert-replace", "variableCollectionUpsert", "gone",
        "replaces every variable of the Railway service {input.serviceId} in environment {input.environmentId}, deleting the ones not sent",
        arg={"input.replace": [True]}),
    # outage
    gql("railway.accessGroupDelete", "accessGroupDelete", "outage", "deletes a Railway access group {id}; its members lose the access it gave"),
    gql("railway.apiTokenDelete", "apiTokenDelete", "outage", "deletes the Railway API token {id}; whatever uses it stops"),
    gql("railway.workspaceApiTokenDelete", "workspaceApiTokenDelete", "outage", "revokes the Railway workspace token {id}; whatever uses it stops"),
    gql("railway.projectTokenDelete", "projectTokenDelete", "outage", "deletes the Railway project token {id}; whatever uses it stops"),
    gql("railway.clearanceTokenRevoke", "clearanceTokenRevoke", "outage", "revokes a Railway Authentication token"),
    gql("railway.bucketCredentialsReset", "bucketCredentialsReset", "outage", "resets the credentials of a Railway bucket; clients with the old ones stop"),
    gql("railway.databasePasswordReset", "databasePasswordReset", "outage", "resets a Railway database's credentials; clients with the old ones stop"),
    gql("railway.customDomainDelete", "customDomainDelete", "outage", "removes the custom domain {id} from a Railway service"),
    gql("railway.serviceDomainDelete", "serviceDomainDelete", "outage", "removes the Railway domain {id} from a service"),
    gql("railway.tcpProxyDelete", "tcpProxyDelete", "outage", "removes the Railway TCP proxy {id}"),
    gql("railway.deploymentRemove", "deploymentRemove", "outage", "removes the Railway deployment {id}, taking it offline"),
    gql("railway.deploymentStop", "deploymentStop", "outage", "stops the Railway deployment {id}"),
    gql("railway.egressGatewayAssociationsClear", "egressGatewayAssociationsClear", "outage", "clears the static egress IPs of a Railway service; allowlists that name them stop matching"),
    gql("railway.egressGatewayRollbackFromHA", "egressGatewayRollbackFromHA", "outage", "moves a Railway service off its HA static egress IPs; allowlists that name them stop matching"),
    gql("railway.railwayDomainDnsRecordDelete", "railwayDomainDnsRecordDelete", "outage", "deletes a DNS record of a Railway domain"),
    gql("railway.railwayDomainDnsRecordUpdate", "railwayDomainDnsRecordUpdate", "outage", "changes a DNS record of a Railway domain"),
    gql("railway.railwayDomainNameserversSet", "railwayDomainNameserversSet", "outage", "moves a Railway domain to other nameservers"),
    gql("railway.railwayDomainEmailForwardingRemoveAll", "railwayDomainEmailForwardingRemoveAll", "outage", "stops all mail forwarding of a Railway domain"),
    gql("railway.templateRevert", "templateRevert", "outage", "reverts a Railway HA database cluster to a single instance, removing variables"),
    gql("railway.projectMemberRemove", "projectMemberRemove", "outage", "removes a member from the Railway project {input.projectId|projectId}"),
    gql("railway.workspaceUserRemove", "workspaceUserRemove", "outage", "removes a user from a Railway workspace"),
    gql("railway.projectLeave", "projectLeave", "outage", "makes the token's user leave the Railway project {id}"),
    gql("railway.workspaceLeave", "workspaceLeave", "outage", "makes the token's user leave a Railway workspace"),
    gql("railway.serviceInstanceUpdate-zero", "serviceInstanceUpdate", "outage",
        "scales the Railway service {serviceId} to zero replicas", arg={"input.numReplicas": [0]}),
    # exposed
    gql("railway.apiTokenCreate", "apiTokenCreate", "exposed", "creates a Railway API token"),
    gql("railway.projectTokenCreate", "projectTokenCreate", "exposed", "creates a Railway project token for {input.projectId}"),
    gql("railway.clearanceTokenCreate", "clearanceTokenCreate", "exposed", "creates a Railway Authentication token for protected services"),
    gql("railway.projectInvitationCreate", "projectInvitationCreate", "exposed", "invites {input.email} to the Railway project {id|input.projectId}"),
    gql("railway.projectInviteCodeCreate", "projectInviteCodeCreate", "exposed", "creates an invite link to a Railway project"),
    gql("railway.workspaceInviteCodeCreate", "workspaceInviteCodeCreate", "exposed", "creates an invite code to a Railway workspace"),
    gql("railway.workspaceUserInvite", "workspaceUserInvite", "exposed", "invites {input.email} to a Railway workspace"),
    gql("railway.projectMemberAdd", "projectMemberAdd", "exposed", "adds a member to the Railway project {input.projectId}"),
    gql("railway.projectMemberUpdate", "projectMemberUpdate", "exposed", "changes a member's role in the Railway project {input.projectId} to {input.role}"),
    gql("railway.workspacePermissionChange", "workspacePermissionChange", "exposed", "changes a user's role in a Railway workspace to {input.role}"),
    gql("railway.accessGroupMemberAdd", "accessGroupMemberAdd", "exposed", "adds a member to a Railway access group"),
    gql("railway.accessGroupProjectAttach", "accessGroupProjectAttach", "exposed", "gives a Railway access group access to a project"),
    gql("railway.accessGroupUpdate", "accessGroupUpdate", "exposed", "changes a Railway access group's name or role"),
    gql("railway.projectTransfer", "projectTransfer", "exposed", "transfers the Railway project {projectId} to workspace {input.workspaceId}"),
    gql("railway.projectTransferInitiate", "projectTransferInitiate", "exposed", "starts transferring a Railway project to another owner"),
    gql("railway.projectTransferConfirm", "projectTransferConfirm", "exposed", "confirms a Railway project transfer"),
    gql("railway.projectTransferToTeam", "projectTransferToTeam", "exposed", "transfers a Railway project to a team"),
    gql("railway.projectUpdate-public", "projectUpdate", "exposed", "makes the Railway project {id} public", arg={"input.isPublic": [True]}),
    gql("railway.environmentClearanceDefaultUpdate", "environmentClearanceDefaultUpdate", "exposed", "changes whether Railway Authentication protects an environment's services"),
    gql("railway.projectClearanceDefaultUpdate", "projectClearanceDefaultUpdate", "exposed", "changes whether new Railway environments start protected"),
    gql("railway.serviceInstanceClearanceUpdate", "serviceInstanceClearanceUpdate", "exposed", "changes whether Railway Authentication protects a service"),
    gql("railway.sshPublicKeyCreate", "sshPublicKeyCreate", "exposed", "adds an SSH key to the Railway account"),
    gql("railway.sshSignupApprove", "sshSignupApprove", "exposed", "approves an SSH key on the Railway account"),
    gql("railway.trustedDomainCreate", "trustedDomainCreate", "exposed", "lets anyone with an email at {input.domainName|input.domain} join a Railway workspace"),
    gql("railway.trustedDomainUpdate", "trustedDomainUpdate", "exposed", "changes who may join a Railway workspace by email domain"),
    gql("railway.twoFactorInfoDelete", "twoFactorInfoDelete", "exposed", "switches off two-factor sign-in on the Railway account"),
    gql("railway.passkeyDelete", "passkeyDelete", "exposed", "deletes a passkey of the Railway account"),
    gql("railway.emailChangeInitiate", "emailChangeInitiate", "exposed", "starts changing the Railway account's email to {newEmail}"),
    gql("railway.emailChangeConfirm", "emailChangeConfirm", "exposed", "confirms a change of the Railway account's email"),
    gql("railway.railwayDomainEmailForwardingRuleUpdate", "railwayDomainEmailForwardingRuleUpdate", "exposed", "changes where mail to a Railway domain address is forwarded"),
    # money
    gql("railway.usageLimitRemove", "usageLimitRemove", "money", "removes the Railway spending limit"),
    gql("railway.usageLimitSet", "usageLimitSet", "money", "sets the Railway spending limit to {input.hardLimitDollars} dollars"),
    gql("railway.agentUsageLimitSet", "agentUsageLimitSet", "money", "sets the Railway agent spending limit"),
    gql("railway.customerTogglePayoutsToCredits", "customerTogglePayoutsToCredits", "money", "changes where Railway template payouts go"),
    gql_allow("railway.small-or-undo", [
        "devNewLandingTargetClear", "environmentIacPartialRelease", "environmentIacPartialTransfer", "featureFlagRemove",
        "loginSessionCancel", "projectFeatureFlagRemove", "projectInvitationDelete", "projectScheduleDeleteCancel",
        "serviceFeatureFlagRemove", "sessionDelete", "userBetaLeave", "userDiscordDisconnect", "userFlagsRemove",
        "volumeInstancePITRRestore", "cloudAgentArchive", "cloudAgentUnarchive"],
        "a Railway call that removes nothing of value or undoes a removal"),
]

# ------------------------------------------------------------------ Fly.io (Machines REST + GraphQL)
FLY = [
    rest("fly.secrets-update-delete", "POST", "/v1/apps/{app}/secrets", "gone",
         "deletes the secret {matched} of the Fly app {app}", body={"field": "values.*", "in": [None]}),
    rest("fly.secretkey-delete", "DELETE", "/v1/apps/{app}/secretkeys/{name}", "gone", "deletes the secret key {name} of the Fly app {app}"),
    rest("fly.machine-stop", "POST", "/v1/apps/{app}/machines/{machine}/stop", "outage", "stops the Fly machine {machine} of {app}"),
    rest("fly.machine-suspend", "POST", "/v1/apps/{app}/machines/{machine}/suspend", "outage", "suspends the Fly machine {machine} of {app}"),
    rest("fly.machine-signal", "POST", "/v1/apps/{app}/machines/{machine}/signal", "outage", "sends signal {body.signal} to the Fly machine {machine} of {app}"),
    rest("fly.machine-cordon", "POST", "/v1/apps/{app}/machines/{machine}/cordon", "outage", "takes the Fly machine {machine} of {app} out of the proxy"),
    rest("fly.certificate-delete", "DELETE", ["/v1/apps/{app}/certificates/{host}", "/v1/apps/{app}/certificates/{host}/{kind}"], "outage", "removes the certificate for {host} from the Fly app {app}"),
    rest("fly.ip-delete", "DELETE", "/v1/apps/{app}/ip_assignments/{ip}", "outage", "releases the IP {ip} of the Fly app {app}"),
    rest("fly.postgres-delete", "DELETE", "/v1/postgres/{cluster}", "gone", "deletes the Fly Managed Postgres cluster {cluster}"),
    rest("fly.postgres-user-delete", "DELETE", "/v1/postgres/{cluster}/users/{user}", "outage", "deletes the database user {user} of Fly Postgres {cluster}"),
    rest("fly.postgres-user-rotate", "POST", "/v1/postgres/{cluster}/users/{user}/rotate_password", "outage", "rotates the password of {user} on Fly Postgres {cluster}; clients with the old one stop"),
    rest("fly.postgres-user-create", "POST", "/v1/postgres/{cluster}/users", "exposed", "creates a database user on Fly Postgres {cluster}"),
    rest("fly.postgres-user-role", "PATCH", "/v1/postgres/{cluster}/users/{user}", "exposed", "changes the role of {user} on Fly Postgres {cluster}"),
    rest("fly.postgres-extension-disable", "DELETE", "/v1/postgres/{cluster}/databases/{db}/extensions/{name}", "gone", "disables the extension {name} in {db} on Fly Postgres {cluster}"),
    rest("fly.postgres-attachment-delete", "DELETE", "/v1/postgres/{cluster}/attachments/{app}", "outage", "detaches the Fly app {app} from Postgres {cluster}"),
    allow("fly.lease-release", "DELETE", "/v1/apps/{app}/machines/{machine}/lease", "releases a Fly machine lease, which flyctl does after every change"),
    gql("fly.gql-moveApp", "moveApp", "exposed", "moves the Fly app {input.appId} to organization {input.organizationId}"),
    gql("fly.gql-setSecrets-replaceAll", "setSecrets", "gone", "replaces every secret of the Fly app {input.appId}, deleting the ones not sent", arg={"input.replaceAll": [True]}),
    gql("fly.gql-createLimitedAccessToken", "createLimitedAccessToken", "exposed", "creates a Fly access token {input.name}"),
    gql("fly.gql-createOrganizationInvitation", "createOrganizationInvitation", "exposed", "invites {input.email} to a Fly organization"),
    gql("fly.gql-updateOrganizationMembership", "updateOrganizationMembership", "exposed", "changes a member's role in a Fly organization"),
    gql("fly.gql-grantPostgresClusterUserAccess", "grantPostgresClusterUserAccess", "exposed", "grants a user access to a Fly Postgres cluster"),
    gql("fly.gql-createPostgresClusterUser", "createPostgresClusterUser", "exposed", "creates a database user on a Fly Postgres cluster"),
    gql("fly.gql-addWireGuardPeer", "addWireGuardPeer", "exposed", "adds a WireGuard peer to a Fly organization's private network"),
    gql_allow("fly.gql-small", ["deleteOrganizationInvitation"], "cancels a Fly invitation"),
]

# ------------------------------------------------------------------ Vercel (REST)
V = "/v*"
VERCEL = [
    rest("vercel.alias-delete", "DELETE", [V + "/aliases/{alias}"], "outage", "removes the Vercel alias {alias}"),
    rest("vercel.project-domain-delete", "DELETE", [V + "/projects/{project}/domains/{domain}"], "outage", "removes {domain} from the Vercel project {project}"),
    rest("vercel.cert-delete", "DELETE", [V + "/certs/{id}"], "outage", "removes the Vercel certificate {id}"),
    rest("vercel.env-batch-delete", "DELETE", [V + "/projects/{project}/env", V + "/env"], "gone", "deletes environment variables of the Vercel project {project}"),
    rest("vercel.custom-env-delete", "DELETE", [V + "/projects/{project}/custom-environments/{env}"], "gone", "deletes the custom environment {env} of the Vercel project {project}"),
    rest("vercel.token-delete", "DELETE", [V + "/user/tokens/{id}"], "outage", "deletes the Vercel token {id}; whatever uses it stops"),
    rest("vercel.member-delete", "DELETE", [V + "/teams/{team}/members/{uid}", V + "/projects/{project}/members/{uid}"], "outage", "removes a member from Vercel"),
    rest("vercel.user-delete", "DELETE", [V + "/user"], "gone", "deletes the Vercel account of the token's user"),
    rest("vercel.dns-replace", "PUT", ["/domains/{domain}/records", V + "/domains/{domain}/records"], "gone", "replaces every DNS record of {domain} on Vercel"),
    rest("vercel.dns-update", "PATCH", [V + "/domains/records/{record}"], "outage", "changes the Vercel DNS record {record}"),
    rest("vercel.nameservers", "PATCH", [V + "/registrar/domains/{domain}/nameservers"], "outage", "moves {domain} to other nameservers"),
    rest("vercel.domain-move", "POST", [V + "/projects/{project}/domains/{domain}/move"], "outage", "moves {domain} out of the Vercel project {project}"),
    rest("vercel.env-unlink", "PATCH", [V + "/env/{id}/unlink/{project}"], "outage", "unlinks a shared variable from the Vercel project {project}"),
    rest("vercel.project-pause", "POST", [V + "/projects/{project}/pause"], "outage", "pauses the Vercel project {project}: every deployment stops serving"),
    rest("vercel.cache-dangerous-delete", "POST", [V + "/edge-cache/dangerously-delete-by-tags", V + "/edge-cache/dangerously-delete-by-src-images"], "outage", "deletes cached content on Vercel's edge"),
    rest("vercel.global-config-restore", "POST", [V + "/global-config/{id}/backups/{backup}/restore", V + "/edge-config/{id}/backups/{backup}/restore"], "gone", "restores the Global Config {id} from a backup, replacing its items"),
    rest("vercel.installation-credentials", "POST", [V + "/installations/{id}/credentials/revoke", V + "/installations/{id}/credentials/rotate"], "outage", "revokes or rotates the credentials of the Vercel integration {id}"),
    rest("vercel.kms-key-revoke", "POST", [V + "/kms/issuers/{issuer}/keys/{key}/revoke"], "outage", "revokes the Vercel signing key {key}"),
    rest("vercel.ai-budget-delete", "DELETE", ["/ai-gateway/budgets", "/ai-gateway/budgets/defaults"], "money", "removes an AI Gateway spending budget"),
    rest("vercel.api-key-quota", "PATCH", ["/api-keys/{id}/quota"], "money", "changes the spending budget of the AI Gateway key {id}"),
    rest("vercel.buy", "POST", [V + "/billing/buy", V + "/registrar/domains/buy", V + "/registrar/domains/{domain}/buy",
                                 V + "/registrar/domains/{domain}/renew", V + "/registrar/domains/{domain}/transfer"], "money", "buys on Vercel: {path}"),
    rest("vercel.token-create", "POST", [V + "/user/tokens", "/api-keys", V + "/global-config/{id}/token", V + "/edge-config/{id}/token"], "exposed", "creates a Vercel token or key"),
    rest("vercel.sdk-key-create", "PUT", [V + "/projects/{project}/feature-flags/sdk-keys"], "exposed", "creates a flags SDK key for the Vercel project {project}"),
    rest("vercel.member-add", "POST", [V + "/teams/{team}/members", V + "/projects/{project}/members"], "exposed", "adds {body.email|body.uid} to Vercel with role {body.role}"),
    rest("vercel.member-role", "PATCH", [V + "/teams/{team}/members/{uid}"], "exposed", "changes the role of {uid} in a Vercel team to {body.role}"),
    rest("vercel.access-group-grant", "POST", [V + "/access-groups/{group}/projects", V + "/access-groups/{group}"], "exposed", "changes what the Vercel access group {group} may reach"),
    rest("vercel.access-group-project-role", "PATCH", [V + "/access-groups/{group}/projects/{project}"], "exposed", "changes the role of the Vercel access group {group} on {project}"),
    rest("vercel.dsync-roles", "POST", [V + "/teams/{team}/dsync-roles"], "exposed", "changes how directory groups map to Vercel roles"),
    rest("vercel.dsync-group", "PUT", [V + "/organizations/{org}/dsync/groups/{group}"], "exposed", "changes how a directory group maps to Vercel roles"),
    rest("vercel.protection-bypass", "PATCH", [V + "/projects/{project}/protection-bypass", "/aliases/{id}/protection-bypass"], "exposed", "creates or changes a link that skips Vercel deployment protection"),
    rest("vercel.project-unprotect", "PATCH", [V + "/projects/{project}"], "exposed", "removes {matched} from the Vercel project {project}",
         body={"field": "ssoProtection", "in": [None]}),
    rest("vercel.project-unprotect-password", "PATCH", [V + "/projects/{project}"], "exposed", "removes password protection from the Vercel project {project}",
         body={"field": "passwordProtection", "in": [None]}),
    rest("vercel.project-public-source", "PATCH", [V + "/projects/{project}"], "exposed", "makes the source and logs of the Vercel project {project} public",
         body={"field": "publicSource", "in": [True]}),
    rest("vercel.firewall-bypass", ["POST", "PUT"], [V + "/security/firewall/bypass", V + "/security/firewall/bypass/rules"], "exposed", "lets traffic skip the Vercel firewall"),
    rest("vercel.firewall-replace", "PUT", [V + "/security/firewall/config"], "exposed", "replaces the whole Vercel firewall configuration"),
    rest("vercel.drain-create", "POST", [V + "/drains", V + "/log-drains", V + "/integrations/log-drains"], "exposed", "sends Vercel logs to {body.url|body.endpoint}"),
    rest("vercel.drain-change", "PATCH", [V + "/drains/{id}"], "exposed", "changes where the Vercel drain {id} sends logs"),
    rest("vercel.webhook", "POST", [V + "/webhooks"], "exposed", "sends Vercel events to {body.url}"),
    rest("vercel.vcr-permission", "POST", [V + "/vcr/repository/{repo}/permissions"], "exposed", "grants access to the Vercel container repository {repo}"),
    rest("vercel.transfer-request", "POST", ["/projects/{project}/transfer-request", V + "/projects/{project}/transfer-request"], "exposed", "starts transferring the Vercel project {project} to another team"),
    allow("vercel.small-deletes", "DELETE", [V + "/projects/{project}/avatar", V + "/teams/{team}/invites/{invite}",
                                              V + "/{team}/{project}/{repo}/blobs/uploads/{uuid}"], "a Vercel call that removes nothing of value"),
]

# ------------------------------------------------------------------ Supabase (Management REST)
SB = [
    rest("supabase.branch-merge", "POST", "/v1/branches/{branch}/merge", "gone", "merges the Supabase branch {branch} into production, running its migrations there"),
    rest("supabase.backup-restore", "POST", ["/v1/projects/{ref}/database/backups/restore", "/v1/projects/{ref}/database/backups/restore-pitr",
                                              "/v1/projects/{ref}/database/backups/undo"], "gone",
         "restores the Supabase database {ref} to an earlier point, losing what was written since"),
    rest("supabase.pgsodium-root-key", "PUT", "/v1/projects/{ref}/pgsodium", "gone", "changes the pgsodium root key of {ref}; data encrypted with the old key can become unreadable"),
    rest("supabase.functions-delete-all", "DELETE", "/v1/projects/{ref}/functions", "gone", "deletes every Edge Function of the Supabase project {ref}"),
    rest("supabase.function-delete", "DELETE", "/v1/projects/{ref}/functions/{slug}", "gone", "deletes the Edge Function {slug} of the Supabase project {ref}"),
    rest("supabase.secrets-delete", "DELETE", "/v1/projects/{ref}/secrets", "gone", "deletes {count.} secrets of the Supabase project {ref}"),
    rest("supabase.preview-branching-off", "DELETE", "/v1/projects/{ref}/branches", "gone", "switches off branching for {ref}, deleting every preview branch"),
    rest("supabase.restart", "POST", ["/v1/projects/{ref}/restart", "/v1/projects/{ref}/config/realtime/shutdown"], "outage", "restarts or shuts down a service of the Supabase project {ref}"),
    rest("supabase.upgrade", "POST", "/v1/projects/{ref}/upgrade", "outage", "upgrades the Postgres version of {ref}: downtime, and no way back"),
    rest("supabase.db-password", "PATCH", "/v1/projects/{ref}/database/password", "outage", "changes the database password of {ref}; clients with the old one stop"),
    rest("supabase.legacy-keys", "PUT", "/v1/projects/{ref}/api-keys/legacy", "outage", "disables or re-enables the anon and service_role keys of {ref}"),
    rest("supabase.signing-key-status", "PATCH", "/v1/projects/{ref}/config/auth/signing-keys/{id}", "outage", "changes the status of a JWT signing key of {ref}; revoking it signs every user out"),
    rest("supabase.oauth-revoke", "POST", "/v1/oauth/revoke", "outage", "revokes a Supabase OAuth authorization and its tokens"),
    rest("supabase.read-replica-remove", "POST", "/v1/projects/{ref}/read-replicas/remove", "outage", "removes a read replica of {ref}"),
    rest("supabase.addon-remove", "DELETE", "/v1/projects/{ref}/billing/addons/{addon}", "outage", "removes the add-on {addon} from {ref} (compute size or point-in-time recovery)"),
    rest("supabase.addon-apply", "PATCH", "/v1/projects/{ref}/billing/addons", "money", "changes the paid add-ons of {ref} to {body.addon_variant}"),
    rest("supabase.api-key-create", "POST", "/v1/projects/{ref}/api-keys", "exposed", "creates an API key for {ref}"),
    rest("supabase.claim-token", "POST", "/v1/projects/{ref}/claim-token", "exposed", "creates a token that lets another organization claim {ref}"),
    rest("supabase.sso-provider", ["POST", "PUT"], ["/v1/projects/{ref}/config/auth/sso/providers", "/v1/projects/{ref}/config/auth/sso/providers/{id}"], "exposed", "adds or changes an SSO provider of {ref}"),
    rest("supabase.third-party-auth", "POST", "/v1/projects/{ref}/config/auth/third-party-auth", "exposed", "trusts a third-party auth issuer for {ref}"),
    rest("supabase.jit", ["POST", "PUT"], ["/v1/projects/{ref}/database/jit", "/v1/projects/{ref}/database/jit/invite"], "exposed", "grants database access to a user of {ref}"),
    rest("supabase.network-restrictions", ["PATCH", "POST"], ["/v1/projects/{ref}/network-restrictions", "/v1/projects/{ref}/network-restrictions/apply"], "exposed", "changes which addresses may reach the database of {ref}"),
    rest("supabase.ssl-enforcement", "PUT", "/v1/projects/{ref}/ssl-enforcement", "exposed", "changes whether {ref} requires SSL"),
    rest("supabase.postgrest-config", "PATCH", "/v1/projects/{ref}/postgrest", "exposed", "changes which schemas the data API of {ref} exposes"),
    allow("supabase.small-deletes", "DELETE", ["/v1/projects/{ref}/claim-token", "/v1/projects/{ref}/cli/login-role",
                                                "/v1/projects/{ref}/database/jit/invite/{id}"], "a Supabase call that removes nothing of value"),
]

# ------------------------------------------------------------------ Cloudflare (REST v4)
A = "/client/v4/accounts/{account}"
Z = "/client/v4/zones/{zone}"
CF = [
    rest("cloudflare.d1-time-travel", "POST", A + "/d1/database/{db}/time_travel/restore", "gone", "restores the D1 database {db} to {query.bookmark|query.timestamp}, losing what was written since"),
    rest("cloudflare.queue-purge", "POST", [A + "/queues/{queue}/purge", A + "/queues/{queue}/messages/purge"], "gone", "deletes every message of the queue {queue}"),
    rest("cloudflare.queue-preview-ack", "POST", A + "/queues/{queue}/messages/preview/ack", "gone", "deletes previewed messages of the queue {queue}"),
    rest("cloudflare.vectors-delete", "POST", [A + "/vectorize/indexes/{index}/delete-by-ids", A + "/vectorize/v2/indexes/{index}/delete_by_ids"], "gone", "deletes {count.ids} vectors from the index {index}"),
    rest("cloudflare.r2-lifecycle", "PUT", A + "/r2/buckets/{bucket}/lifecycle", "gone", "sets expiry rules on the R2 bucket {bucket}: objects they match are deleted later, without another request"),
    rest("cloudflare.r2-lock-delete", "DELETE", A + "/r2/buckets/{bucket}/lock", "gone", "removes the lock rules of the R2 bucket {bucket}, so locked objects can be deleted"),
    rest("cloudflare.r2-objects-delete", "DELETE", [A + "/r2/buckets/{bucket}/objects", A + "/r2/buckets/{bucket}/objects/{key+}"], "gone", "deletes {key} from the R2 bucket {bucket}"),
    rest("cloudflare.workflow-instances-delete", "POST", A + "/workflows/{wf}/instances/batch/delete", "gone", "deletes instances of the workflow {wf}"),
    rest("cloudflare.workflow-instances-terminate", "POST", A + "/workflows/{wf}/instances/batch/terminate", "outage", "terminates instances of the workflow {wf}"),
    rest("cloudflare.workflow-instance-terminate", "PATCH", A + "/workflows/{wf}/instances/{id}/status", "outage", "terminates the instance {id} of the workflow {wf}",
         body={"field": "status", "in": ["terminate"]}),
    rest("cloudflare.dns-change", ["PUT", "PATCH"], Z + "/dns_records/{record}", "outage", "changes the DNS record {record} to {body.content}"),
    rest("cloudflare.dns-batch-delete", "POST", Z + "/dns_records/batch", "outage", "deletes DNS records in a batch", body={"field": "deletes.*", "present": True}),
    rest("cloudflare.dns-batch-change", "POST", Z + "/dns_records/batch", "outage", "changes DNS records in a batch", body={"field": "patches.*", "present": True}),
    rest("cloudflare.dns-batch-put", "POST", Z + "/dns_records/batch", "outage", "overwrites DNS records in a batch", body={"field": "puts.*", "present": True}),
    rest("cloudflare.zone-pause", "PATCH", Z, "outage", "pauses Cloudflare on the zone {zone}", body={"field": "paused", "in": [True]}),
    rest("cloudflare.custom-hostname-fallback", "PUT", Z + "/custom_hostnames/fallback_origin", "outage", "changes the fallback origin of every custom hostname of {zone}"),
    rest("cloudflare.worker-route-replace", "PUT", Z + "/workers/routes/{route}", "outage", "changes the Worker route {route}"),
    rest("cloudflare.email-routing-off", "POST", Z + "/email/routing/disable", "outage", "switches off email routing for {zone}"),
    rest("cloudflare.token-roll", "PUT", [A + "/tokens/{id}/value", "/client/v4/user/tokens/{id}/value"], "outage", "rolls the API token {id}; whatever uses the old value stops"),
    rest("cloudflare.token-create", "POST", [A + "/tokens", "/client/v4/user/tokens", A + "/r2/temp-access-credentials"], "exposed", "creates an API token or credentials ({body.name})"),
    rest("cloudflare.token-update", "PUT", [A + "/tokens/{id}", "/client/v4/user/tokens/{id}"], "exposed", "changes the rights of the API token {id}"),
    rest("cloudflare.member", ["POST", "PUT"], [A + "/members", A + "/members/{id}"], "exposed", "adds a member to the account or changes a member's roles"),
    rest("cloudflare.r2-public-domain", "POST", [A + "/r2/buckets/{bucket}/domains/custom", A + "/r2/buckets/{bucket}/custom_domains"], "exposed", "serves the R2 bucket {bucket} on the public domain {body.domain}"),
    rest("cloudflare.r2-public-dev", "PUT", A + "/r2/buckets/{bucket}/domains/managed", "exposed", "makes the R2 bucket {bucket} public on r2.dev", body={"field": "enabled", "in": [True]}),
    rest("cloudflare.tunnel-config", "PUT", A + "/cfd_tunnel/{tunnel}/configurations", "exposed", "changes what the tunnel {tunnel} publishes"),
    rest("cloudflare.observability-destination", ["POST", "PATCH"], [A + "/workers/observability/destinations", A + "/workers/observability/destinations/{slug}"], "exposed", "sends Workers telemetry to an outside endpoint"),
    rest("cloudflare.email-catch-all", "PUT", [Z + "/email/routing/rules/catch_all", Z + "/email/routing/rules/{rule}"], "exposed", "changes where mail to {zone} is delivered"),
    rest("cloudflare.ruleset-replace", ["PUT", "DELETE"], [Z + "/rulesets/phases/{phase}/entrypoint", Z + "/rulesets/{ruleset}"], "exposed", "replaces or deletes the rules of {phase|ruleset} on {zone} (WAF and redirects included)"),
    rest("cloudflare.ruleset-rule-delete", "DELETE", Z + "/rulesets/{ruleset}/rules/{rule}", "exposed", "deletes the rule {rule} of ruleset {ruleset} on {zone}"),
    rest("cloudflare.setting-ssl-off", "PATCH", Z + "/settings/ssl", "exposed", "sets SSL on {zone} to {body.value}", body={"field": "value", "in": ["off", "flexible"]}),
    rest("cloudflare.setting-https-off", "PATCH", [Z + "/settings/always_use_https", Z + "/settings/automatic_https_rewrites", Z + "/settings/min_tls_version"], "exposed", "weakens HTTPS on {zone}: {path} to {body.value}", body={"field": "value", "in": ["off", "1.0"]}),
    rest("cloudflare.setting-security-off", "PATCH", [Z + "/settings/security_level", Z + "/settings/waf", Z + "/settings/browser_check"], "exposed", "switches off a protection on {zone}: {path} to {body.value}", body={"field": "value", "in": ["off", "essentially_off"]}),
    rest("cloudflare.settings-ssl-off", "PATCH", Z + "/settings", "exposed", "changes several settings of {zone}, one of them a protection switched off", body={"field": "items.*.value", "in": ["off", "flexible", "essentially_off"]}),
    allow("cloudflare.small-deletes", "DELETE", [A + "/pages/projects/{p}/deployments/{d}/tails/{t}", A + "/workers/scripts/{s}/tails/{t}",
                                                  A + "/workers/dispatch/namespaces/{n}/scripts/{s}/tags/{tag}",
                                                  A + "/workers/observability/issues/automations/{id}", A + "/workers/observability/issues/groups/{id}",
                                                  A + "/workers/observability/issues/groups/{id}/members/{issue}",
                                                  A + "/workers/observability/queries/{id}", Z + "/rulesets/{ruleset}/versions/{version}"],
          "a Cloudflare call that removes nothing of value"),
]

ADD = {"railway": RW, "fly": FLY, "vercel": VERCEL, "supabase": SB, "cloudflare": CF}


def main():
    rules = json.load(open(os.path.join(OLD, "rules.json")))
    out = copy.deepcopy(rules)
    out["about"] = (rules.get("about", "") + " Extended on 2026-11-25 for Railway, Fly.io, Vercel, Supabase and Cloudflare "
                    "from each provider's documented surface (research/2026-11-25-destructive-calls-platforms).").strip()
    for api in out["apis"]:
        # paths of files #353 refers to are relative to its own folder
        g = api.get("graphql")
        if g and g.get("known_mutations"):
            g["known_mutations"] = {"railway": "schemas/railway_mutations.json", "fly": "schemas/fly_mutations.json"}.get(api["id"], OLDP + g["known_mutations"])
        if api.get("aws") and api["aws"].get("catalogue"):
            api["aws"]["catalogue"] = OLDP + api["aws"]["catalogue"]
        for r in api.get("rules", []):
            ff = r["match"].get("graphql", {}).get("field_from")
            if ff:
                ff["file"] = OLDP + ff["file"]
        new = ADD.get(api["id"])
        if not new:
            continue
        have = {r["id"] for r in api["rules"]}
        dup = [r["id"] for r in new if r["id"] in have]
        assert not dup, dup
        # specific rules first; #353's fallbacks keep working because fallbacks only fire when nothing else matched
        api["rules"] = [r for r in api["rules"] if not r.get("fallback")] + new + [r for r in api["rules"] if r.get("fallback")]
    with open(os.path.join(HERE, "rules.json"), "w") as f:
        json.dump(out, f, indent=1)
    print("rules.json:", sum(len(a.get("rules", [])) for a in out["apis"]), "rules,",
          sum(len(v) for v in ADD.values()), "new")


if __name__ == "__main__":
    main()
