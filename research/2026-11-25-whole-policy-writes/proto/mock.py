"""Providers that keep their access documents: Google IAM policies with etags, AWS IAM, S3 and KMS
policies, Azure Key Vault access policies, role definitions and assignments, and Kubernetes bindings
and the aws-auth ConfigMap. Every other request is answered by #404's mock (server3.py), which gets
each CLI to its step. Every value is made up.

handle(req) -> {status, headers, body}. State is reset per scenario with reset(world).
"""
import base64
import copy
import json
import os
import re
import sys
import threading
import uuid
from urllib.parse import parse_qs, quote, urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
P404 = os.path.normpath(os.path.join(HERE, "..", "..", "2026-11-25-destructive-calls-clouds", "proto", "capture"))
sys.path.insert(0, P404)
import server3  # noqa: E402

LOCK = threading.Lock()
STATE = {}
DENY_READ = set()
PROJECT_ALIASES = {"123456789012": "proj1"}
SUB = server3.SUB


def reset(world):
    with LOCK:
        STATE.clear()
        STATE.update(copy.deepcopy(world))
        DENY_READ.clear()


def j(obj, status=200, ctype="application/json"):
    return {"status": status, "headers": [["Content-Type", ctype]], "body": json.dumps(obj).encode()}


def xml(action, inner, status=200):
    body = ('<%sResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/"><%sResult>%s</%sResult>'
            '<ResponseMetadata><RequestId>1</RequestId></ResponseMetadata></%sResponse>' % (action, action, inner, action, action))
    return {"status": status, "headers": [["Content-Type", "text/xml"]], "body": body.encode()}


def next_etag(old):
    raw = base64.b64decode(old + "=" * (-len(old) % 4)) if old else b"\0" * 6
    n = int.from_bytes(raw, "big") + 1
    return base64.b64encode(n.to_bytes(max(len(raw), 6), "big")).decode()


# ---------------------------------------------------------------- Google
def gcp_key(host, res):
    m = re.match(r"projects/(\d+)(.*)", res)
    if m and m.group(1) in PROJECT_ALIASES:
        res = "projects/" + PROJECT_ALIASES[m.group(1)] + m.group(2)
    return "gcp|%s|%s" % (host, res)


def gcp_conflict():
    return j({"error": {"code": 409, "status": "ABORTED", "message": "There were concurrent policy changes. Please retry the whole read-modify-write with exponential backoff."}}, 409)


def gcp_set(key, body, storage=False):
    cur = STATE.get(key)
    if cur is None:
        return j({"error": {"code": 404, "status": "NOT_FOUND", "message": "not found"}}, 404)
    pol = body if storage else (body.get("policy") or {})
    if pol.get("etag"):
        if base64.b64decode(pol["etag"] + "==") != base64.b64decode(cur["etag"] + "=="):
            return gcp_conflict()
    mask = {p.strip() for p in (body.get("updateMask") or "bindings,etag").split(",")} if not storage else {"bindings"}
    new = copy.deepcopy(cur)
    new["bindings"] = pol.get("bindings") or []
    if "auditConfigs" in mask:
        new["auditConfigs"] = pol.get("auditConfigs") or []
    new["version"] = pol.get("version", cur.get("version", 1))
    new["etag"] = next_etag(cur["etag"])
    STATE[key] = new
    out = copy.deepcopy(new)
    if storage:
        out.update({"kind": "storage#policy", "resourceId": "projects/_/" + key.split("|")[-1]})
    return j(out)


def google(m, host, path, q, body):
    rv = re.match(r"^/v\d\w*/(.*):(getIamPolicy|setIamPolicy)$", path)
    if rv:
        key = gcp_key(host, rv.group(1))
        if key not in STATE:
            return None
        if rv.group(2) == "getIamPolicy":
            if key in DENY_READ:
                return j({"error": {"code": 403, "status": "PERMISSION_DENIED", "message": "Permission 'getIamPolicy' denied"}}, 403)
            return j(STATE[key])
        return gcp_set(key, json.loads(body or b"{}"))
    rv = re.fullmatch(r"/storage/v1/(b/[^/]+)/iam", path)
    if rv:
        key = gcp_key(host, rv.group(1))
        if m == "GET":
            out = copy.deepcopy(STATE[key])
            out.update({"kind": "storage#policy", "resourceId": "projects/_/" + rv.group(1)})
            return j(out)
        return gcp_set(key, json.loads(body or b"{}"), storage=True)
    rv = re.fullmatch(r"/storage/v1/b/([^/]+)", path)
    if rv and m == "GET":
        return j({"kind": "storage#bucket", "name": rv.group(1), "id": rv.group(1), "location": "US", "metageneration": "1",
                  "iamConfiguration": {"uniformBucketLevelAccess": {"enabled": True}}, "projectNumber": "123456789012"})
    return None


