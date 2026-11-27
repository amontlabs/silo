"""Tests of classify.py.   python3 -m unittest test_classify -v"""

import base64
import gzip
import json
import os
import subprocess
import tempfile
import unittest

from classify import GitHubHosts, Request, Scope, classify, decide, foreign_credentials

NAMED_NODE = "R_kgDOAAAAAg"
SCOPE = Scope(repos={"victim/app"}, repo_ids={"42": "victim/app"}, repo_nodes={NAMED_NODE: "victim/app"})
JSON = [("Content-Type", "application/json")]


def req(method, target, body=b"", host="api.github.com", headers=JSON):
    if isinstance(body, (dict, list)):
        body = json.dumps(body).encode()
    return Request(method, host, target, list(headers), body)


def gql(query, variables=None, **extra):
    body = dict(query=query, **extra)
    if variables is not None:
        body["variables"] = variables
    return req("POST", "/graphql", body)


class Base(unittest.TestCase):
    def check(self, r, effect, decision, scope=SCOPE, **kw):
        v = classify(r, **kw)
        d, why = decide(v, scope)
        self.assertEqual((v.effect, d), (effect, decision), f"{v} -> {why}")
        return v


class Rest(Base):
    def test_new_repositories_are_held(self):
        self.check(req("POST", "/user/repos", {"name": "loot", "private": False}), "create", "hold")
        self.check(req("POST", "/orgs/acme/repos", {"name": "loot"}), "create", "hold")
        # The path names a repository the secret may use, and the request still makes a new one.
        self.check(req("POST", "/repos/victim/app/generate", {"name": "copy"}), "create", "hold")
        self.check(req("POST", "/repos/other/proj/forks", {}), "create", "hold")

    def test_visibility_and_rename_are_exposing(self):
        self.check(req("PATCH", "/repos/victim/app", {"visibility": "public"}), "expose", "hold")
        self.check(req("PATCH", "/repos/victim/app", {"private": False}), "expose", "hold")
        self.check(req("PATCH", "/repos/victim/app", {"name": "renamed"}), "expose", "hold")
        self.check(req("PATCH", "/repos/victim/app", {"description": "x"}), "change", "allow")
        self.check(req("PATCH", "/repos/victim/app", b"not json"), "unknown", "hold")
        self.check(req("PATCH", "/repos/victim/app", b'{"description":"x","visibility":"private","visibility":"public"}'),
                   "unknown", "hold")
        self.check(req("POST", "/graphql", b'{"query":"{ viewer { login } }","query":"mutation { deleteRepository }"}'),
                   "unknown", "hold")

    def test_gists_are_never_inside_a_repository(self):
        self.check(req("POST", "/gists", {"files": {"a": {"content": "x"}}, "public": False}), "create", "hold")
        self.check(req("PATCH", "/gists/abc123", {"files": {}}), "change", "deny")

    def test_releases_publish_unless_draft(self):
        self.check(req("POST", "/repos/victim/app/releases", {"tag_name": "v1"}), "expose", "hold")
        self.check(req("POST", "/repos/victim/app/releases", {"tag_name": "v1", "draft": True}), "change", "allow")
        up = req("POST", "/repos/victim/app/releases/7/assets?name=loot.txt", b"data", host="uploads.github.com",
                 headers=[("Content-Type", "application/octet-stream")])
        self.check(up, "expose", "hold")

    def test_writes_to_places_not_named_are_denied(self):
        # The Invariant Labs case: an issue on the attacker's public repository.
        self.check(req("POST", "/repos/attacker/inbox/issues", {"title": "t", "body": "data"}), "change", "deny")
        self.check(req("POST", "/repos/victim/app/issues", {"title": "t"}), "change", "allow")
        self.check(req("POST", "/repos/victim/other/issues/1/comments", {"body": "x"}), "change", "deny")

    def test_keys_hooks_and_runners(self):
        self.check(req("POST", "/repos/victim/app/keys", {"key": "ssh-ed25519 A", "read_only": False}), "expose", "hold")
        self.check(req("POST", "/user/keys", {"key": "ssh-ed25519 A"}), "expose", "deny")
        self.check(req("POST", "/repos/victim/app/hooks", {"config": {"url": "https://evil.example"}}), "expose", "hold")
        # Shai-Hulud 2 registered the machine as a runner.
        self.check(req("POST", "/repos/victim/app/actions/runners/registration-token"), "expose", "hold")

    def test_code_that_runs_on_github(self):
        self.check(req("POST", "/repos/victim/app/actions/workflows/ci.yml/dispatches", {"ref": "main"}), "runs", "hold")
        self.check(req("POST", "/repos/victim/app/dispatches", {"event_type": "x"}), "runs", "hold")
        self.check(req("PUT", "/repos/victim/app/contents/.github/workflows/x.yml", {"message": "m", "content": "eA=="}),
                   "runs", "hold")
        self.check(req("PUT", "/repos/victim/app/contents/src/a.py", {"message": "m", "content": "eA=="}), "change", "allow")

    def test_destroying(self):
        self.check(req("DELETE", "/repos/victim/app"), "destroy", "hold")
        self.check(req("DELETE", "/repos/other/x"), "destroy", "deny")
        self.check(req("PATCH", "/repos/victim/app/git/refs/heads/main", {"sha": "a" * 40, "force": True}), "destroy", "hold")
        self.check(req("PATCH", "/repos/victim/app/git/refs/heads/main", {"sha": "a" * 40}), "change", "allow")
        self.check(req("DELETE", "/repos/victim/app/issues/1/labels/bug"), "change", "allow")

    def test_merges_are_held_by_their_tag(self):
        v = self.check(req("PUT", "/repos/victim/app/pulls/1/merge", {}), "change", "hold")
        self.assertIn("merge", v.tags)
        self.check(req("PUT", "/repos/victim/app/pulls/1/merge", {}), "change", "allow",
                   scope=Scope(repos={"victim/app"}, allow={"merge"}))

    def test_repository_by_id(self):
        # GitHub answers GET /repositories/42, but its description does not list it: held.
        self.check(req("GET", "/repositories/42"), "unknown", "hold")

    def test_a_path_that_names_two_operations_is_not_guessed(self):
        self.check(req("GET", "/repos/victim/app/branches/feature/x"), "read", "allow")
        self.check(req("DELETE", "/repos/victim/app/branches/feature/x/protection"), "destroy", "hold")
        self.check(req("PATCH", "/repos/victim/app/git/refs/heads/agent/fix", {"sha": "a" * 40}), "change", "allow")

    def test_reads(self):
        self.check(req("GET", "/user"), "read", "allow")
        self.check(req("GET", "/repos/victim/app/pulls"), "read", "allow")
        self.check(req("HEAD", "/repos/victim/app/releases/tags/v1"), "read", "allow")
        self.check(req("GET", "/repos/victim/private-two/contents/x"), "read", "deny")
        self.check(req("GET", "/repos/victim/private-two/contents/x"), "read", "allow",
                   scope=Scope(repos={"victim/app"}, reads="anywhere"))

    def test_names_are_one_spelling(self):
        self.check(req("POST", "/repos/Victim/APP/issues", {"title": "t"}), "change", "allow")
        for bad in ["/repos/victim%2Fapp/issues", "/repos/victim/app/../../attacker/x/issues", "//repos/attacker/x/issues",
                    "/repos/victim/app/issues;x", "/repos/victim/app/%2e%2e/x", "/repos/victim/app\\issues",
                    "/repos/victim/app/issues/"]:
            self.check(req("POST", bad, {"title": "t"}), "unknown", "hold")

    def test_method_override_is_not_read(self):
        r = req("GET", "/repos/victim/app", headers=JSON + [("X-HTTP-Method-Override", "DELETE")])
        self.check(r, "unknown", "hold")
        self.check(req("POST", "/repos/victim/app/issues?_method=DELETE", {"title": "t"}), "unknown", "hold")
        self.check(req("PROPFIND", "/repos/victim/app"), "unknown", "hold")

    def test_unknown_paths_are_held(self):
        self.check(req("POST", "/repos/victim/app/frobnicate", {}), "unknown", "hold")
        # The web host takes Git paths only: an API path sent to github.com is not read.
        self.check(req("POST", "/user/repos", {"name": "x"}, host="github.com"), "unknown", "hold")

    def test_enterprise_server(self):
        h = GitHubHosts.enterprise_server("ghe.example.com")
        self.check(req("POST", "/api/v3/user/repos", {"name": "x"}, host="ghe.example.com"), "create", "hold", hosts=h)
        self.check(req("POST", "/api/v3/repos/attacker/x/issues", {"t": 1}, host="ghe.example.com"), "change", "deny", hosts=h)
        self.check(req("POST", "/api/graphql", {"query": "mutation{createRepository(input:{name:\"x\",visibility:PUBLIC}){clientMutationId}}"},
                       host="ghe.example.com"), "create", "hold", hosts=h)
        # The same paths on github.com's hosts mean nothing to an Enterprise Server limit.
        self.check(req("POST", "/user/repos", {"name": "x"}, host="ghe.example.com"), "unknown", "hold", hosts=h)


