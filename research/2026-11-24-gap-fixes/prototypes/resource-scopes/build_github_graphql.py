"""Builds data/github-graphql.json: every mutation of GitHub's GraphQL schema, with its effect and
the input fields that name the place it acts on.

    python3 build_github_graphql.py PATH/TO/schema.docs.graphql

The schema is GitHub's public one: https://docs.github.com/public/fpt/schema.docs.graphql
Effects are the ones of build_github_rest.py.
"""

import collections
import json
import re
import sys

from graphql import build_schema
from graphql.type import GraphQLInputObjectType, GraphQLList, GraphQLNonNull

EXACT = {
    "createRepository": ("create", []),
    "cloneTemplateRepository": ("create", []),
    "createEnterpriseOrganization": ("create", []),
    "startRepositoryMigration": ("create", []),
    "startOrganizationMigration": ("create", []),
    "updateRepository": ("change", ["body"]),  # renaming is exposing: see classify.py
    "transferIssue": ("expose", []),
    "transferEnterpriseOrganization": ("expose", []),
    "archiveRepository": ("change", []),
    "unarchiveRepository": ("change", []),
    "mergePullRequest": ("change", ["merge"]),
    "mergeBranch": ("change", ["merge"]),
    "enablePullRequestAutoMerge": ("change", ["merge"]),
    "enqueuePullRequest": ("change", ["merge"]),
    "approveDeployments": ("runs", ["deploy"]),
    "createDeployment": ("change", ["deploy"]),
    "createDeploymentStatus": ("change", ["deploy"]),
    "requestCopilotCodeReview": ("runs", []),
    "rerequestCheckSuite": ("runs", []),
    "createIssue": ("change", ["body"]),  # agentAssignment starts an agent: see classify.py
    "replaceActorsForAssignable": ("change", ["body"]),
    "createSponsorship": ("expose", ["pays"]),
    "createSponsorships": ("expose", ["pays"]),
    "updateSubscription": ("change", ["account"]),
    "changeUserStatus": ("change", ["account"]),
    "followUser": ("change", ["account"]),
    "unfollowUser": ("change", ["account"]),
    "followOrganization": ("change", ["account"]),
    "unfollowOrganization": ("change", ["account"]),
    "addStar": ("change", ["account"]),
    "removeStar": ("change", ["account"]),
    "removeOutsideCollaborator": ("expose", []),
    "updateTeamsRepository": ("expose", []),
    "updateProjectV2Collaborators": ("expose", []),
    "createAttributionInvitation": ("expose", []),
    "addPullRequestCreationCapBypassUsers": ("expose", []),
    "accessUserNamespaceRepository": ("expose", []),
    "publishSponsorsTier": ("expose", ["publish"]),
    "createSponsorsListing": ("expose", ["publish"]),
    "createSponsorsTier": ("expose", ["publish"]),
}
EXPOSE_RX = re.compile(r"Enterprise|IpAllowList|VerifiableDomain|IdentityProvider|MigratorRole|Migration"
                       r"|BranchProtectionRule|RepositoryRuleset|Environment|InteractionLimit|Organization.*Setting"
                       r"|RecoveryCodes|CustomProperty|WebCommitSignoff|CopilotCodeReview(Policy|Runner|Settings)"
                       r"|NotificationRestriction|TeamReviewAssignment")
PLACE_RX = re.compile(r"(^|[a-z])(Id|Ids|NameWithOwner|Login)$|^id$|^ids$")


def unwrap(t):
    while isinstance(t, (GraphQLNonNull, GraphQLList)):
        t = t.of_type
    return t


def place_fields(t, prefix="", depth=0):
    out = []
    if depth > 3:
        return out
    for name, f in t.fields.items():
        inner = unwrap(f.type)
        if isinstance(inner, GraphQLInputObjectType):
            out += place_fields(inner, f"{prefix}{name}.", depth + 1)
        elif PLACE_RX.search(name) and name != "clientMutationId":
            out.append(f"{prefix}{name}")
    return out


def effect_of(name):
    if name in EXACT:
        return EXACT[name] + ("exact",)
    if EXPOSE_RX.search(name):
        return "expose", [], "name names a setting of who may do what"
    if name.startswith("delete"):
        return "destroy", [], "name starts with delete"
    if re.match(r"(add|remove|update|set|create|close|reopen|lock|unlock|mark|unmark|pin|unpin|minimize|unminimize"
                r"|resolve|unresolve|dismiss|submit|request|rerequest|convert|link|unlink|move|reorder|archive"
                r"|unarchive|clear|apply|reject|accept|decline|cancel|abort|revert|dequeue|disable|import|clone"
                r"|copy|reprioritize|regenerate|retire|grant|revoke|invite|verify|approve|promote)", name):
        return "change", [], "name starts with a verb that edits"
    return "unknown", [], "no rule"


def main(src):
    s = build_schema(open(src).read())
    muts, counts, why = {}, collections.Counter(), collections.Counter()
    for name, f in sorted(s.mutation_type.fields.items()):
        effect, tags, reason = effect_of(name)
        fields = []
        for arg_name, arg in f.args.items():
            inner = unwrap(arg.type)
            if isinstance(inner, GraphQLInputObjectType):
                fields += [f"{arg_name}.{p}" for p in place_fields(inner)]
        muts[name] = {"effect": effect, "tags": tags, "places": fields, "why": reason}
        counts[effect] += 1
        why[reason] += 1
    json.dump({"source": "docs.github.com/public/fpt/schema.docs.graphql", "mutations": muts,
               "query_fields": sorted(s.query_type.fields)}, open("data/github-graphql.json", "w"), indent=0)
    print(f"{len(muts)} mutations")
    for k, v in counts.most_common():
        print(f"  {k:8} {v}")
    print("by what decided it:", dict(why))
    anchor = collections.Counter()
    for m in muts.values():
        p = m["places"]
        anchor["input.repositoryId" in p and "repositoryId" or
               ("names a place only by another node id" if p else "names no place")] += 1
    print("how a mutation names its place:", dict(anchor))
    for n, m in muts.items():
        if m["effect"] == "unknown":
            print("   unknown:", n)


if __name__ == "__main__":
    main(sys.argv[1])