# ---------------------------------------------------------------- AWS
def aws_iam(f):
    a = f.get("Action")
    if a in ("GetRolePolicy", "PutRolePolicy"):
        key = "aws|inline|role|%s|%s" % (f.get("RoleName"), f.get("PolicyName"))
        if a == "PutRolePolicy":
            STATE[key] = f["PolicyDocument"]
            return xml(a, "")
        if key not in STATE:
            return None
        return xml(a, "<RoleName>%s</RoleName><PolicyName>%s</PolicyName><PolicyDocument>%s</PolicyDocument>" % (f["RoleName"], f["PolicyName"], quote(STATE[key])))
    if a in ("GetRole", "UpdateAssumeRolePolicy"):
        key = "aws|trust|%s" % f.get("RoleName")
        if key not in STATE:
            return None
        if a == "UpdateAssumeRolePolicy":
            STATE[key] = f["PolicyDocument"]
            return xml(a, "")
        r = f["RoleName"]
        return xml(a, "<Role><Path>/</Path><RoleName>%s</RoleName><RoleId>AROA1</RoleId><Arn>arn:aws:iam::111122223333:role/%s</Arn>"
                      "<CreateDate>2026-01-01T00:00:00Z</CreateDate><AssumeRolePolicyDocument>%s</AssumeRolePolicyDocument></Role>" % (r, r, quote(STATE[key])))
    if a in ("GetPolicy", "GetPolicyVersion", "CreatePolicyVersion"):
        arn = f.get("PolicyArn")
        key = "aws|managed|%s" % arn
        if key not in STATE:
            return None
        st = STATE[key]
        if a == "GetPolicy":
            return xml(a, "<Policy><PolicyName>%s</PolicyName><PolicyId>ANPA1</PolicyId><Arn>%s</Arn><Path>/</Path><DefaultVersionId>%s</DefaultVersionId>"
                          "<AttachmentCount>1</AttachmentCount><IsAttachable>true</IsAttachable><CreateDate>2026-01-01T00:00:00Z</CreateDate></Policy>"
                          % (arn.rsplit("/", 1)[-1], arn, st["default"]))
        if a == "GetPolicyVersion":
            v = f.get("VersionId")
            return xml(a, "<PolicyVersion><Document>%s</Document><VersionId>%s</VersionId><IsDefaultVersion>%s</IsDefaultVersion>"
                          "<CreateDate>2026-01-01T00:00:00Z</CreateDate></PolicyVersion>" % (quote(st["versions"][v]), v, "true" if v == st["default"] else "false"))
        v = "v%d" % (len(st["versions"]) + 1)
        st["versions"][v] = f["PolicyDocument"]
        if (f.get("SetAsDefault") or "").lower() == "true":
            st["default"] = v
        return xml(a, "<PolicyVersion><VersionId>%s</VersionId><IsDefaultVersion>%s</IsDefaultVersion><CreateDate>2026-01-01T00:00:00Z</CreateDate></PolicyVersion>"
                      % (v, "true" if st["default"] == v else "false"))
    return None


def aws(m, host, path, q, body, headers):
    if host == "iam.amazonaws.com" and m == "POST":
        return aws_iam({k: v[0] for k, v in parse_qs(body.decode(), keep_blank_values=True).items()})
    hm = re.match(r"^([^.]+)\.s3[.-]", host)
    if hm and "policy" in q:
        key = "aws|s3|%s" % hm.group(1)
        if m == "GET":
            return {"status": 200, "headers": [["Content-Type", "application/json"]], "body": STATE[key].encode()}
        if m == "PUT":
            STATE[key] = body.decode()
            return {"status": 204, "headers": [], "body": b""}
    t = server3.base.hget(headers, "x-amz-target") or ""
    if t in ("TrentService.GetKeyPolicy", "TrentService.PutKeyPolicy"):
        b = json.loads(body)
        key = "aws|kms|%s" % b["KeyId"]
        if t.endswith("GetKeyPolicy"):
            return j({"Policy": STATE[key], "PolicyName": "default"}, ctype="application/x-amz-json-1.1")
        STATE[key] = b["Policy"]
        return j({}, ctype="application/x-amz-json-1.1")
    return None