class GraphQL(Base):
    CREATE = 'mutation { createRepository(input:{name:"loot", visibility:PUBLIC}) { repository { url } } }'

    def test_mutations_are_read_whatever_their_spelling(self):
        self.check(gql(self.CREATE), "create", "hold")
        self.check(gql('mutation M { innocent: createRepository(input:{name:"x",visibility:PUBLIC}) { clientMutationId } }'),
                   "create", "hold")
        self.check(gql('mutation { ...F } fragment F on Mutation { createRepository(input:{name:"x",visibility:PUBLIC}) { clientMutationId } }'),
                   "create", "hold")
        self.check(gql('mutation { ... on Mutation { createRepository(input:{name:"x",visibility:PUBLIC}) { clientMutationId } } }'),
                   "create", "hold")
        # operationName names the harmless operation; the host might run the other.
        self.check(gql('query A { viewer { login } } ' + self.CREATE.replace("mutation", "mutation B"), operationName="A"),
                   "create", "hold")

    def test_batches_persisted_queries_and_transports(self):
        self.check(req("POST", "/graphql", [{"query": "{ viewer { login } }"}, {"query": self.CREATE}]), "create", "hold")
        self.check(req("POST", "/graphql", {"extensions": {"persistedQuery": {"version": 1, "sha256Hash": "ab"}}}),
                   "unknown", "hold")
        self.check(req("POST", "/graphql", {"id": "doc-1"}), "unknown", "hold")
        self.check(req("GET", "/graphql?query=" + self.CREATE.replace(" ", "%20").replace('"', "%22").replace("{", "%7B").replace("}", "%7D")),
                   "create", "hold")
        z = Request("POST", "api.github.com", "/graphql", JSON + [("Content-Encoding", "gzip")],
                    gzip.compress(json.dumps({"query": self.CREATE}).encode()))
        self.check(z, "create", "hold")
        raw = Request("POST", "api.github.com", "/graphql", [("Content-Type", "application/graphql")], self.CREATE.encode())
        self.check(raw, "create", "hold")
        self.check(gql("subscription { x }"), "unknown", "hold")
        self.check(gql("mutation { createRepository(input:"), "unknown", "hold")
        self.check(gql('mutation { createRepositoryX(input:{}) { a } }'), "unknown", "hold")
        self.check(gql('mutation { ...F } fragment F on Mutation { ...F }'), "unknown", "hold")

    def test_mutation_inside_a_named_repository(self):
        q = 'mutation($i: CreateIssueInput!) { createIssue(input: $i) { issue { id } } }'
        self.check(gql(q, {"i": {"repositoryId": NAMED_NODE, "title": "t"}}), "change", "allow")
        v = self.check(gql(q, {"i": {"repositoryId": "R_kgDOZZZZZZ", "title": "t"}}), "change", "hold")
        self.assertEqual(v.lookups, ["R_kgDOZZZZZZ"])
        looked = Scope(repos={"victim/app"}, repo_nodes={NAMED_NODE: "victim/app"}, node_repo={"R_kgDOZZZZZZ": "attacker/inbox"})
        self.check(gql(q, {"i": {"repositoryId": "R_kgDOZZZZZZ", "title": "t"}}), "change", "deny", scope=looked)

    def test_mutation_on_a_node_needs_a_lookup(self):
        q = 'mutation { addComment(input:{subjectId:"I_kwDOAAAA", body:"data"}) { clientMutationId } }'
        self.check(gql(q), "change", "hold")
        for repo, decision in (("victim/app", "allow"), ("attacker/inbox", "deny"), ("", "deny")):
            s = Scope(repos={"victim/app"}, node_repo={"I_kwDOAAAA": repo})
            self.check(gql(q), "change", decision, scope=s)

    def test_body_facts(self):
        self.check(gql('mutation { updateRepository(input:{repositoryId:"%s", name:"x"}) { clientMutationId } }' % NAMED_NODE),
                   "expose", "hold")
        self.check(gql('mutation { updateRepository(input:{repositoryId:"%s", description:"x"}) { clientMutationId } }' % NAMED_NODE),
                   "change", "allow")
        self.check(gql('mutation { createIssue(input:{repositoryId:"%s", title:"t", agentAssignment:{targetRepositoryId:"%s"}}) { clientMutationId } }'
                       % (NAMED_NODE, NAMED_NODE)), "runs", "hold")
        self.check(gql('mutation { createCommitOnBranch(input:{branch:{repositoryNameWithOwner:"attacker/x", branchName:"m"}, expectedHeadOid:"a", message:{headline:"h"}}) { clientMutationId } }'),
                   "change", "deny")
        self.check(gql('mutation { createCommitOnBranch(input:{branch:{repositoryNameWithOwner:"Victim/App", branchName:"m"}, expectedHeadOid:"a", message:{headline:"h"}}) { clientMutationId } }'),
                   "change", "allow")
        self.check(gql('mutation { createSponsorship(input:{sponsorableLogin:"x", amount:5}) { clientMutationId } }'),
                   "expose", "deny")

    def test_queries(self):
        self.check(gql("{ viewer { login } }"), "read", "allow")
        self.check(gql('{ repository(owner:"victim", name:"app") { issues(first:1){ nodes { title } } } }'), "read", "allow")
        self.check(gql('{ repository(owner:"victim", name:"secret-two") { id } }'), "read", "deny")
        self.check(gql("{ viewer { repositories(first:100) { nodes { name } } } }"), "read", "deny")
        self.check(gql("{ viewer { repositories(first:100) { nodes { name } } } }"), "read", "allow",
                   scope=Scope(repos={"victim/app"}, reads="anywhere"))


