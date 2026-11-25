#!/usr/bin/env python3
"""#353's capture server (../../../2026-11-24-destructive-calls/proto/capture/server.py), unchanged,
answering with a small mock per CLI so each command reaches the request that does its step:

- aws: the empty answer of each protocol (XML for query and REST-XML, {} for JSON), and an S3
  listing of two objects so "s3 rm --recursive", "rb --force" and "sync --delete" have something to delete.
- gcloud and bq: a finished operation for every write, and a resource with the fields gcloud reads
  before a write (fingerprints, etags, IAM policies, settings).
- az: an App Service managed identity endpoint (identity.capture.test) that hands out a made-up
  token built when asked, one subscription, ARM resources in state Succeeded, and the 401 challenge
  Key Vault sends first.
- gh: GraphQL run against GitHub's public schema with made-up values (as #383's server2.py does for
  Railway and Fly), and REST objects with the fields gh reads.
- kubectl: discovery built from the Kubernetes v1.37.2 OpenAPI document, and objects echoed back.

Every value is made up. The log never holds a credential: #353's server redacts the headers that
carry them, and the made-up token is built at run time and never written down.
"""
import argparse, base64, json, os, re, sys, threading, time
from urllib.parse import urlsplit, parse_qs

HERE = os.path.dirname(os.path.abspath(__file__))
BASE = os.path.normpath(os.path.join(HERE, "..", "..", "..", "2026-11-24-destructive-calls", "proto", "capture"))
sys.path.insert(0, BASE)
import server as base  # noqa: E402

TL = threading.local()
SUB = "00000000-0000-0000-0000-00000000000a"
TENANT = "00000000-0000-0000-0000-00000000000b"


def j(obj, status=200, headers=None):
    return status, dict({"Content-Type": "application/json"}, **(headers or {})), json.dumps(obj).encode()


def xml(text, status=200):
    return status, {"Content-Type": "text/xml"}, text.encode()


# ---------------------------------------------------------------- AWS
S3_LIST = """<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
<Name>b1</Name><Prefix>{prefix}</Prefix><KeyCount>2</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>
<Contents><Key>{prefix}a.csv</Key><LastModified>2026-11-24T00:00:00.000Z</LastModified><ETag>"1"</ETag><Size>1</Size><StorageClass>STANDARD</StorageClass></Contents>
<Contents><Key>{prefix}b.csv</Key><LastModified>2026-11-24T00:00:00.000Z</LastModified><ETag>"2"</ETag><Size>1</Size><StorageClass>STANDARD</StorageClass></Contents>
</ListBucketResult>"""


def aws(method, host, path, query, text, headers):
    target = base.hget(headers, "x-amz-target")
    if target:
        op = target.split(".")[-1]
        if op == "GetCallerIdentity":
            return j({"UserId": "U1", "Account": "111122223333", "Arn": "arn:aws:iam::111122223333:user/u1"}, headers={"Content-Type": "application/x-amz-json-1.1"})
        return j({}, headers={"Content-Type": "application/x-amz-json-1.1"})
    form = parse_qs(text or "") if text and "Action=" in text else {}
    action = (form.get("Action") or query.get("Action") or [None])[0]
    if action:
        if action == "GetCallerIdentity":
            return xml('<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/"><GetCallerIdentityResult>'
                       '<Arn>arn:aws:iam::111122223333:user/u1</Arn><UserId>U1</UserId><Account>111122223333</Account>'
                       '</GetCallerIdentityResult></GetCallerIdentityResponse>')
        return xml('<%sResponse><%sResult></%sResult><ResponseMetadata><RequestId>1</RequestId></ResponseMetadata></%sResponse>'
                   % (action, action, action, action))
    if ".s3" in host or host.startswith("s3."):
        if method == "GET" and "list-type" in query:
            return xml(S3_LIST.format(prefix=(query.get("prefix") or [""])[0]))
        if method == "POST" and "delete" in query:
            return xml('<DeleteResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"></DeleteResult>')
        if method == "HEAD" or method == "GET" and not query:
            return 200, {"Content-Length": "1", "ETag": '"1"', "Last-Modified": "Mon, 24 Nov 2026 00:00:00 GMT"}, b""
        if method == "DELETE":
            return 204, {}, b""
        return 200, {"Content-Type": "application/xml"}, b""
    if host.startswith("route53."):
        return xml('<ChangeResourceRecordSetsResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><ChangeInfo><Id>/change/C1</Id>'
                   '<Status>INSYNC</Status><SubmittedAt>2026-11-24T00:00:00Z</SubmittedAt></ChangeInfo></ChangeResourceRecordSetsResponse>')
    return j({})