# ---------------------------------------------------------------- Azure
ROLE_NAMES = {"Owner": "8e3af657-a8ff-443c-a75c-2fe8c4bcb635", "Reader": "acdd72a7-3385-48ef-bd42-f606fba81ae7",
              "Contributor": "b24988ac-6180-42a0-ab88-20f7382dd24c"}


def azure(m, host, path, q, body):
    if host != "management.azure.com":
        return None
    lp = path.lower()
    vm = re.match(r"^(/subscriptions/[^/]+/resourcegroups/[^/]+/providers/microsoft\.keyvault/vaults/([^/]+))(/accesspolicies/(add|replace|remove))?$", lp)
    if vm:
        key = "az|vault|" + vm.group(1)
        if key not in STATE:
            return None
        if m == "GET":
            return j(STATE[key])
        b = json.loads(body or b"{}")
        cur = STATE[key]
        if vm.group(4):
            listed = (b.get("properties") or {}).get("accessPolicies") or []
            aps = cur["properties"]["accessPolicies"]
            if vm.group(4) == "replace":
                cur["properties"]["accessPolicies"] = listed
            elif vm.group(4) == "add":
                cur["properties"]["accessPolicies"] = aps + listed
            else:
                gone = {p["objectId"].lower() for p in listed}
                cur["properties"]["accessPolicies"] = [p for p in aps if p["objectId"].lower() not in gone]
            return j({"id": vm.group(1) + "/accessPolicies/" + vm.group(4), "properties": {"accessPolicies": cur["properties"]["accessPolicies"]}})
        if m == "PATCH":
            cur["properties"].update(b.get("properties") or {})
        else:
            new = copy.deepcopy(cur)
            new["properties"] = b.get("properties") or {}
            new["tags"] = b.get("tags") or {}
            STATE[key] = cur = new
        return j(cur)
    rd = re.match(r"^(/.*/providers/microsoft\.authorization/roledefinitions)(/([^/]+))?$", lp)
    if rd:
        defs = {k: v for k, v in STATE.items() if k.startswith("az|roledef|")}
        if rd.group(3):
            key = "az|roledef|" + rd.group(3)
            if m == "GET":
                return j(STATE[key]) if key in STATE else j({"error": {"code": "RoleDefinitionDoesNotExist"}}, 404)
            b = json.loads(body or b"{}")
            new = copy.deepcopy(STATE.get(key) or {"id": path, "name": rd.group(3), "type": "Microsoft.Authorization/roleDefinitions"})
            new["properties"] = b.get("properties") or {}
            STATE[key] = new
            return j(new, 201)
        flt = (q.get("$filter") or [""])[0]
        fm = re.search(r"roleName eq '([^']+)'", flt)
        out = []
        for name, guid in ROLE_NAMES.items():
            out.append({"id": "/subscriptions/%s/providers/Microsoft.Authorization/roleDefinitions/%s" % (SUB, guid), "name": guid,
                        "type": "Microsoft.Authorization/roleDefinitions",
                        "properties": {"roleName": name, "type": "BuiltInRole", "permissions": [{"actions": ["*"]}], "assignableScopes": ["/"]}})
        out += list(defs.values())
        if fm:
            out = [d for d in out if d["properties"].get("roleName", "").lower() == fm.group(1).lower()]
        return j({"value": out})
    ra = re.match(r"^/.*/providers/microsoft\.authorization/roleassignments(/([^/]+))?$", lp)
    if ra:
        if m == "PUT":
            key = "az|assign|" + ra.group(2)
            if key in STATE:
                return j({"error": {"code": "RoleAssignmentExists"}}, 409)
            b = json.loads(body or b"{}")
            STATE[key] = {"id": path, "name": ra.group(2), "type": "Microsoft.Authorization/roleAssignments", "properties": dict(b.get("properties") or {}, scope="/subscriptions/" + SUB)}
            return j(STATE[key], 201)
        if m == "GET" and not ra.group(1):
            return j({"value": [v for k, v in STATE.items() if k.startswith("az|assign|")]})
    return None


