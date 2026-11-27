"""Hand classification of the calls that give or remove access, in #383's and #404's sets.

Every call that #383's wide wording of "exposed" holds and the wording in force does not, and every
call #404 labelled hold:exposed, gets one class:

  anyone      makes something readable or reachable by anyone (the wording in force)
  protection  turns off a protection the provider offers (already held by the answer in force)
  lasting     gives lasting access to someone who did not have it: a long-lived token or key, an
              invitation, a member or role, the project moved to another account, data sent to an
              outside address
  run         mints a credential that expires within the run, for the caller's own run
  self        gives nobody new anything: the caller logs in, accepts an invitation someone else
              made, or sets up its own signing authority

`lifetime` notes where the request itself can say how long the credential lives (a field of the
body), which is what a rule needs to tell `run` from `lasting` on the wire.

A revocation removes access someone had and gives nobody anything: a member or role removed, a key
or token deleted, a webhook, drain or sink deleted. REVOKE_* list them. A removal that widens
access (a deny rule, a ban, a permissions boundary, an admission or network policy deleted) is not
a revocation; it is `protection`.
"""
import re

# ---------------------------------------------------------------- #383's set (Railway, Fly, Vercel, Supabase, Cloudflare)
# The 86 calls #405 marked as access given to someone named, which the wide wording holds and the
# wording in force passes. Key: case id as in #383's surface. Value: (class, reason, lifetime).
P_NAMED = {
    "cloudflare POST /client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/management": ("lasting", "a tunnel management token; its lifetime is not documented", None),
    "cloudflare POST /client/v4/accounts/{account_id}/containers/registries/{domain}/credentials": ("run", "a registry credential wrangler mints to push an image during a deploy", "expiration_minutes in the body"),
    "cloudflare POST /client/v4/accounts/{account_id}/members": ("lasting", "a member invited", None),
    "cloudflare POST /client/v4/accounts/{account_id}/r2/temp-access-credentials": ("run", "temporary R2 credentials", "ttlSeconds in the body"),
    "cloudflare POST /client/v4/accounts/{account_id}/tokens": ("lasting", "an API token, no expiry unless set", "expires_on in the body, optional"),
    "cloudflare POST /client/v4/accounts/{account_id}/workers/observability/destinations": ("lasting", "logs sent to an outside URL", None),
    "cloudflare POST /client/v4/user/tokens": ("lasting", "an API token, no expiry unless set", "expires_on in the body, optional"),
    "cloudflare POST /client/v4/zones/{zone_id}/email/routing/rules": ("lasting", "mail forwarded to an outside address", None),
    "fly POST /v1/postgres/{id}/users": ("lasting", "a database user", None),
    "fly-gql addAllowedReplaySourceOrgs": ("lasting", "another organization may replay requests into the apps", None),
    "fly-gql addWireGuardPeer": ("lasting", "a peer on the private network, kept until removed", None),
    "fly-gql createLimitedAccessToken": ("lasting", "flyctl tokens create: 20 years unless set", "expiry argument, optional"),
    "fly-gql createOrganizationInvitation": ("lasting", "an invitation", None),
    "fly-gql createPostgresClusterUser": ("lasting", "a database user", None),
    "fly-gql createThirdPartyConfiguration": ("lasting", "an outside service connected", None),
    "fly-gql establishSshKey": ("self", "the organization's own SSH certificate authority; it gives nobody access until a certificate is issued", None),
    "fly-gql grantPostgresClusterUserAccess": ("lasting", "a database grant", None),
    "fly-gql issueCertificate": ("run", "fly ssh console issues a short certificate per session", "validHours argument"),
    "fly-gql moveApp": ("lasting", "the app moved to another organization", None),
    "fly-gql updateOrganizationMembership": ("lasting", "a role changed", None),
    "railway accessGroupMemberAdd": ("lasting", "a member added", None),
    "railway accessGroupProjectAttach": ("lasting", "a group given a project", None),
    "railway accessGroupUpdate": ("lasting", "a group's role raised", None),
    "railway apiTokenCreate": ("lasting", "an account token, no expiry", None),
    "railway clearanceTokenCreate": ("lasting", "a clearance token; its lifetime is not documented", None),
    "railway cloudAgentHarnessToken": ("run", "a token for the cloud agent's own run", None),
    "railway generateShellToken": ("run", "railway ssh mints a shell token for its session", None),
    "railway loginSessionAuth": ("lasting", "authorizes a CLI login started elsewhere: a device whose code it is gets the account", None),
    "railway loginSessionConsume": ("self", "the CLI collects the login its user already authorized", None),
    "railway notificationRuleCreate": ("lasting", "alerts sent to an outside URL", None),
    "railway notificationRuleUpdate": ("lasting", "alerts sent to another URL", None),
    "railway projectInvitationCreate": ("lasting", "an invitation", None),
    "railway projectInviteCodeCreate": ("lasting", "an invite link", None),
    "railway projectMemberAdd": ("lasting", "a member added", None),
    "railway projectMemberUpdate": ("lasting", "a role changed", None),
    "railway projectTokenCreate": ("lasting", "a project token, no expiry", None),
    "railway projectTransfer": ("lasting", "the project moved to another workspace", None),
    "railway projectTransferConfirm": ("lasting", "the project moved to another workspace", None),
    "railway projectTransferInitiate": ("lasting", "a transfer to another party started", None),
    "railway projectTransferToTeam": ("lasting", "the project moved to a team", None),
    "railway railwayDomainEmailForwardingRuleCreate": ("lasting", "mail forwarded to an outside inbox", None),
    "railway railwayDomainEmailForwardingRuleUpdate": ("lasting", "mail forwarded elsewhere", None),
    "railway recoveryCodeGenerate": ("lasting", "recovery codes for the account, returned to the caller", None),
    "railway sshPublicKeyCreate": ("lasting", "an SSH key added", None),
    "railway sshSignupApprove": ("lasting", "an SSH key registered", None),
    "railway trustedDomainCreate": ("lasting", "anyone with a mail address at the domain may join", None),
    "railway trustedDomainUpdate": ("lasting", "the joining policy widened", None),
    "railway upsertSlackChannel": ("lasting", "events sent to a shared Slack channel", None),
    "railway webhookTest": ("lasting", "stored headers sent to any URL once; data sent cannot be taken back", None),
    "railway workspaceInviteCodeCreate": ("lasting", "an invite code", None),
    "railway workspaceUserInvite": ("lasting", "a member invited", None),
    "railway workspacePermissionChange": ("lasting", "permissions raised", None),
    "railway workspacePolicyDeploySourceAllowlistAdd": ("lasting", "a new source may deploy", None),
    "railway workspaceUpsertSlackChannel": ("lasting", "events sent to a shared Slack channel", None),
    "supabase POST /v1/oauth/token": ("self", "an OAuth app exchanges a code the user granted at consent", None),
    "supabase POST /v1/projects/{ref}/api-keys": ("lasting", "an API key", None),
    "supabase POST /v1/projects/{ref}/claim-token": ("lasting", "a token that lets another organization take the project", None),
    "supabase POST /v1/projects/{ref}/cli/login-role": ("run", "supabase db commands create a temporary login role", None),
    "supabase POST /v1/projects/{ref}/config/auth/sso/providers": ("lasting", "an outside identity provider may sign users in", None),
    "supabase POST /v1/projects/{ref}/config/auth/third-party-auth": ("lasting", "an outside issuer's tokens accepted", None),
    "supabase POST /v1/projects/{ref}/database/jit": ("lasting", "a database role granted to a user for a set time", "an expiry in the body"),
    "supabase POST /v1/projects/{ref}/database/jit/invite": ("lasting", "an outside user invited", None),
    "vercel PATCH /aliases/{id}/protection-bypass": ("lasting", "a shareable link past deployment protection", None),
    "vercel PATCH /v1/projects/{idOrName}/protection-bypass": ("lasting", "a secret past deployment protection", None),
    "vercel POST /api-keys": ("lasting", "an API key", None),
    "vercel POST /projects/{idOrName}/transfer-request": ("lasting", "a code that lets another team take the project", None),
    "vercel POST /v1/access-groups/{accessGroupIdOrName}/projects": ("lasting", "a group given a project", None),
    "vercel POST /v1/connect/token/{connector}": ("lasting", "a connector token; its lifetime is not documented", None),
    "vercel POST /v1/drains": ("lasting", "logs sent to an outside URL", None),
    "vercel POST /v1/global-config/{edgeConfigId}/token": ("lasting", "an Edge Config read token, no expiry", None),
    "vercel POST /v1/kms/issuers/{issuerId}/policies": ("lasting", "a subject may get signatures", None),
    "vercel POST /v1/kms/issuers/{issuerId}/sign/token": ("run", "a signed token for the caller", "an expiry in the body"),
    "vercel POST /v1/log-drains": ("lasting", "logs sent to an outside URL", None),
    "vercel POST /v1/projects/traces/session": ("run", "a tracing session token", None),
    "vercel POST /v1/projects/{idOrName}/members": ("lasting", "a member added", None),
    "vercel POST /v1/projects/{idOrName}/token": ("run", "the project's OIDC token for a build", None),
    "vercel POST /v1/security/firewall/bypass": ("lasting", "an address let past the firewall", None),
    "vercel POST /v1/security/firewall/bypass/rules": ("lasting", "an address let past the firewall", None),
    "vercel POST /v1/teams/{teamId}/members/teams/join": ("self", "the caller accepts an invitation someone else made", None),
    "vercel POST /v1/vcr/repository/{idOrName}/permissions": ("lasting", "a registry permission granted", None),
    "vercel POST /v1/webhooks": ("lasting", "events sent to an outside URL", None),
    "vercel POST /v2/integrations/log-drains": ("lasting", "logs sent to an outside URL", None),
    "vercel POST /v2/teams/{teamId}/members": ("lasting", "a member invited", None),
    "vercel POST /v3/user/tokens": ("lasting", "an account token, no expiry unless set", "expiresAt in the body, optional"),
    "vercel PUT /v1/organizations/{organizationId}/dsync/groups/{groupId}": ("lasting", "a directory group given roles", None),
    "vercel PUT /v1/projects/{projectIdOrName}/feature-flags/sdk-keys": ("lasting", "an SDK key", None),
}