# ---------------------------------------------------------------- Google Cloud
def gc_op(host, path, kind="compute#operation"):
    m = re.match(r"(/compute/v1/projects/[^/]+(?:/(?:zones|regions)/[^/]+|/global))", path)
    scope = m.group(1) if m else "/compute/v1/projects/proj1/global"
    return {"kind": kind, "name": "op-1", "status": "DONE", "done": True, "progress": 100, "operationType": "x",
            "selfLink": "https://%s%s/operations/op-1" % (host, scope), "targetLink": "https://%s%s" % (host, path),
            "targetId": "1", "insertTime": "2026-11-24T00:00:00Z", "endTime": "2026-11-24T00:00:01Z",
            "response": {"@type": "type.googleapis.com/google.protobuf.Empty"}}


def gcloud(method, host, path, query, text, headers):
    if host == "oauth2.googleapis.com":
        return j({"email": "dev@example.com", "scope": "https://www.googleapis.com/auth/cloud-platform", "expires_in": 3600})
    if path.endswith(":getIamPolicy") or path.endswith("/iam") and method == "GET":
        return j({"version": 1, "etag": "BwX=", "bindings": [{"role": "roles/viewer", "members": ["user:dev@example.com"]}]})
    if path.endswith(":setIamPolicy") or path.endswith("/iam"):
        return j(json.loads(text or "{}").get("policy") or json.loads(text or "{}"))
    if host == "storage.googleapis.com":
        if method == "GET" and re.search(r"/b/[^/]+/o$", path):
            return j({"kind": "storage#objects", "items": [{"kind": "storage#object", "name": "a.csv", "bucket": "b1", "generation": "1", "size": "1"},
                                                           {"kind": "storage#object", "name": "b.csv", "bucket": "b1", "generation": "1", "size": "1"}]})
        if method == "GET" and re.search(r"/b/[^/]+/o/", path):
            return j({"kind": "storage#object", "name": path.rsplit("/", 1)[-1], "bucket": "b1", "generation": "1", "size": "1", "metageneration": "1"})
        if method == "GET" and re.search(r"/b/[^/]+$", path):
            return j({"kind": "storage#bucket", "name": "b1", "id": "b1", "location": "US", "metageneration": "1", "versioning": {"enabled": True}})
        if method == "DELETE":
            return 204, {}, b""
        return j({"kind": "storage#object", "name": "hostname", "bucket": "b1", "generation": "1", "size": "1"})
    if host == "compute.googleapis.com":
        if method == "GET" and "/operations/" not in path and not re.search(r"/(instances|aggregated/\w+|disks|snapshots|firewalls|addresses)$", path):
            name = path.rstrip("/").rsplit("/", 1)[-1]
            return j({"kind": "compute#instance", "name": name, "id": "1", "status": "RUNNING", "zone": "us-central1-a",
                      "selfLink": "https://%s%s" % (host, path), "fingerprint": "YWJj", "labelFingerprint": "YWJj",
                      "metadata": {"kind": "compute#metadata", "fingerprint": "YWJj", "items": []}, "labels": {},
                      "deletionProtection": True, "targetSize": 2, "baseInstanceName": "mig1"})
        if method == "GET" and "/operations/" not in path:
            return j({"kind": "compute#list", "items": [] if "aggregated" not in path else {}, "id": "x"})
        return j(gc_op(host, re.sub(r"/operations/[^/]+(/wait)?$", "", path)))
    if host == "sqladmin.googleapis.com":
        if method == "GET" and re.search(r"/instances/[^/]+$", path):
            return j({"kind": "sql#instance", "name": "db1", "project": "proj1", "databaseVersion": "POSTGRES_15", "backendType": "SECOND_GEN",
                      "instanceType": "CLOUD_SQL_INSTANCE", "region": "us-central1", "state": "RUNNABLE",
                      "settings": {"tier": "db-custom-1-3840", "activationPolicy": "ALWAYS", "deletionProtectionEnabled": True,
                                   "ipConfiguration": {"ipv4Enabled": True, "authorizedNetworks": []},
                                   "backupConfiguration": {"enabled": True}, "settingsVersion": "1"}})
        return j({"kind": "sql#operation", "name": "op-1", "status": "DONE", "operationType": "UPDATE", "targetId": "db1",
                  "targetProject": "proj1", "selfLink": "https://%s/sql/v1beta4/projects/proj1/operations/op-1" % host})
    if host == "bigquery.googleapis.com" or host == "www.googleapis.com" and path.startswith("/bigquery"):
        if "/jobs" in path or "/queries" in path:
            return j({"kind": "bigquery#job", "jobReference": {"projectId": "proj1", "jobId": "job1", "location": "US"},
                      "status": {"state": "DONE"}, "jobComplete": True, "totalRows": "0", "schema": {"fields": []},
                      "configuration": {"query": {"query": "x"}}, "statistics": {}})
        if method == "GET":
            return j({"kind": "bigquery#table", "id": "proj1:ds.users", "tableReference": {"projectId": "proj1", "datasetId": "ds", "tableId": "users"}, "type": "TABLE"})
        return 204, {}, b""
    if host.endswith("run.googleapis.com"):
        name = path.rstrip("/").rsplit("/", 1)[-1]
        if "/namespaces/" in path:
            obj = {"apiVersion": "serving.knative.dev/v1", "kind": "Service",
                   "metadata": {"name": name, "namespace": "proj1", "resourceVersion": "1", "generation": 1, "annotations": {}},
                   "spec": {"template": {"metadata": {"annotations": {}}, "spec": {"containers": [{"image": "x"}]}}, "traffic": [{"percent": 100, "latestRevision": True}]},
                   "status": {"observedGeneration": 1, "latestReadyRevisionName": "web-1", "latestCreatedRevisionName": "web-1", "url": "https://web.example.com",
                              "conditions": [{"type": "Ready", "status": "True"}, {"type": "ConfigurationsReady", "status": "True"}, {"type": "RoutesReady", "status": "True"}]}}
            if method == "GET" and not text and name == "web" and query.get("_new"):
                return j({"code": 404}, status=404)
            return j(obj if method != "DELETE" else {"kind": "Status", "status": "Success"})
    # every other Google API: a long-running operation that is done, or a resource
    if method == "GET":
        name = path.rstrip("/").rsplit("/", 1)[-1]
        return j({"name": path.lstrip("/").split("/", 1)[-1], "done": True, "state": "ENABLED", "etag": "BwX=", "displayName": name,
                  "email": name, "uniqueId": "1", "resourceRecordSets": [], "rrsets": [{"name": "api.example.com.", "type": "A", "ttl": 300, "rrdatas": ["192.0.2.1"]}],
                  "nodePools": [{"name": "default-pool", "initialNodeCount": 3, "instanceGroupUrls": []}], "status": "RUNNING",
                  "response": {}, "primary": {"name": "x", "state": "ENABLED"}, "keyRing": "r1"})
    return j({"name": "operations/op-1", "done": True, "response": {}, "status": "DONE", "selfLink": "https://%s/v1/operations/op-1" % host,
              "kind": "dns#change", "id": "1", "additions": [], "deletions": [], "operationType": "DELETE", "targetLink": "x"})


