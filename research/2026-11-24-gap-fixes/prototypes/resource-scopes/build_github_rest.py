"""Builds data/github-rest.json: every operation of GitHub's REST description, with its effect.

    python3 build_github_rest.py PATH/TO/api.github.com.json

The description is github/rest-api-description, descriptions/api.github.com/api.github.com.json.
Each operation gets one effect, and says which line of RULES gave it, so that a reviewer can see
which ones rest on a named rule and which on a fallback:

    read     changes nothing at GitHub
    change   changes something inside the place the path names (a repository, the account)
    create   makes a new place: a repository, a fork, a gist, a codespace, a team
    expose   changes who can read or reach something: visibility, collaborators, keys, webhooks,
             published releases and Pages, runner tokens, org membership, security settings
    runs     makes code run outside Silo: workflow dispatch, re-runs, agent tasks, codespaces
    destroy  deletes something GitHub may not bring back

Tags add facts that other rule sets use: merge, deploy, publish, account, body (the effect
depends on the body, which classify.py reads).
"""

import collections
import json
import re
import sys

# (methods, regex on the path template, effect, tags). The first line that matches wins.
RULES = [
    # Requests sent as POST that only read.
    ("POST", r"^/markdown(/raw)?$|/attestations/bulk-list$", "read", []),
    ("POST", r"/actions/oidc/customization/sub$|/code-scanning/codeql/variant-analyses$", None, []),
    # New places.
    ("POST", r"^/user/repos$|^/orgs/\{org\}/repos$", "create", []),
    ("POST", r"^/repos/\{template_owner\}/\{template_repo\}/generate$|^/repos/\{owner\}/\{repo\}/generate$", "create", []),
    ("POST", r"^/repos/\{owner\}/\{repo\}/forks$", "create", ["body"]),
    ("POST", r"^/gists$|^/gists/\{gist_id\}/forks$", "create", []),
    ("POST", r"^/orgs/\{org\}/teams$", "create", []),
    ("POST", r"^/user/migrations$|^/orgs/\{org\}/migrations$", "create", []),
    ("POST", r"/security-advisories/\{ghsa_id\}/forks$", "create", []),
    ("POST", r"codespaces", "runs", []),
    ("PUT|POST|PATCH", r"^/agents/|/copilot/.*tasks|/agent-tasks|/automations", "runs", []),
    # Code that runs on GitHub's machines.
    ("POST", r"/dispatches$|/rerun(-failed-jobs)?$|/rerequest$|/hooks/\{hook_id\}/(pings|tests)$|/deliveries/\{delivery_id\}/attempts$", "runs", []),
    ("PUT", r"/actions/workflows/\{workflow_id\}/enable$", "runs", []),
    ("POST", r"/actions/runs/\{run_id\}/(approve|deployment_protection_rule|pending_deployments)$", "runs", ["deploy"]),
    # Who can reach or read something.
    ("PATCH", r"^/repos/\{owner\}/\{repo\}$", "expose", ["body"]),
    ("POST", r"^/repos/\{owner\}/\{repo\}/transfer$", "expose", []),
    ("PUT|POST|PATCH", r"/collaborators/|/invitations|/outside_collaborators|/memberships/|/members/|/teams/.*/repos/", "expose", []),
    ("POST|PUT|PATCH", r"/keys(/\{key_id\})?$|^/user/(gpg_keys|ssh_signing_keys|emails)$", "expose", []),
    ("POST|PATCH|PUT", r"/hooks(/\{hook_id\}(/config)?)?$", "expose", []),
    ("POST|PUT", r"/pages(/builds|/deployments)?$", "expose", ["deploy", "publish"]),
    ("POST", r"/releases$", "expose", ["publish", "body"]),
    ("POST", r"/releases/\{release_id\}/assets$", "expose", ["publish"]),
    ("PATCH", r"/releases/\{release_id\}$", "expose", ["publish", "body"]),
    ("POST", r"/actions/runners/(registration-token|remove-token|generate-jitconfig)$", "expose", []),
    ("PUT|POST|PATCH", r"/actions/permissions|/actions/oidc|/environments/\{environment_name\}$|protection|deployment-branch-policies|/rulesets|/rules/", "expose", []),
    ("PUT|POST|PATCH", r"/security-advisories", "expose", ["publish"]),
    ("PUT|POST|PATCH", r"/installations/\{installation_id\}/repositories|/access_tokens$|/applications/", "expose", []),
    ("PUT|POST|PATCH", r"^/orgs/\{org\}/(secrets|variables|actions/secrets|actions/variables|dependabot/secrets|codespaces/secrets|agents/secrets|agents/variables)", "expose", []),
    ("PUT|PATCH|POST", r"^/orgs/\{org\}$|^/orgs/\{org\}/(settings|properties|personal-access-token|custom-repository-roles|organization-roles|security-managers|interaction-limits|private-registries|credentials|blocks)", "expose", []),
    ("PUT|PATCH|POST|DELETE", r"^/enterprises/", "expose", []),
    # Merges and deploys, inside the repository.
    ("PUT", r"/pulls/\{pull_number\}/merge(-async)?$", "change", ["merge"]),
    ("POST", r"^/repos/\{owner\}/\{repo\}/(merges|merge-upstream)$", "change", ["merge"]),
    ("POST", r"/deployments(/\{deployment_id\}/statuses)?$", "change", ["deploy"]),
    ("PUT", r"/contents/\{path\}$", "change", ["body"]),
    ("PATCH", r"/git/refs/\{ref\}$", "change", ["body"]),
    # Deletions that GitHub may not bring back, and deletions that are an ordinary edit.
    ("DELETE", r"/labels/\{name\}$|/assignees$|/requested_reviewers$|/reactions/\{reaction_id\}$|/lock$|/subscription$|/starred/|/following/|/blocks/|/notifications|/pin$|/sub_issue$|/topics$|/interaction-limits$|/contents/\{path\}$", "change", []),
    ("DELETE", r".", "destroy", []),
    # Places of their own, never inside a repository: gists and projects.
    ("PATCH|PUT|POST", r"^/gists/|projectsV2", "change", []),
    # Anything else an organization or an enterprise is told to do.
    ("PATCH|PUT|POST", r"^/orgs/\{org\}/|^/organizations/", "expose", []),
    # The account.
    ("PATCH|PUT|POST", r"^/user(/|$)|^/notifications|^/user/starred", "change", ["account"]),
]