# ---------------------------------------------------------------- Kubernetes
K8S_PATH = re.compile(r"^/(?:api/v1|apis/rbac\.authorization\.k8s\.io/v1)(?:/namespaces/([^/]+))?/(clusterrolebindings|rolebindings|configmaps)(?:/([^/]+))?$")


def json_patch(doc, ops):
    doc = copy.deepcopy(doc)
    for op in ops:
        parts = [p.replace("~1", "/").replace("~0", "~") for p in op["path"].split("/")[1:]]
        tgt = doc
        for p in parts[:-1]:
            tgt = tgt[int(p)] if isinstance(tgt, list) else tgt[p]
        last = parts[-1]
        if op["op"] in ("add", "replace"):
            if isinstance(tgt, list):
                if last == "-":
                    tgt.append(op["value"])
                elif op["op"] == "add":
                    tgt.insert(int(last), op["value"])
                else:
                    tgt[int(last)] = op["value"]
            else:
                tgt[last] = op["value"]
        elif op["op"] == "remove":
            if isinstance(tgt, list):
                del tgt[int(last)]
            else:
                del tgt[last]
    return doc


def k8s(m, host, path, q, body, headers):
    if host != "kube.capture.test":
        return None
    km = K8S_PATH.match(path)
    if not km:
        return None
    ns, res, name = km.groups()
    ct = (server3.base.hget(headers, "content-type") or "").lower()
    if m == "POST" and not name:
        if "json" not in ct:
            return None
        b = json.loads(body)
        name = b["metadata"]["name"]
        key = "k8s|%s/%s" % (path, name)
        b["metadata"].update({"uid": "u-" + name, "resourceVersion": "1", "creationTimestamp": "2026-11-24T00:00:00Z"})
        STATE[key] = b
        return j(b, 201)
    key = "k8s|" + path
    if key not in STATE:
        return None if res != "configmaps" else j({"kind": "Status", "status": "Failure", "reason": "NotFound", "code": 404}, 404)
    cur = STATE[key]
    if m == "GET":
        return j(cur)
    if m == "PUT":
        if "json" not in ct:
            return {"status": 415, "headers": [["Content-Type", "application/json"]], "body": b'{"kind":"Status","message":"mock reads JSON only"}'}
        b = json.loads(body)
        b["metadata"]["resourceVersion"] = str(int(cur["metadata"].get("resourceVersion", "1")) + 1)
        STATE[key] = b
        return j(b)
    if m == "PATCH":
        p = json.loads(body)
        if "json-patch" in ct:
            new = json_patch(cur, p)
        else:
            sys.path.insert(0, HERE)
            import docs
            new = docs.merge_patch(cur, p)
        new["metadata"]["resourceVersion"] = str(int(cur["metadata"].get("resourceVersion", "1")) + 1)
        STATE[key] = new
        return j(new)
    return None


# ---------------------------------------------------------------- dispatch
def handle(req):
    u = urlsplit(req["target"])
    path, q = u.path, parse_qs(u.query, keep_blank_values=True)
    host, m, body = req["host"], req["method"], req["body"]
    with LOCK:
        try:
            out = (google(m, host, path, q, body) if host.endswith("googleapis.com") else
                   aws(m, host, path, q, body, req["headers"]) if host.endswith("amazonaws.com") else
                   azure(m, host, path, q, body) if host.endswith("azure.com") else
                   k8s(m, host, path, q, body, req["headers"]))
        except Exception as e:
            out = j({"error": "mock: %r" % e}, 500)
    if out is not None:
        return out
    fam = ("gcloud" if host.endswith("googleapis.com") else "aws" if host.endswith("amazonaws.com") else
           "az" if host.endswith(("azure.com", "azure.net", "windows.net", "capture.test")) and host != "kube.capture.test" else "kubectl")
    server3.TL.headers = req["headers"]
    try:
        st, hd, bd = server3.HANDLERS[fam](m, host, path, q, body.decode("utf-8", "replace"), req["headers"])
    except Exception as e:
        st, hd, bd = 500, {"Content-Type": "application/json"}, json.dumps({"error": "mock: %r" % e}).encode()
    return {"status": st, "headers": [[k, v] for k, v in hd.items()], "body": bd}


def new_guid():
    return str(uuid.uuid4())