def git(cwd, *args):
    return subprocess.run(["git", "-C", cwd, *args], check=True, capture_output=True, text=True,
                          env={"PATH": os.environ["PATH"], "GIT_AUTHOR_NAME": "v", "GIT_AUTHOR_EMAIL": "v@e",
                               "GIT_COMMITTER_NAME": "v", "GIT_COMMITTER_EMAIL": "v@e", "HOME": cwd}).stdout.strip()


def pkt(s):
    b = s.encode() if isinstance(s, str) else s
    return b"%04x" % (len(b) + 4) + b


class Git(Base):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        d = cls.d = cls.tmp.name
        git(d, "init", "-q", "-b", "main")
        os.makedirs(f"{d}/.github/workflows")
        open(f"{d}/.github/workflows/ci.yml", "w").write("on: push\n")
        open(f"{d}/a.txt", "w").write("a\n")
        git(d, "add", "-A"); git(d, "commit", "-qm", "base")
        cls.base = git(d, "rev-parse", "HEAD")
        open(f"{d}/a.txt", "a").write("b\n"); git(d, "commit", "-qam", "edit")
        cls.edit = git(d, "rev-parse", "HEAD")
        open(f"{d}/.github/workflows/ci.yml", "a").write("# x\n"); git(d, "commit", "-qam", "wf")
        cls.wf = git(d, "rev-parse", "HEAD")
        git(d, "checkout", "-q", "-b", "other", cls.base)
        open(f"{d}/b.txt", "w").write("b\n"); git(d, "add", "-A"); git(d, "commit", "-qm", "other")
        cls.other = git(d, "rev-parse", "HEAD")

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def tree_of(self, place, commit):
        commit = self.base if commit == "default-branch" else commit
        gh = git(self.d, "rev-parse", f"{commit}:.github")
        return (gh, {"workflows": git(self.d, "rev-parse", f"{commit}:.github/workflows")})

    def push(self, commands, revs, thin=False, path="/victim/app.git/git-receive-pack", options=None):
        caps = "report-status side-band-64k" + (" push-options" if options else "")
        body = b""
        for i, (old, new, ref) in enumerate(commands):
            line = f"{old} {new} {ref}"
            body += pkt(line + ("\0" + caps if i == 0 else "") + "\n")
        body += b"0000"
        if options:
            body += b"".join(pkt(o + "\n") for o in options) + b"0000"
        if revs:
            args = ["pack-objects", "-q", "--revs", "--stdout"] + (["--thin"] if thin else [])
            body += subprocess.run(["git", "-C", self.d] + args, input="\n".join(revs).encode() + b"\n",
                                   capture_output=True, check=True).stdout
        return Request("POST", "github.com", path, [("Content-Type", "application/x-git-receive-pack-request")], body)

    def test_push_inside_a_named_repository(self):
        r = self.push([(self.base, self.edit, "refs/heads/main")], [self.edit, "^" + self.base])
        self.check(r, "change", "allow", tree_of=self.tree_of)

    def test_push_to_the_default_branch_is_a_merge(self):
        r = self.push([(self.base, self.edit, "refs/heads/main")], [self.edit, "^" + self.base])
        s = Scope(repos={"victim/app"}, default_branches={"victim/app": "main"})
        self.check(r, "change", "hold", scope=s, tree_of=self.tree_of)
        r = self.push([(self.base, self.edit, "refs/heads/agent/x")], [self.edit, "^" + self.base])
        self.check(r, "change", "allow", scope=s, tree_of=self.tree_of)

    def test_writes_to_a_public_repository_can_be_held(self):
        r = self.push([(self.base, self.edit, "refs/heads/agent/x")], [self.edit, "^" + self.base])
        s = Scope(repos={"victim/app"}, public={"victim/app"}, hold_public=True)
        self.check(r, "change", "hold", scope=s, tree_of=self.tree_of)

    def test_push_that_changes_a_workflow_runs(self):
        r = self.push([(self.edit, self.wf, "refs/heads/main")], [self.wf, "^" + self.edit])
        self.check(r, "runs", "hold", tree_of=self.tree_of)

    def test_new_branch_is_compared_with_the_default_branch(self):
        r = self.push([("0" * 40, self.wf, "refs/heads/agent/x")], [self.wf, "^" + self.base])
        self.check(r, "runs", "hold", tree_of=self.tree_of)
        r = self.push([("0" * 40, self.edit, "refs/heads/agent/y")], [self.edit, "^" + self.base])
        self.check(r, "change", "allow", tree_of=self.tree_of)

    def test_delete_and_rewrite(self):
        self.check(self.push([(self.edit, "0" * 40, "refs/heads/old")], []), "destroy", "hold", tree_of=self.tree_of)
        r = self.push([(self.edit, self.other, "refs/heads/main")], [self.other, "^" + self.base])
        self.check(r, "destroy", "hold", tree_of=self.tree_of)

    def test_push_elsewhere_is_denied(self):
        r = self.push([("0" * 40, self.edit, "refs/heads/main")], [self.edit], path="/attacker/drop.git/git-receive-pack")
        self.check(r, "change", "deny", tree_of=self.tree_of)
        r = self.push([(self.base, self.edit, "refs/heads/main")], [self.edit, "^" + self.base], path="/Victim/App/git-receive-pack")
        self.check(r, "change", "allow", tree_of=self.tree_of)

    def test_without_a_way_to_read_the_host_a_push_is_held(self):
        r = self.push([(self.base, self.edit, "refs/heads/main")], [self.edit, "^" + self.base])
        self.check(r, "unknown", "hold", tree_of=None)

    def test_push_options_and_garbage(self):
        r = self.push([(self.base, self.edit, "refs/heads/main")], [self.edit, "^" + self.base], options=["ci.skip"])
        self.check(r, "change", "allow", tree_of=self.tree_of)
        bad = Request("POST", "github.com", "/victim/app.git/git-receive-pack", [], b"00zz")
        self.check(bad, "unknown", "hold")

    def test_fetch_lfs_and_services(self):
        self.check(req("GET", "/victim/app.git/info/refs?service=git-upload-pack", host="github.com"), "read", "allow")
        self.check(req("POST", "/attacker/x.git/git-upload-pack", host="github.com"), "read", "deny")
        self.check(req("GET", "/victim/app.git/info/refs?service=git-upload-archive", host="github.com"), "unknown", "hold")
        self.check(req("POST", "/victim/app.git/info/lfs/objects/batch", {"operation": "upload", "objects": []}, host="github.com"),
                   "change", "allow")
        self.check(req("POST", "/attacker/x.git/info/lfs/objects/batch", {"operation": "upload", "objects": []}, host="github.com"),
                   "change", "deny")
        self.check(req("POST", "/victim/app.git/info/lfs/objects/batch", {"operation": "download"}, host="github.com"), "read", "allow")