# ---------------------------------------------------------------- Azure
def az_token(resource):
    """A made-up access token, built here at run time: three base64url parts, unsigned. Never stored."""
    def b64(d):
        return base64.urlsafe_b64encode(json.dumps(d).encode()).decode().rstrip("=")
    now = int(time.time())
    claims = {"aud": resource, "iss": "https://sts.windows.net/%s/" % TENANT, "tid": TENANT, "oid": "00000000-0000-0000-0000-000000000001",
              "appid": "00000000-0000-0000-0000-000000000003", "iat": now, "nbf": now, "exp": now + 3600, "xms_mirid": "/subscriptions/%s/rg1/mi" % SUB}
    return ".".join([b64({"alg": "none", "typ": "JWT"}), b64(claims), "made-up"])


def azure(method, host, path, query, text, headers):
    if host == "identity.capture.test":
        res = (query.get("resource") or ["https://management.azure.com/"])[0]
        return j({"access_token": az_token(res), "expires_on": str(int(time.time()) + 3600), "resource": res, "token_type": "Bearer",
                  "client_id": "00000000-0000-0000-0000-000000000003"})
    auth = base.hget(headers, "authorization")
    if host.endswith(".vault.azure.net") or host.endswith(".blob.core.windows.net"):
        res = "https://vault.azure.net" if "vault" in host else "https://storage.azure.com"
        if not auth:
            return 401, {"WWW-Authenticate": 'Bearer authorization="https://login.microsoftonline.com/%s", resource="%s"' % (TENANT, res),
                         "Content-Type": "application/json"}, b'{"error":{"code":"Unauthorized"}}'
        if "vault" in host:
            name = path.rstrip("/").rsplit("/", 1)[-1]
            if method == "DELETE" and "/deletedsecrets/" in path:
                return 204, {}, b""
            return j({"id": "https://%s/secrets/%s/1" % (host, name), "value": "x", "attributes": {"enabled": True},
                      "recoveryId": "https://%s/deletedsecrets/%s" % (host, name), "deletedDate": 1, "scheduledPurgeDate": 2})
        if method == "DELETE":
            return 202, {"x-ms-version": "2025-01-05"}, b""
        return 200, {"x-ms-version": "2025-01-05", "Content-Type": "application/xml"}, b""
    if host == "graph.microsoft.com":
        return j({"value": [], "id": "00000000-0000-0000-0000-000000000002"})
    if host == "login.microsoftonline.com":
        return j({"tenant_discovery_endpoint": "https://login.microsoftonline.com/%s/v2.0/.well-known/openid-configuration" % TENANT,
                  "api-version": "1.1", "metadata": []})
    # Azure Resource Manager
    if re.fullmatch(r"/subscriptions/?", path):
        return j({"value": [{"id": "/subscriptions/" + SUB, "subscriptionId": SUB, "tenantId": TENANT, "displayName": "test",
                             "state": "Enabled", "subscriptionPolicies": {}}]})
    if re.fullmatch(r"/tenants/?", path):
        return j({"value": [{"id": "/tenants/" + TENANT, "tenantId": TENANT}]})
    if "roleDefinitions" in path and method == "GET":
        rid = "8e3af657-a8ff-443c-a75c-2fe8c4bcb635"
        return j({"value": [{"id": "/subscriptions/%s/providers/Microsoft.Authorization/roleDefinitions/%s" % (SUB, rid), "name": rid,
                             "type": "Microsoft.Authorization/roleDefinitions", "properties": {"roleName": "Owner", "type": "BuiltInRole"}}]})
    if method == "DELETE":
        return 200, {"Content-Type": "application/json"}, b""
    name = path.rstrip("/").rsplit("/", 1)[-1]
    obj = {"id": path, "name": name, "location": "westeurope", "type": "x", "tags": {}, "sku": {"name": "Standard_LRS", "tier": "Standard", "capacity": 1},
           "kind": "StorageV2", "properties": {"provisioningState": "Succeeded", "mode": "Incremental", "count": 1, "accessPolicies": [],
                                               "tenantId": TENANT, "sku": {"family": "A", "name": "standard"}, "agentPoolProfiles": [{"name": "np1", "count": 1, "mode": "User"}],
                                               "primaryEndpoints": {"blob": "https://sa1.blob.core.windows.net/"}, "networkAcls": {"defaultAction": "Deny", "bypass": "AzureServices"},
                                               "adminUserEnabled": False, "loginServer": "acr1.azurecr.io", "state": "Running", "keys": [{"keyName": "key1", "value": "x"}],
                                               "kubeconfigs": [{"name": "clusterAdmin", "value": base64.b64encode(b"apiVersion: v1\nkind: Config\nclusters: []\n").decode()}]}}
    if method == "GET" and re.search(r"/(locks|resourceGroups|resourcegroups)$", path):
        return j({"value": [dict(obj, name="lock1", id=path + "/lock1")]})
    return j(obj)


