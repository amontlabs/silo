"""Labels for GitHub's REST API (api.github.com.json 1.1.4): an ordered table of (method, path pattern,
label, why), first match wins, written from reading every write operation. Writes labels/github.txt.
Words as in label_aws.py.
"""
import json, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
G, O, X, R, E, A, D, C, M = ("hold:gone", "hold:outage", "hold:exposed", "hold:recoverable", "either", "allow",
                             "deploy", "cond", "hold:money")

V = r"\{[^}]+\}"
TABLE = [
    # ---- whole things
    ("DELETE", r"^/repos/{v}/{v}$", G, "the repository; 90 days to restore for some"),
    ("DELETE", r"^/orgs/{v}$", G, "the organization and its repositories"),
    ("PATCH", r"^/repos/{v}/{v}$", C, ""),
    ("PATCH", r"^/orgs/{v}$", E, "member and repository defaults"),
    ("POST", r"^/repos/{v}/{v}/transfer$", X, "another owner gets the repository"),
    ("DELETE", r"/gists/{v}$", G, ""),
    ("DELETE", r"^/user/codespaces/{v}$", G, "unpushed work in it"),
    ("DELETE", r"/members/{v}/codespaces/{v}$", G, "unpushed work in it"),
    ("DELETE", r"/environments/{v}$", G, "its secrets and protection rules go"),
    ("DELETE", r"/releases/{v}$", G, "its assets go"), ("DELETE", r"/releases/assets/{v}$", G, ""),
    ("DELETE", r"/packages/{v}/{v}$", R, "30 days to restore"), ("DELETE", r"/packages/{v}/{v}/versions/{v}$", R, "30 days to restore"),
    ("DELETE", r"/git/refs/{v}$", R, "the commits stay reachable for a while"),
    ("PATCH", r"/git/refs/{v}$", C, ""),
    ("DELETE", r"/contents/{v}$", E, "a commit that removes the file"),
    ("PUT", r"/contents/{v}$", E, "a commit that overwrites the file"),
    ("DELETE", r"/actions/runs/{v}/logs$", G, "the run's logs"), ("DELETE", r"/actions/runs/{v}$", G, "the run and its logs"),
    ("DELETE", r"/actions/artifacts/{v}$", A, ""), ("DELETE", r"/actions/caches", A, ""),
    ("DELETE", r"/code-scanning/analyses/{v}$", A, ""), ("DELETE", r"/code-scanning/codeql/databases/{v}$", A, ""),
    ("DELETE", r"/deployments/{v}$", A, "only an inactive one"), ("DELETE", r"/pages$", O, "the site goes offline"),
    ("DELETE", r"/migrations/{v}/archive$", A, ""), ("DELETE", r"/migrations/{v}/repos/{v}/lock$", A, ""),
    ("DELETE", r"/attestations/", E, ""), ("POST", r"/attestations/delete-request$", E, ""),
    ("DELETE", r"/import$", A, ""),
    # ---- protections
    ("DELETE", r"/branches/{v}/protection$", O, "branch protection goes"),
    ("DELETE", r"/branches/{v}/protection/restrictions$", X, "anyone with write may push"),
    ("DELETE", r"/branches/{v}/protection/", O, "a branch protection rule goes"),
    ("PUT", r"/branches/{v}/protection$", E, "replaces the protection, may weaken it"),
    ("PATCH", r"/branches/{v}/protection/required_pull_request_reviews$", E, ""),
    ("DELETE", r"/rulesets/{v}$", O, "the ruleset goes"), ("PUT", r"/rulesets/{v}$", E, "may weaken it"),
    ("DELETE", r"/environments/{v}/deployment_protection_rules/{v}$", O, ""),
    ("DELETE", r"/environments/{v}/deployment-branch-policies/{v}$", E, ""),
    ("PUT", r"/environments/{v}$", E, "may drop its reviewers"),
    ("DELETE", r"/vulnerability-alerts$", E, ""), ("DELETE", r"/immutable-releases", E, ""),
    ("POST", r"/secret-scanning/push-protection-bypasses$", X, "lets a secret be pushed"),
    ("PATCH", r"/secret-scanning/alerts/{v}$", E, "may dismiss an alert"),
    ("PATCH", r"/(code-scanning|dependabot)/alerts/{v}$", E, ""),
    # ---- access given
    ("PUT", r"/collaborators/{v}$", X, ""), ("PUT", r"/outside_collaborators/{v}$", X, ""),
    ("PUT", r"/memberships/{v}$", X, ""), ("PUT", r"/members/{v}$", X, ""),
    ("POST", r"/invitations$", X, ""), ("PATCH", r"/invitations/{v}$", X, "changes the invitation's permission"),
    ("PUT", r"/teams/{v}/repos/{v}/{v}$", X, ""), ("PUT", r"^/orgs/{v}/organization-roles/", X, ""),
    ("PUT", r"/security-managers/teams/{v}$", X, ""), ("POST", r"/copilot-spaces/{v}/collaborators$", X, ""),
    ("PUT", r"/copilot-spaces/{v}/collaborators/", X, ""),
    ("POST", r"/keys$", X, "a deploy or SSH key"), ("POST", r"^/user/(gpg_keys|ssh_signing_keys)$", A, ""),
    ("POST", r"/hooks$", X, "sends events to a URL"), ("PATCH", r"/hooks/{v}(/config)?$", X, "may change where events go"),
    ("POST", r"/runners/(registration-token|generate-jitconfig)$", X, "lets a machine run jobs"),
    ("POST", r"/access_tokens$", X, ""), ("POST", r"^/applications/{v}/token/scoped$", X, ""),
    ("POST", r"^/orgs/{v}/personal-access-token-requests", X, "approves a token's access"),
    ("POST", r"^/orgs/{v}/personal-access-tokens", E, ""),
    ("PUT", r"/secrets/{v}/repositories", X, "shares a secret with more repositories"),
    ("PUT", r"/variables/{v}/repositories", E, ""),
    ("PUT", r"/codespaces/access$", X, ""), ("POST", r"/codespaces/access/selected_users$", X, ""),
    ("PUT", r"/actions/permissions/(access|fork-pr-workflows-private-repos|fork-pr-contributor-approval)$", X, ""),
    ("PUT", r"/actions/permissions/workflow$", C, ""), ("PUT", r"/actions/permissions", E, ""),
    ("PUT", r"/actions/oidc/customization/sub$", E, ""), ("POST", r"/actions/oidc/customization/properties/repo$", E, ""),
    ("PUT", r"/runner-groups/{v}/repositories", E, ""), ("PUT", r"/public_members/{v}$", E, ""),
    ("POST", r"/pages$", E, "a public site, maybe from a private repository"), ("PUT", r"/pages$", E, ""),
    ("POST", r"/forks$", E, "copies the code under another owner"), ("POST", r"/generate$", A, ""),
    ("POST", r"/security-advisories/{v}/forks$", A, ""),
    # ---- what stops when removed
    ("DELETE", r"/collaborators/{v}$", O, ""), ("DELETE", r"/outside_collaborators/{v}$", O, ""),
    ("DELETE", r"/memberships/{v}$", O, ""), ("DELETE", r"/members/{v}$", O, ""), ("DELETE", r"/public_members/{v}$", A, ""),
    ("DELETE", r"/teams/{v}/repos/{v}/{v}$", O, ""), ("DELETE", r"^/orgs/{v}/teams/{v}$", O, ""), ("DELETE", r"^/teams/{v}$", O, ""),
    ("DELETE", r"/organization-roles/", O, ""), ("DELETE", r"/security-managers/", O, ""),
    ("DELETE", r"/secrets/{v}$", O, "workflows that use it fail"), ("DELETE", r"/variables/{v}$", O, ""),
    ("DELETE", r"/(secrets|variables)/{v}/repositories/{v}$", O, ""),
    ("DELETE", r"/hooks/{v}$", O, ""), ("DELETE", r"/keys/{v}$", O, ""), ("DELETE", r"^/user/(gpg_keys|ssh_signing_keys|keys)/{v}$", O, ""),
    ("DELETE", r"/runners/{v}$", O, ""), ("DELETE", r"/runner-groups/{v}$", O, ""), ("DELETE", r"/hosted-runners/{v}$", O, ""),
    ("DELETE", r"/runner-groups/{v}/(repositories|runners)/{v}$", O, ""),
    ("DELETE", r"/installations/{v}$", O, ""), ("PUT", r"/installations/{v}/suspended$", O, ""),
    ("DELETE", r"^/applications/{v}/(grant|token)$", O, ""), ("POST", r"^/credentials/revoke$", O, ""),
    ("DELETE", r"/private-registries/{v}$", O, ""), ("DELETE", r"/settings/network-configurations/{v}$", O, ""),
    ("PUT", r"/actions/workflows/{v}/disable$", O, ""), ("DELETE", r"/actions/policies/{v}$", E, ""),
    ("DELETE", r"/actions/permissions/(repositories|self-hosted-runners/repositories)/{v}$", O, ""),
    ("DELETE", r"/copilot/coding-agent/permissions/repositories/{v}$", A, ""),
    ("DELETE", r"/properties/schema/{v}$", E, ""), ("DELETE", r"/code-security/configurations", E, ""),
    ("DELETE", r"/(settings/billing/budgets|budgets)/{v}$", M, "removes a spending cap"),
    ("PATCH", r"/settings/billing/budgets/{v}$", M, "may raise a spending cap"),
    ("POST", r"/copilot/billing/selected_(users|teams)$", M, "adds paid seats"),
    ("DELETE", r"/copilot/billing/selected_", A, ""), ("POST", r"/actions/hosted-runners$", M, ""),
    ("PUT", r"/actions/cache/(retention|storage)-limit$", E, ""),
    ("PUT", r"/artifact-and-log-retention$", E, "a shorter retention deletes older logs"),
    ("DELETE", r"/environments/{v}/(secrets|variables)/{v}$", O, ""),
    ("POST", r"/codespaces/{v}/stop$", O, ""), ("POST", r"^/user/codespaces/{v}/stop$", O, ""),
    ("DELETE", r"/interaction-limits", A, ""), ("DELETE", r"/invitations/{v}$", A, ""),
    ("DELETE", r"/copilot-spaces/{v}$", G, ""), ("DELETE", r"/copilot-spaces/{v}/(collaborators|resources)/", O, ""),
    ("DELETE", r"/agents/repos/{v}/{v}/automations/{v}$", O, ""), ("DELETE", r"/campaigns/{v}$", A, ""),
    ("DELETE", r"/issue-(fields|types)/{v}$", E, ""), ("DELETE", r"/autolinks/{v}$", A, ""),
    ("DELETE", r"/milestones/{v}$", A, ""), ("DELETE", r"/labels/{v}$", A, ""),
    ("DELETE", r"/(comments|issues/comments|pulls/comments|gists/{v}/comments)/{v}$", E, "someone's comment"),
    ("DELETE", r"/pulls/{v}/reviews/{v}$", A, "a pending review"),
    ("DELETE", r"/secret-scanning/custom-patterns", E, ""), ("DELETE", r"/blocks/{v}$", A, ""),
    ("DELETE", r"/private-vulnerability-reporting$", A, ""), ("DELETE", r"/automated-security-fixes$", A, ""),
    ("DELETE", r"/oidc/customization/properties/repo/{v}$", E, ""),
    ("DELETE", r"/hosted-runners/images/custom/", E, ""), ("DELETE", r"/properties/installations/values/{v}$", A, ""),
    ("DELETE", r"/agent-merge$", A, ""), ("DELETE", r"^/user/(emails|following|starred|social_accounts|repository_invitations)", A, ""),
    ("DELETE", r"/suspended$", A, ""), ("DELETE", r"^/installation/token$", A, ""), ("DELETE", r"/organizations/{v}/settings/billing", M, ""),
    ("DELETE", r"/projectsV2/{v}/items/{v}$", E, ""), ("DELETE", r"/teams/{v}/copilot/customizations$", A, ""),
    ("DELETE", r"/copilot/policies/coding_agent/organizations$", A, ""),
    # ---- merges and deploys (#353's merges-and-deploys-held)
    ("PUT", r"/pulls/{v}/merge(-async)?$", D, ""), ("POST", r"/merges$", D, ""), ("POST", r"/merge-upstream$", D, ""),
    ("PUT", r"/agent-merge$", D, ""), ("POST", r"/dispatches$", D, "starts a workflow"),
    ("POST", r"/actions/runs/{v}/(rerun|rerun-failed-jobs|approve)$", D, ""), ("POST", r"/actions/jobs/{v}/rerun$", D, ""),
    ("POST", r"/actions/runs/{v}/(pending_deployments|deployment_protection_rule)$", D, "approves a deployment"),
    ("POST", r"/deployments$", D, ""), ("POST", r"/pages/(builds|deployments)$", D, ""),
    ("PUT", r"/pulls/{v}/update-branch$", A, ""),
    ("PUT", r"/pulls/{v}/reviews/{v}/dismissals$", E, "removes an approval"),
    ("POST", r"/branches/{v}/rename$", E, ""), ("PUT", r"/topics$", A, ""),
    ("PATCH", r"^/user$", A, ""), ("PATCH", r"^/user/email/visibility$", E, ""),
    ("PUT", r"/import$", E, ""), ("PATCH", r"/import", E, ""),
    ("POST", r"/migrations$", E, "exports repositories to an archive"),
]
TABLE = [(m, re.compile(p.replace("{v}", V)), l, w) for m, p, l, w in TABLE]