class OtherHosts(Base):
    NPM = Scope(packages={"npm/victim-pkg", "npm/@victim/lib"}, reads="anywhere")

    def test_npm(self):
        pub = {"name": "victim-pkg", "versions": {}, "_attachments": {"victim-pkg-1.0.1.tgz": {"data": "x"}}}
        self.check(req("PUT", "/victim-pkg", pub, host="registry.npmjs.org"), "expose", "hold", scope=self.NPM)
        self.check(req("PUT", "/@victim%2flib", pub, host="registry.npmjs.org"), "expose", "hold", scope=self.NPM)
        # Shai-Hulud republished every package its stolen token could publish.
        self.check(req("PUT", "/someone-elses-pkg", pub, host="registry.npmjs.org"), "expose", "deny", scope=self.NPM)
        self.check(req("PUT", "/-/package/victim-pkg/dist-tags/latest", b'"6.6.6"', host="registry.npmjs.org"),
                   "expose", "hold", scope=self.NPM)
        self.check(req("POST", "/-/npm/v1/tokens", {"password": "x"}, host="registry.npmjs.org"), "expose", "deny", scope=self.NPM)
        self.check(req("DELETE", "/victim-pkg/-rev/3-abc", host="registry.npmjs.org"), "destroy", "hold", scope=self.NPM)
        self.check(req("GET", "/left-pad", host="registry.npmjs.org"), "read", "allow", scope=self.NPM)

    def test_pypi(self):
        def upload(name):
            b = "XyZ"
            body = (f'--{b}\r\nContent-Disposition: form-data; name=":action"\r\n\r\nfile_upload\r\n'
                    f'--{b}\r\nContent-Disposition: form-data; name="name"\r\n\r\n{name}\r\n'
                    f'--{b}\r\nContent-Disposition: form-data; name="content"; filename="x.tar.gz"\r\n\r\nDATA\r\n--{b}--\r\n')
            return Request("POST", "upload.pypi.org", "/legacy/", [("Content-Type", f"multipart/form-data; boundary={b}")], body.encode())
        s = Scope(packages={"pypi/victim-pkg"}, reads="anywhere")
        self.check(upload("Victim_Pkg"), "expose", "hold", scope=s)
        self.check(upload("other"), "expose", "deny", scope=s)

    def test_s3(self):
        s = Scope(buckets={"victim-builds"}, reads="anywhere")
        self.check(Request("PUT", "attacker-drop.s3.amazonaws.com", "/loot.txt", [], b"x"), "change", "deny", scope=s)
        self.check(Request("PUT", "victim-builds.s3.us-west-2.amazonaws.com", "/ok.txt", [], b"x"), "change", "allow", scope=s)
        self.check(Request("PUT", "s3.amazonaws.com", "/attacker-drop/loot.txt", [], b"x"), "change", "deny", scope=s)
        self.check(Request("PUT", "victim-builds.s3.amazonaws.com", "/?policy", [], b"{}"), "expose", "hold", scope=s)
        self.check(Request("PUT", "victim-builds.s3.amazonaws.com", "/k", [("x-amz-acl", "public-read")], b"x"), "expose", "hold", scope=s)
        self.check(Request("PUT", "new-bucket.s3.amazonaws.com", "/", [], b""), "create", "hold", scope=s)
        self.check(Request("PUT", "victim-builds.s3.amazonaws.com", "/copy", [("x-amz-copy-source", "/secret-bucket/k")], b""),
                   "change", "deny", scope=s)


class ForeignCredentials(unittest.TestCase):
    P = ["SILO_PLACEHOLDER_gh"]

    def f(self, headers, target="/user"):
        return foreign_credentials(Request("GET", "api.github.com", target, headers), self.P)

    def test(self):
        self.assertEqual(self.f([("Authorization", "token SILO_PLACEHOLDER_gh")]), [])
        self.assertEqual(self.f([("Authorization", "Bearer SILO_PLACEHOLDER_gh")]), [])
        basic = base64.b64encode(b"x-access-token:SILO_PLACEHOLDER_gh").decode()
        self.assertEqual(self.f([("Authorization", "Basic " + basic)]), [])
        self.assertEqual(self.f([]), [])
        self.assertTrue(self.f([("Authorization", "token ghp_attackersOwnToken")]))
        self.assertTrue(self.f([("Authorization", "Basic " + base64.b64encode(b"attacker:ghp_x").decode())]))
        self.assertTrue(self.f([("Authorization", "Basic " + base64.b64encode(b"ghp_tokenAsUser:").decode())]))
        self.assertTrue(self.f([("Cookie", "user_session=abc")]))
        self.assertTrue(self.f([], target="/user?access_token=ghp_x"))


if __name__ == "__main__":
    unittest.main()