# ---------------------------------------------------------------- GitHub
_gh_mock = []


def gh_graphql(text):
    if not _gh_mock:
        sys.path.insert(0, os.path.join(HERE, "..", "..", "..", "2026-11-25-destructive-calls-platforms", "proto", "capture"))
        import server2
        m = server2.Mock.__new__(server2.Mock)
        from graphql import build_client_schema
        m.schema = build_client_schema(json.load(open("/opt/specs/github-gql.json")), assume_valid=True)
        m.fx = {"id": "R_1", "nameWithOwner": "acme/app", "login": "acme", "name": "app", "databaseId": 1, "number": 1,
                "defaultBranchRef": None, "url": "https://github.com/acme/app", "viewerPermission": "ADMIN", "state": "OPEN",
                "headRefName": "feature", "baseRefName": "main", "mergeable": "MERGEABLE", "oid": "1" * 40, "title": "x"}
        _gh_mock.append(m)
    return j(_gh_mock[0].answer(text))


GH_REPO = {"id": 1, "node_id": "R_1", "name": "app", "full_name": "acme/app", "owner": {"login": "acme", "type": "Organization"},
           "default_branch": "main", "private": True, "visibility": "private", "archived": False, "permissions": {"admin": True, "push": True, "pull": True},
           "html_url": "https://github.com/acme/app", "clone_url": "https://github.com/acme/app.git", "parent": {"full_name": "upstream/app", "default_branch": "main", "owner": {"login": "upstream"}, "name": "app"}}