# Revocations in #383's set, by case id prefix (variants share it).
REVOKE_P = """
cloudflare DELETE /client/v4/accounts/{account_id}/members/{member_id}
cloudflare DELETE /client/v4/accounts/{account_id}/tokens/{token_id}
cloudflare DELETE /client/v4/accounts/{account_id}/workers/observability/destinations/{slug}
cloudflare DELETE /client/v4/user/tokens/{token_id}
fly-gql deleteDeploymentSource
fly-gql deleteLimitedAccessToken
fly-gql deleteOrganizationInvitation
fly-gql deleteOrganizationMembership
fly-gql removeAllowedReplaySourceOrgs
fly-gql removeWireGuardPeer
fly-gql deleteThirdPartyConfiguration
railway accessGroupDelete
railway accessGroupMemberRemove
railway accessGroupProjectDetach
railway apiTokenDelete
railway botScopeBindingRemove
railway clearanceTokenRevoke
railway notificationRuleDelete
railway projectInvitationDelete
railway projectLeave
railway projectMemberRemove
railway projectTokenDelete
railway providerAuthRemove
railway railwayDomainEmailForwardingRuleDelete
railway serviceDisconnect
railway sshPublicKeyDelete
railway trustedDomainDelete
railway userDiscordDisconnect
railway workspaceApiTokenDelete
railway workspaceLeave
railway workspacePolicyDeploySourceAllowlistRemove
railway workspaceUserRemove
supabase DELETE /v1/projects/{ref}/api-keys/{id}
supabase DELETE /v1/projects/{ref}/claim-token
supabase DELETE /v1/projects/{ref}/cli/login-role
supabase DELETE /v1/projects/{ref}/config/auth/sso/providers/{provider_id}
supabase DELETE /v1/projects/{ref}/config/auth/third-party-auth/{tpa_id}
supabase DELETE /v1/projects/{ref}/database/jit/invite/{invite_id}
supabase DELETE /v1/projects/{ref}/database/jit/{user_id}
supabase POST /v1/oauth/revoke
vercel DELETE /ai-gateway/byok-credentials/{credentialId}
vercel DELETE /v1/access-groups/{accessGroupIdOrName}/projects/{projectId}
vercel DELETE /v1/access-groups/{idOrName}
vercel DELETE /v1/connect/connectors/{connector}/projects/{projectId}
vercel DELETE /v1/connect/connectors/{connector}/projects/{projectId}/environments/{environment}
vercel DELETE /v1/drains/{id}
vercel DELETE /v1/global-config/{edgeConfigId}/tokens
vercel DELETE /v1/integrations/log-drains/{id}
vercel DELETE /v1/kms/issuers/{issuerId}/policies/{kind}/{policyKey}
vercel DELETE /v1/log-drains/{id}
vercel DELETE /v1/organizations/{organizationId}/dsync/groups/{groupId}
vercel DELETE /v1/projects/{idOrName}/members/{uid}
vercel DELETE /v1/projects/{projectIdOrName}/feature-flags/connections/{consumerProjectId}
vercel DELETE /v1/projects/{projectIdOrName}/feature-flags/sdk-keys/{hashKey}
vercel DELETE /v1/security/firewall/bypass
vercel DELETE /v1/security/firewall/bypass/rules
vercel DELETE /v1/teams/{teamId}/invites/{inviteId}
vercel DELETE /v1/teams/{teamId}/members/{uid}
vercel DELETE /v1/vcr/repository/{idOrName}/permissions
vercel DELETE /v1/vcr/repository/{idOrName}/permissions/all
vercel DELETE /v1/webhooks/{id}
vercel DELETE /v3/user/tokens/{tokenId}
vercel POST /v1/installations/{integrationConfigurationId}/credentials/revoke
vercel POST /v1/kms/issuers/{issuerId}/keys/{keyId}/revoke
""".split("\n")
REVOKE_P = {x.strip() for x in REVOKE_P if x.strip()}