FALLBACK_EXPOSE = {"orgs", "teams", "apps", "billing", "enterprise-admin", "copilot", "copilot-spaces",
                   "code-security", "private-registries", "campaigns", "credentials", "oidc",
                   "interactions", "migrations", "packages"}


def classify(method, path, op):
    for methods, rx, effect, tags in RULES:
        if method in methods.split("|") and re.search(rx, path):
            if effect is None:
                return "read", [], "rule: read sent as POST"
            return effect, tags, f"rule: {rx}"
    if method in ("GET", "HEAD"):
        return "read", [], "method"
    cat = op.get("x-github", {}).get("category", "")
    if cat in FALLBACK_EXPOSE:
        return "expose", [], f"fallback: category {cat}"
    if path.startswith("/repos/{owner}/{repo}/") or path.startswith("/repositories/"):
        return "change", [], "fallback: write inside a repository"
    return "unknown", [], "fallback: no rule"


def scope_of(path):
    if re.match(r"^/repos/\{(template_)?owner\}/\{(template_)?repo\}", path) or path.startswith("/agents/repos/{owner}/{repo}"):
        return "repo"
    if "{repository_id}" in path:
        return "repo-id"
    if path.startswith("/orgs/{org}") or path.startswith("/organizations/{org"):
        return "org"
    if path.startswith("/gists"):
        return "gist"
    if path.startswith("/user/") or path == "/user" or path.startswith("/notifications"):
        return "account"
    if path.startswith("/users/{username}"):
        return "other-user"
    if path.startswith("/enterprises/"):
        return "enterprise"
    if path.startswith("/app") or path.startswith("/installation") or path.startswith("/applications"):
        return "app"
    return "global"


BODY_PLACE_FIELDS = {"owner", "organization", "repository", "repositories", "repo", "repository_id",
                     "repository_ids", "selected_repository_ids", "include_repositories", "target", "head",
                     "new_owner", "new_name"}


def main(src):
    d = json.load(open(src))
    out, counts, by_reason, body_names = [], collections.Counter(), collections.Counter(), []
    for path, ops in d["paths"].items():
        for m, op in ops.items():
            if m not in ("get", "head", "post", "put", "patch", "delete"):
                continue
            method = m.upper()
            effect, tags, why = classify(method, path, op)
            props = set()
            try:
                schema = op["requestBody"]["content"]["application/json"]["schema"]
                props = set(schema.get("properties", {}))
            except (KeyError, TypeError):
                pass
            named = sorted(props & BODY_PLACE_FIELDS)
            if named and method != "GET":
                body_names.append((method, path, named))
            out.append({"method": method, "path": path, "op": op["operationId"], "effect": effect,
                        "tags": tags, "scope": scope_of(path), "why": why, "body_names_place": named})
            counts[effect] += 1
            by_reason[why.split(":")[0]] += 1
    json.dump({"source": f"github/rest-api-description {d['info']['version']}", "operations": out},
              open("data/github-rest.json", "w"), indent=0)
    print(f"{len(out)} operations from {d['info']['version']}")
    for k, v in counts.most_common():
        print(f"  {k:8} {v}")
    print("by what decided it:", dict(by_reason))
    print("scopes:", dict(collections.Counter(o["scope"] for o in out)))
    print(f"writes whose body names another place: {len(body_names)}")
    for b in body_names:
        print("   ", *b)
    print("fallbacks:")
    for o in out:
        if o["why"].startswith("fallback"):
            print("   ", o["effect"], o["method"], o["path"], "|", o["why"])


if __name__ == "__main__":
    main(sys.argv[1])