def github(method, host, path, query, text, headers):
    scopes = {"X-OAuth-Scopes": "repo, delete_repo, admin:org, workflow, admin:public_key, project, read:org, gist"}
    if path == "/graphql":
        return gh_graphql(text)
    if method == "DELETE":
        return 204, scopes, b""
    if re.fullmatch(r"/repos/[^/]+/[^/]+", path):
        return j(GH_REPO, headers=scopes)
    if path.endswith("/secrets/public-key") or path.endswith("/variables/public-key"):
        return j({"key_id": "k1", "key": base64.b64encode(b"\x09" * 32).decode()}, headers=scopes)
    if "/releases/tags/" in path:
        return j({"id": 5, "tag_name": "v1", "name": "v1", "assets": []}, headers=scopes)
    if "/actions/workflows" in path and method == "GET":
        wf = {"id": 7, "name": "ci", "path": ".github/workflows/ci.yml", "state": "active"}
        return j({"total_count": 1, "workflows": [wf]} if path.endswith("/workflows") else wf, headers=scopes)
    if "/actions/runs/" in path and method == "GET":
        return j({"id": 123, "name": "ci", "status": "completed", "workflow_id": 7, "head_branch": "main", "event": "push"}, headers=scopes)
    if path.endswith("/actions/caches") and method == "GET":
        return j({"total_count": 1, "actions_caches": [{"id": 1, "key": "npm-1", "ref": "refs/heads/main", "size_in_bytes": 1}]}, headers=scopes)
    if path in ("/user", "/user/keys") or path.startswith("/users/"):
        return j({"login": "dev", "id": 1, "type": "User"} if path != "/user/keys" else [], headers=scopes)
    if "/git/ref" in path and method == "GET":
        return j({"ref": "refs/heads/main", "object": {"sha": "1" * 40, "type": "commit"}}, headers=scopes)
    if method == "GET" and re.search(r"/(keys|labels|releases|collaborators)$", path):
        return j([], headers=scopes)
    return j({"id": 1, "number": 1, "name": "x", "sha": "1" * 40, "merged": True, "message": "ok", "ref": "refs/heads/main",
              "object": {"sha": "1" * 40}, "state": "closed", "node_id": "X_1"}, status=201 if method == "POST" else 200, headers=scopes)