def is_revoke_p(case_id):
    base = case_id.split(" #")[0]
    return base in REVOKE_P


# ---------------------------------------------------------------- #404's set (AWS, Google Cloud, Azure, GitHub, Kubernetes)
# #404 labelled under #383's wide wording. Its 420 hold:exposed calls are classed by the label's
# reason and the operation, first match wins. (pattern on "provider op", class, reason, lifetime)
C_RULES = [
    # run: expires within the run, minted for the caller
    (r"^kubernetes POST .*/serviceaccounts/\{name\}/token$", "run", "a TokenRequest; one hour unless set", "expirationSeconds in the body"),
    (r"^github POST /app/installations/\{installation_id\}/access_tokens$", "run", "an installation token: one hour", None),
    (r"^github POST /applications/\{client_id\}/token/scoped$", "run", "a narrowed copy of a user token, same expiry", None),
    (r"^gcloud iam\.projects\.serviceAccounts\.signJwt$", "run", "a JWT signed as the service account: at most 12 hours", "exp in the payload"),
    (r"^github POST .*/actions/runners/generate-jitconfig$", "run", "a just-in-time runner: one job, then removed", None),
    # protection: a protection turned off
    (r"DisableImageBlockPublicAccess|DisableSnapshotBlockPublicAccess|CreateVpcBlockPublicAccessExclusion|EnableSerialConsoleAccess|DeactivateMFADevice|DeleteVirtualMFADevice|DeleteAccountPasswordPolicy|DisassociateDistributionTenantWebACL|removeAssociation$|basicPublishingCredentialsPolicies|fork-pr-|removeEnterpriseIdentityProvider|validatingadmissionpolicybindings|rowAccessPolicies\.batchDelete|setMasterAuth", "protection", "a protection turned off", None),
]
C_REASON = [  # on #404's own reason
    (r"guard|all off|IMDSv1|any image|removes a limit|stops masking|firewall goes|work again|guardrail|every policy|rules stop applying|ABAC|auth off|logins work again|non-TLS|admin user on|write by default|anyone with write|secret be pushed|admission check|every object that matches|let through|rows become visible|prevention off", "protection"),
    (r"allUsers|allAuthenticatedUsers|public|Principal \*|0\.0\.0\.0/0|every address|from the Internet|network open|AllowAll|LoadBalancer|external IP|no auth|anonymous pull|private false|API server open|opens the Data API|readable", "anyone"),
]