SMALL = re.compile(r"/(reactions|labels|assignees|star|subscription|pin|lock|requested_reviewers|notifications|"
                   r"threads|relates_to|blocked_by|sub_issue|issue-field-values|artifacts|interaction-limits|"
                   r"customizations)(/|$)")


def label(op):
    m, p = op["method"], op["path"]
    for mm, rx, lab, why in TABLE:
        if mm == m and rx.search(p):
            return lab, why
    if SMALL.search(p):
        return A, "small"
    if m == "DELETE":
        return E, "removes something"
    if m in ("PUT", "PATCH"):
        return E if re.search(r"(protection|permissions|policies|settings|config|secrets|variables)", p) else A, ""
    return A, ""


# GraphQL mutations (octokit/graphql-schema, the public schema; 242 mutations)
GQL = {
    G: "deleteIssue deleteDiscussion deleteProject deleteProjectV2 deleteTeamDiscussion deleteEnvironment",
    R: "deletePackageVersion deleteRef",
    O: "archiveRepository removeOutsideCollaborator removeEnterpriseMember removeEnterpriseAdmin removeEnterpriseOrganization "
       "revokeMigratorRole revokeEnterpriseOrganizationsMigratorRole deleteBranchProtectionRule deleteRepositoryRuleset "
       "deleteVerifiableDomain deleteIpAllowListEntry regenerateEnterpriseIdentityProviderRecoveryCodes "
       "regenerateVerifiableDomainToken removeEnterpriseSupportEntitlement",
    X: "grantMigratorRole grantEnterpriseOrganizationsMigratorRole inviteEnterpriseAdmin inviteEnterpriseMember "
       "addEnterpriseOrganizationMember updateTeamsRepository updateEnterpriseAdministratorRole "
       "updateEnterpriseOwnerOrganizationRole updateProjectV2Collaborators setEnterpriseIdentityProvider "
       "removeEnterpriseIdentityProvider transferEnterpriseOrganization createAttributionInvitation "
       "acceptEnterpriseAdministratorInvitation",
    M: "createSponsorship createSponsorships updateEnterpriseMembersCanMakePurchasesSetting",
    D: "mergeBranch mergePullRequest enqueuePullRequest enablePullRequestAutoMerge approveDeployments",
    C: "updateRefs updateRef",
    E: "deleteIssueComment deleteDiscussionComment deletePullRequestReviewComment deleteTeamDiscussionComment "
       "deleteProjectV2Field dismissPullRequestReview dismissRepositoryVulnerabilityAlert updateBranchProtectionRule "
       "updateRepositoryRuleset updateRepository updateIpAllowListEnabledSetting updateIpAllowListForInstalledAppsEnabledSetting "
       "updateEnterpriseAllowPrivateRepositoryForkingSetting updateEnterpriseDefaultRepositoryPermissionSetting "
       "updateEnterpriseMembersCanChangeRepositoryVisibilitySetting updateEnterpriseMembersCanCreateRepositoriesSetting "
       "updateEnterpriseMembersCanDeleteIssuesSetting updateEnterpriseMembersCanDeleteRepositoriesSetting "
       "updateEnterpriseMembersCanInviteCollaboratorsSetting updateEnterpriseMembersCanUpdateProtectedBranchesSetting "
       "updateEnterpriseTwoFactorAuthenticationRequiredSetting updateOrganizationAllowPrivateRepositoryForkingSetting "
       "updateEnvironment createIpAllowListEntry updateIpAllowListEntry createCommitOnBranch startRepositoryMigration "
       "startOrganizationMigration createMigrationSource transferIssue importProject updatePullRequestBranch "
       "updateEnterpriseMembersCanViewDependencyInsightsSetting updateNotificationRestrictionSetting "
       "updateOrganizationWebCommitSignoffSetting updateRepositoryWebCommitSignoffSetting deleteProjectV2Workflow",
}
GQL = {m: lab for lab, names in GQL.items() for m in names.split()}