# ---------------------------------------------------------------- Kubernetes
_k8s = {}
_v3 = {}


def k8s_discovery():
    if _k8s:
        return _k8s
    spec = json.load(open("/opt/specs/k8s.json"))
    groups = {}
    short = {"persistentvolumeclaims": ["pvc"], "services": ["svc"], "namespaces": ["ns"], "customresourcedefinitions": ["crd", "crds"],
             "configmaps": ["cm"], "deployments": ["deploy"], "statefulsets": ["sts"], "pods": ["po"], "nodes": ["no"],
             "networkpolicies": ["netpol"], "certificatesigningrequests": ["csr"], "serviceaccounts": ["sa"], "persistentvolumes": ["pv"],
             "daemonsets": ["ds"], "replicasets": ["rs"], "ingresses": ["ing"], "cronjobs": ["cj"], "events": ["ev"]}
    for p, item in spec["paths"].items():
        m = re.match(r"^/(api/(v1)|apis/([^/]+)/([^/]+))/(namespaces/\{namespace\}/)?([a-z0-9]+)(/\{name\})?(/([a-z]+))?$", p)
        if not m:
            continue
        gv = m.group(2) and ("", "v1") or (m.group(3), m.group(4))
        res = m.group(6)
        if res == "namespaces" and m.group(5):
            continue
        name = res + ("/" + m.group(9) if m.group(9) else "")
        verbs, kind = set(), None
        for meth, op in item.items():
            if not isinstance(op, dict) or "x-kubernetes-action" not in op:
                continue
            a = op["x-kubernetes-action"]
            verbs.add({"post": "create", "put": "update", "get": "get" if m.group(7) else "list"}.get(a, a))
            gvk = op.get("x-kubernetes-group-version-kind")
            if gvk and not kind:
                kind = gvk["kind"]
        r = groups.setdefault(gv, {}).setdefault(name, {"name": name, "singularName": "", "namespaced": bool(m.group(5)) or (res == "namespaces" and False),
                                                         "kind": kind or "Status", "verbs": set(), "shortNames": short.get(name, [])})
        r["verbs"] |= verbs
        if m.group(5):
            r["namespaced"] = True
    ev = groups.get(("", "v1"), {}).get("pods/eviction")
    if ev:
        ev.update({"group": "policy", "version": "v1", "kind": "Eviction"})
    for gv in groups:
        for r in groups[gv].values():
            r["verbs"] = sorted(r["verbs"])
            if not r["shortNames"]:
                del r["shortNames"]
    _k8s.update(groups)
    return _k8s


def k8s_openapi3():
    """The v2 document's definitions as an OpenAPI v3 document, which kubectl validates manifests with."""
    if "_v3" not in _v3:
        spec = json.load(open("/opt/specs/k8s.json"))
        txt = json.dumps(spec["definitions"]).replace("#/definitions/", "#/components/schemas/")
        _v3["_v3"] = {"openapi": "3.0.0", "info": {"title": "Kubernetes", "version": "v1.37.2"}, "paths": {},
                       "components": {"schemas": json.loads(txt)}}
    return _v3["_v3"]