def class_c(provider, op, why):
    key = provider + " " + op
    for pat, cls, reason, life in C_RULES:
        if re.search(pat, key):
            return cls, reason, life
    for pat, cls in C_REASON:
        if why and re.search(pat, why):
            return cls, why, None
    life = None
    if re.search(r"listAccountSas|listServiceSas", op):
        life = "signedExpiry in the body"
    elif re.search(r"beginGetAccess", op):
        life = "durationInSeconds in the body"
    elif re.search(r"registration-token", op):
        life = "the token lasts an hour, the runner it registers stays"
    return "lasting", why or "access, a credential or data given to someone named", life


# Revocations in #404's set: pattern on "provider op". Excluded on purpose: removals that widen
# access (permissions boundaries, SCPs, row access policies, network and admission policies, branch
# protection, deny rules), key rotation (a new key is returned), and deletes of whole resources.
REVOKE_C = [
    r"^aws ec2 Revoke(ClientVpnIngress|SecurityGroupEgress|SecurityGroupIngress)$",
    r"^aws rds RevokeDBSecurityGroupIngress$",
    r"^aws (lambda|sqs|sns) RemovePermission$",
    r"^aws iam (DeleteAccessKey|DeleteLoginProfile|DeleteSSHPublicKey|DeleteServiceSpecificCredential|DeleteSigningCertificate|Detach(Group|Role|User)Policy|RemoveUserFromGroup|RemoveRoleFromInstanceProfile|RemoveClientIDFromOpenIDConnectProvider|DeleteOpenIDConnectProvider|DeleteSAMLProvider)$",
    r"^aws eks (DeleteAccessEntry|DisassociateAccessPolicy|DeletePodIdentityAssociation)$",
    r"^aws kms RevokeGrant$",
    r"^aws sns Unsubscribe$",
    r"^aws logs (DeleteSubscriptionFilter|DeleteDestination)$",
    r"^aws organizations (RemoveAccountFromOrganization|DeregisterDelegatedAdministrator)$",
    r"^gcloud iam\..*(keys\.delete|tokens\.delete|revokeSessions|keys\.disable)$",
    r"^gcloud logging\..*sinks\.delete$",
    r"^gcloud sql\.(users|sslCerts)\.delete$",
    r"^gcloud storage\.projects\.hmacKeys\.delete$",
    r"^azure DELETE .*/(roleAssignments|sqlRoleAssignments|tableRoleAssignments|cassandraRoleAssignments|gremlinRoleAssignments|mongoMIRoleAssignments|accessPolicyAssignments|trustedAccessRoleBindings|federatedIdentityCredentials|administrators)/\{[^}]+\}$",
    r"^azure DELETE .*/(functions/\{functionName\}/keys|host/default/\{keyType\})/\{keyName\}$",
    r"^azure DELETE .*/(tokens|credentialSets|webhooks|jobAgents/\{jobAgentName\}/credentials)/\{[^}]+\}$",
    r"^azure POST .*/revokeUserDelegationKeys$",
    r"^github POST /credentials/revoke$",
    r"^github DELETE /orgs/\{org\}/(members|memberships|outside_collaborators|public_members)/\{username\}$",
    r"^github DELETE /orgs/\{org\}/(hooks/\{hook_id\}|invitations/\{invitation_id\})$",
    r"^github DELETE /orgs/\{org\}/teams/\{team_slug\}/memberships/\{username\}$",
    r"^github DELETE /orgs/\{org\}/(copilot-spaces/.*/collaborators/.*|organization-roles/(teams|users)/.*)$",
    r"^github DELETE /orgs/\{org\}/actions/permissions/(repositories|self-hosted-runners/repositories)/\{repository_id\}$",
    r"^github DELETE /orgs/\{org\}/(actions|agents|codespaces|dependabot)/secrets/\{secret_name\}/repositories/\{repository_id\}$",
    r"^github DELETE /repos/\{owner\}/\{repo\}/(collaborators/\{username\}|hooks/\{hook_id\}|keys/\{key_id\}|invitations/\{invitation_id\})$",
    r"^github DELETE /teams/\{team_id\}/(members|memberships)/\{username\}$",
    r"^github DELETE /user/(gpg_keys|keys|ssh_signing_keys)/\{[^}]+\}$",
    r"^github DELETE /users/\{username\}/copilot-spaces/.*/collaborators/.*$",
    r"^github DELETE /user/repository_invitations/\{invitation_id\}$",
    r"^github-gql (removeEnterpriseMember|revokeEnterpriseOrganizationsMigratorRole|revokeMigratorRole|removeOutsideCollaborator|removeEnterpriseAdmin|cancelEnterpriseAdminInvitation|cancelEnterpriseMemberInvitation)$",
    r"^kubernetes DELETE /apis/rbac\.authorization\.k8s\.io/v1/(clusterrolebindings|namespaces/\{namespace\}/rolebindings)/\{name\}$",
]
REVOKE_C = [re.compile(p) for p in REVOKE_C]


def is_revoke_c(provider, op):
    key = provider + " " + op
    return any(p.search(key) for p in REVOKE_C)