def gql_ops():
    d = json.load(open("/opt/specs/github-gql.json"))
    s = d.get("data", d)["__schema"]
    t = [x for x in s["types"] if x["name"] == s["mutationType"]["name"]][0]
    return sorted(f["name"] for f in t["fields"])


def main():
    ops = json.load(open(os.path.join(HERE, "ops", "github.json")))
    with open(os.path.join(HERE, "labels", "github.txt"), "w") as f:
        f.write("# GitHub REST (api.github.com.json 1.1.4), every write operation, labelled on 2026-11-25.\n"
                "# Key: METHOD path. An ordered table (label_github.py), first match wins.\n")
        for op in ops:
            lab, why = label(op)
            f.write("%s %s %s %s\n" % (op["method"], op["path"], lab, why))
    names = gql_ops()
    assert not set(GQL) - set(names), set(GQL) - set(names)
    with open(os.path.join(HERE, "labels", "github-gql.txt"), "w") as f:
        f.write("# GitHub GraphQL mutations (octokit/graphql-schema), labelled on 2026-11-25. Not listed: allow.\n")
        for n in names:
            f.write("%s %s\n" % (n, GQL.get(n, A)))
    json.dump([{"provider": "github-gql", "kind": "graphql", "name": n, "host": "api.github.com", "path": "/graphql"} for n in names],
              open(os.path.join(HERE, "ops", "github-gql.json"), "w"), indent=0)
    print("github", len(ops), "github-gql", len(names))


if __name__ == "__main__":
    main()