def k8s(method, host, path, query, text, headers):
    d = k8s_discovery()
    if path == "/openapi/v3":
        gvs = ["api/v1"] + ["apis/%s/%s" % gv for gv in d if gv[0]]
        return j({"paths": {g: {"serverRelativeURL": "/openapi/v3/%s?hash=1" % g} for g in gvs}})
    if path.startswith("/openapi/v3/"):
        return j(k8s_openapi3())
    if path.startswith("/openapi"):
        return j({"kind": "Status", "status": "Failure", "reason": "NotFound", "code": 404}, status=404)
    if path == "/api":
        return j({"kind": "APIVersions", "versions": ["v1"], "serverAddressByClientCIDRs": [{"clientCIDR": "0.0.0.0/0", "serverAddress": "kube.capture.test:443"}]})
    if path == "/apis":
        gs = sorted({g for g, v in d if g})
        out = []
        for g in gs:
            vs = sorted(v for gg, v in d if gg == g)
            out.append({"name": g, "versions": [{"groupVersion": "%s/%s" % (g, v), "version": v} for v in vs],
                        "preferredVersion": {"groupVersion": "%s/%s" % (g, vs[-1] if "v1" not in vs else "v1"), "version": vs[-1] if "v1" not in vs else "v1"}})
        return j({"kind": "APIGroupList", "apiVersion": "v1", "groups": out})
    m = re.fullmatch(r"/api/v1|/apis/([^/]+)/([^/]+)", path)
    if m:
        gv = ("", "v1") if path == "/api/v1" else (m.group(1), m.group(2))
        if gv not in d:
            return j({"kind": "Status", "status": "Failure", "code": 404}, status=404)
        return j({"kind": "APIResourceList", "groupVersion": "v1" if not gv[0] else "%s/%s" % gv, "resources": list(d[gv].values())})
    if path == "/version":
        return j({"major": "1", "minor": "37", "gitVersion": "v1.37.2", "platform": "linux/amd64"})
    m = re.match(r"^/(?:api/v1|apis/([^/]+)/([^/]+))(?:/namespaces/([^/]+))?/([a-z0-9]+)(?:/([^/]+))?(?:/([a-z]+))?$", path)
    if not m:
        return j({"kind": "Status", "status": "Success"})
    grp, ver, ns, res, name, sub = m.groups()
    if res == "namespaces" and name and not ns:
        ns = None
    gv = (grp or "", ver or "v1")
    kind = (d.get(gv, {}).get(res) or {}).get("kind") or "Object"
    api_version = "v1" if not grp else "%s/%s" % (grp, ver)
    body = {}
    try:
        body = json.loads(text) if text and text.strip().startswith("{") else {}
    except Exception:
        body = {}

    def obj(n):
        o = {"apiVersion": api_version, "kind": kind, "metadata": {"name": n, "uid": "u-" + n, "resourceVersion": "1", "generation": 1,
                                                                    "creationTimestamp": "2026-11-24T00:00:00Z", "labels": {"app": "web"}}}
        if ns:
            o["metadata"]["namespace"] = ns
        if kind in ("Deployment", "StatefulSet", "ReplicaSet"):
            o["spec"] = {"replicas": 1, "selector": {"matchLabels": {"app": "web"}},
                         "template": {"metadata": {"labels": {"app": "web"}}, "spec": {"containers": [{"name": "web", "image": "web:1"}]}}}
            o["status"] = {"replicas": 1, "readyReplicas": 1, "observedGeneration": 1}
        if kind == "Pod":
            o["spec"] = {"nodeName": "node1", "containers": [{"name": "web", "image": "web:1"}]}
            o["metadata"]["ownerReferences"] = [{"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "web-1", "uid": "u", "controller": True}]
            o["status"] = {"phase": "Running"}
        if kind == "Service":
            o["spec"] = {"type": "ClusterIP", "ports": [{"port": 5432}], "selector": {"app": "db"}}
        if kind == "Node":
            o["spec"] = {}
        if kind == "CertificateSigningRequest":
            o["spec"] = {"request": base64.b64encode(b"x").decode(), "signerName": "kubernetes.io/kube-apiserver-client", "usages": ["client auth"]}
            o["status"] = {}
        if kind == "Scale" or sub == "scale":
            o.update({"apiVersion": "autoscaling/v1", "kind": "Scale", "spec": {"replicas": 1}, "status": {"replicas": 1}})
        return o
    if sub == "token":
        return j({"apiVersion": "authentication.k8s.io/v1", "kind": "TokenRequest", "status": {"token": "made-up-token-value", "expirationTimestamp": "2026-11-25T00:00:00Z"}}, status=201)
    if sub == "eviction":
        GONE[(res, name)] = time.time()
        return j({"kind": "Status", "status": "Success"}, status=201)
    if method == "GET" and not name:
        items = [obj("web-1"), obj("web-2")] if res == "pods" else [obj("x")] if res not in ("deployments",) else [obj("web")]
        if res == "pods" and "spec.nodeName" in " ".join(query.get("fieldSelector", [])):
            items = [obj("web-1")]
        for it in items:
            if kind in ("Pod", "Deployment") and "namespace" not in it["metadata"]:
                it["metadata"]["namespace"] = "prod"
        return j({"apiVersion": api_version, "kind": kind + "List", "metadata": {"resourceVersion": "1"}, "items": items})
    if method == "GET":
        if GONE.pop((res, name), None):   # just deleted or evicted: kubectl waits for it to go; gone once
            return j({"kind": "Status", "status": "Failure", "reason": "NotFound", "code": 404}, status=404)
        if name == "web" and res == "deployments" and query.get("_missing"):
            return j({"kind": "Status", "status": "Failure", "reason": "NotFound", "code": 404}, status=404)
        return j(obj(name))
    if method == "DELETE":
        GONE[(res, name)] = time.time()
        return j({"kind": "Status", "apiVersion": "v1", "status": "Success", "details": {"name": name, "kind": res}})
    if method == "POST":
        o = obj(body.get("metadata", {}).get("name") or name or "x")
        if isinstance(body, dict) and body:
            o.update({k: v for k, v in body.items() if k not in ("metadata",)})
            o["metadata"].update(body.get("metadata", {}))
        return j(o, status=201)
    o = obj(name)
    if sub == "scale":
        o = obj(name)
    return j(o)


GONE = {}
HANDLERS = {"aws": aws, "gcloud": gcloud, "az": azure, "gh": github, "kubectl": k8s}


class Server(base.Server):
    def read_request(self, f):
        """#353's read_request, plus "Expect: 100-continue" (aws s3 cp waits for it before the body)."""
        line = f.readline(65536)
        if not line:
            return None
        line = line.decode("latin-1").rstrip("\r\n")
        if not line:
            line = f.readline(65536).decode("latin-1").rstrip("\r\n")
            if not line:
                return None
        method, target, version = line.split(" ", 2)
        headers = []
        while True:
            h = f.readline(65536).decode("latin-1")
            if h in ("\r\n", "\n", ""):
                break
            k, _, v = h.partition(":")
            headers.append((k.strip(), v.strip()))
        if (base.hget(headers, "expect") or "").lower() == "100-continue":
            f.raw._sock.sendall(b"HTTP/1.1 100 Continue\r\n\r\n")
        body = b""
        te = (base.hget(headers, "transfer-encoding") or "").lower()
        if "chunked" in te:
            while True:
                size = int(f.readline().strip().split(b";")[0], 16)
                if size == 0:
                    while f.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    break
                body += f.read(size)
                f.readline()
        else:
            n = int(base.hget(headers, "content-length") or 0)
            if n:
                body = f.read(n)
        TL.headers = headers
        return method, target, version, headers, body, te

    def canned(self, host, method, target, ops, text):
        u = urlsplit(target)
        try:
            return HANDLERS[self.args.cli](method, host, u.path, parse_qs(u.query), text, getattr(TL, "headers", []))
        except Exception as e:   # an answer the CLI can read is better than a dropped connection
            return j({"error": "mock: %r" % e}, status=500)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=443)
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--cert", default="/opt/clis/pki/chain.crt")
    ap.add_argument("--key", default="/opt/clis/pki/leaf.key")
    ap.add_argument("--log", required=True)
    ap.add_argument("--cli", required=True)
    ap.add_argument("--canned")
    ap.add_argument("--hold")
    ap.add_argument("--hold-secs", type=float, default=5)
    ap.add_argument("--hold-style", default="json403")
    ap.add_argument("--alpn", default="http/1.1")
    ap.add_argument("--git-root", default="/opt/clis/git")
    Server(ap.parse_args()).serve()


if __name__ == "__main__":
    main()
