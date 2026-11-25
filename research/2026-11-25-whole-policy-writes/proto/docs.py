"""Which requests write a whole access document, which read one, and the read Silo would send itself.

A request is {method, host, target, headers: [[name, value]], body: bytes}; a response is
{status, headers, body}. Everything here reads what the boundary sees: nothing is asked of the agent.

- write_of(req) -> {kind, key, request_doc, etag, mask, incremental, readable} or None
- read_of(req, resp) -> (key, doc, etag) or None, for a read the boundary relays
- own_read(write, req) -> list of requests to send, and parse(responses) -> (doc, etag) or None
"""
import json
import re
from urllib.parse import parse_qs, unquote, urlsplit
from xml.etree import ElementTree as ET


def hget(headers, name):
    for k, v in headers:
        if k.lower() == name:
            return v
    return None


def split(req):
    u = urlsplit(req["target"])
    return u.path, parse_qs(u.query)


def body_json(req):
    try:
        return json.loads(req["body"] or b"{}")
    except (ValueError, UnicodeDecodeError):
        return None


def form(req):
    try:
        return {k: v[0] for k, v in parse_qs(req["body"].decode(), keep_blank_values=True).items()}
    except UnicodeDecodeError:
        return {}


def xml_text(body, tag):
    try:
        root = ET.fromstring(body)
    except ET.ParseError:
        return None
    for el in root.iter():
        if el.tag.split("}")[-1] == tag:
            return el.text
    return None


# ---------------------------------------------------------------- Google
GCP_VER = re.compile(r"^/(?:v\d\w*)/")


def gcp_resource(path):
    return GCP_VER.sub("", path).split(":")[0]


def _gcp_write(req, path, q):
    host = req["host"]
    if host.endswith("googleapis.com") and req["method"] == "POST" and path.endswith(":setIamPolicy"):
        b = body_json(req)
        if b is None:
            return {"kind": "gcp-iam", "key": ("gcp", host, gcp_resource(path)), "readable": False}
        p = b.get("policy") or {}
        return {"kind": "gcp-iam", "key": ("gcp", host, gcp_resource(path)), "request_doc": p, "etag": p.get("etag"),
                "mask": b.get("updateMask"), "readable": True}
    m = re.fullmatch(r"/storage/v1/(b/[^/]+)/iam", path)
    if host == "storage.googleapis.com" and m and req["method"] == "PUT":
        b = body_json(req)
        if b is None:
            return {"kind": "gcp-iam", "key": ("gcp", host, m.group(1)), "readable": False}
        return {"kind": "gcp-iam", "key": ("gcp", host, m.group(1)), "request_doc": b, "etag": b.get("etag"),
                "mask": "bindings,etag", "readable": True}
    return None


def _gcp_read(req, resp, path):
    host = req["host"]
    if host.endswith("googleapis.com") and path.endswith(":getIamPolicy"):
        key = ("gcp", host, gcp_resource(path))
    elif host == "storage.googleapis.com" and re.fullmatch(r"/storage/v1/b/[^/]+/iam", path) and req["method"] == "GET":
        key = ("gcp", host, path[len("/storage/v1/"):-len("/iam")])
    elif host.endswith("googleapis.com") and path.endswith(":setIamPolicy") and resp["status"] == 200:
        key = ("gcp", host, gcp_resource(path))  # the answer to a write is the policy now held
    else:
        return None
    try:
        doc = json.loads(resp["body"])
    except ValueError:
        return None
    return key, doc, doc.get("etag")


def _gcp_own(w, req):
    host, res = w["key"][1], w["key"][2]
    auth = [["Authorization", hget(req["headers"], "authorization") or ""]]
    if host == "storage.googleapis.com":
        return [{"method": "GET", "host": host, "target": "/storage/v1/%s/iam?optionsRequestedPolicyVersion=3" % res, "headers": auth, "body": b""}]
    ver = GCP_VER.match(split(req)[0]).group(0)
    if host == "cloudresourcemanager.googleapis.com":
        return [{"method": "POST", "host": host, "target": "%s%s:getIamPolicy" % (ver, res),
                 "headers": auth + [["Content-Type", "application/json"]], "body": b'{"options":{"requestedPolicyVersion":3}}'}]
    return [{"method": "GET", "host": host, "target": "%s%s:getIamPolicy?options.requestedPolicyVersion=3" % (ver, res), "headers": auth, "body": b""}]


# ---------------------------------------------------------------- AWS
def _aws_query_write(req):
    if req["host"] != "iam.amazonaws.com" or req["method"] != "POST":
        return None
    f = form(req)
    a = f.get("Action")
    if a in ("PutRolePolicy", "PutUserPolicy", "PutGroupPolicy"):
        who = {"PutRolePolicy": "RoleName", "PutUserPolicy": "UserName", "PutGroupPolicy": "GroupName"}[a]
        key = ("aws", "inline", a[3:-6].lower(), f.get(who), f.get("PolicyName"))
        return {"kind": "aws-identity", "key": key, "request_doc": json.loads(f["PolicyDocument"]), "readable": True}
    if a == "UpdateAssumeRolePolicy":
        return {"kind": "aws-trust", "key": ("aws", "trust", f.get("RoleName")), "request_doc": json.loads(f["PolicyDocument"]), "readable": True}
    if a == "CreatePolicyVersion":
        if (f.get("SetAsDefault") or "false").lower() != "true":
            return None  # not in force until SetDefaultPolicyVersion, which is a write of its own
        return {"kind": "aws-identity", "key": ("aws", "managed", f.get("PolicyArn")), "request_doc": json.loads(f["PolicyDocument"]), "readable": True}
    return None


def _s3_bucket(req, path):
    h = req["host"]
    m = re.match(r"^([^.]+)\.s3[.-]", h)
    if m:
        return m.group(1)
    seg = path.strip("/").split("/")[0]
    return seg or None


def _aws_write(req, path, q):
    w = _aws_query_write(req)
    if w:
        return w
    h = req["host"]
    if (".s3." in h or h.startswith("s3.")) and req["method"] == "PUT" and "policy" in q:
        return {"kind": "aws-resource", "key": ("aws", "s3", _s3_bucket(req, path)), "request_doc": json.loads(req["body"]), "readable": True}
    t = hget(req["headers"], "x-amz-target") or ""
    if t == "TrentService.PutKeyPolicy":
        b = body_json(req)
        return {"kind": "aws-resource", "key": ("aws", "kms", b.get("KeyId")), "request_doc": json.loads(b["Policy"]), "readable": True}
    return None


def _aws_read(req, resp, path, q):
    if resp["status"] != 200:
        return None
    h = req["host"]
    if h == "iam.amazonaws.com":
        f = form(req)
        a = f.get("Action")
        if a in ("GetRolePolicy", "GetUserPolicy", "GetGroupPolicy"):
            who = {"GetRolePolicy": "RoleName", "GetUserPolicy": "UserName", "GetGroupPolicy": "GroupName"}[a]
            d = xml_text(resp["body"], "PolicyDocument")
            return (("aws", "inline", a[3:-6].lower(), f.get(who), f.get("PolicyName")), json.loads(unquote(d)), None) if d else None
        if a == "GetRole":
            d = xml_text(resp["body"], "AssumeRolePolicyDocument")
            return (("aws", "trust", f.get("RoleName")), json.loads(unquote(d)), None) if d else None
        if a == "GetPolicyVersion":
            d = xml_text(resp["body"], "Document")
            dflt = xml_text(resp["body"], "IsDefaultVersion")
            if d and dflt == "true":
                return ("aws", "managed", f.get("PolicyArn")), json.loads(unquote(d)), None
        return None
    if (".s3." in h or h.startswith("s3.")) and req["method"] == "GET" and "policy" in q:
        return ("aws", "s3", _s3_bucket(req, path)), json.loads(resp["body"]), None
    if (hget(req["headers"], "x-amz-target") or "") == "TrentService.GetKeyPolicy":
        b = body_json(req)
        return ("aws", "kms", b.get("KeyId")), json.loads(json.loads(resp["body"])["Policy"]), None
    return None


def _aws_own(w, req):
    """Silo's own read. It must be signed: Silo can send it only when it holds the key
    (signing-keys-re-signed). The prototype's mock does not check signatures."""
    k = w["key"]
    hdr = [["Content-Type", "application/x-www-form-urlencoded; charset=utf-8"], ["Authorization", "AWS4-HMAC-SHA256 (signed by Silo)"]]
    if k[1] == "inline":
        who = {"role": "RoleName", "user": "UserName", "group": "GroupName"}[k[2]]
        body = "Action=Get%sPolicy&Version=2010-05-08&%s=%s&PolicyName=%s" % (k[2].capitalize(), who, k[3], k[4])
        return [{"method": "POST", "host": "iam.amazonaws.com", "target": "/", "headers": hdr, "body": body.encode()}]
    if k[1] == "trust":
        return [{"method": "POST", "host": "iam.amazonaws.com", "target": "/", "headers": hdr, "body": ("Action=GetRole&Version=2010-05-08&RoleName=%s" % k[2]).encode()}]
    if k[1] == "managed":
        return [{"method": "POST", "host": "iam.amazonaws.com", "target": "/", "headers": hdr, "body": ("Action=GetPolicy&Version=2010-05-08&PolicyArn=%s" % k[2]).encode()},
                {"then": "GetPolicyVersion", "arn": k[2]}]
    if k[1] == "s3":
        return [{"method": "GET", "host": "%s.s3.amazonaws.com" % k[2], "target": "/?policy", "headers": [["Authorization", "AWS4-HMAC-SHA256 (signed by Silo)"]], "body": b""}]
    if k[1] == "kms":
        return [{"method": "POST", "host": "kms.us-east-1.amazonaws.com", "target": "/",
                 "headers": [["X-Amz-Target", "TrentService.GetKeyPolicy"], ["Content-Type", "application/x-amz-json-1.1"], ["Authorization", "AWS4-HMAC-SHA256 (signed by Silo)"]],
                 "body": json.dumps({"KeyId": k[2], "PolicyName": "default"}).encode()}]
    return []


# ---------------------------------------------------------------- Azure
AZ_VAULT = re.compile(r"(?i)^(/subscriptions/[^/]+/resourcegroups/[^/]+/providers/microsoft\.keyvault/vaults/[^/]+)(/accesspolicies/(add|replace|remove))?$")
AZ_ROLEDEF = re.compile(r"(?i)^/.*/providers/microsoft\.authorization/roledefinitions/[^/]+$")
AZ_ASSIGN = re.compile(r"(?i)^/.*/providers/microsoft\.authorization/roleassignments/[^/]+$")


def _az_write(req, path, q):
    if req["host"] != "management.azure.com" or req["method"] not in ("PUT", "PATCH"):
        return None
    m = AZ_VAULT.match(path)
    if m:
        b = body_json(req)
        key = ("az", "vault", m.group(1).lower())
        if b is None:
            return {"kind": "az-vault", "key": key, "readable": False}
        if m.group(3):
            return {"kind": "az-vault", "key": key, "request_doc": b, "op": m.group(3).lower(), "readable": True}
        if req["method"] == "PATCH":
            return {"kind": "az-vault", "key": key, "request_doc": b, "op": "patch", "readable": True}
        return {"kind": "az-vault", "key": key, "request_doc": b, "readable": True}
    if AZ_ROLEDEF.match(path):
        b = body_json(req)
        return {"kind": "az-roledef", "key": ("az", "roledef", path.lower()), "request_doc": b, "readable": b is not None}
    if AZ_ASSIGN.match(path) and req["method"] == "PUT":
        b = body_json(req)
        # A role assignment is created, never replaced: the provider answers 409 if it exists.
        return {"kind": "az-assignment", "key": ("az", "assignment", path.lower()), "request_doc": b, "incremental": True, "readable": b is not None}
    return None


def _az_read(req, resp, path):
    if req["host"] != "management.azure.com" or resp["status"] != 200:
        return None
    if req["method"] == "GET" and AZ_VAULT.match(path) and not AZ_VAULT.match(path).group(2):
        return ("az", "vault", path.lower()), json.loads(resp["body"]), None
    if req["method"] in ("PUT", "PATCH") and AZ_VAULT.match(path) and not AZ_VAULT.match(path).group(2):
        return ("az", "vault", path.lower()), json.loads(resp["body"]), None
    if req["method"] in ("GET", "PUT") and AZ_ROLEDEF.match(path):
        return ("az", "roledef", path.lower()), json.loads(resp["body"]), None
    return None


def _az_own(w, req):
    path = w["key"][2]
    api = {"vault": "2026-05-15", "roledef": "2022-04-01"}[w["key"][1]]
    return [{"method": "GET", "host": "management.azure.com", "target": "%s?api-version=%s" % (path, api),
             "headers": [["Authorization", hget(req["headers"], "authorization") or ""]], "body": b""}]


def az_vault_after(w, before):
    """The vault after an access-policy add, remove or replace, or after a whole PUT or a PATCH."""
    req = w["request_doc"]
    op = w.get("op")
    if op is None:
        return req
    base = json.loads(json.dumps(before or {"properties": {"accessPolicies": []}}))
    props = base.setdefault("properties", {})
    if op == "patch":
        props.update((req.get("properties") or {}))
        return base
    listed = (req.get("properties") or {}).get("accessPolicies") or []
    cur = props.get("accessPolicies") or []
    if op == "replace":
        props["accessPolicies"] = listed
    elif op == "add":
        props["accessPolicies"] = cur + listed
    else:
        gone = {(p.get("objectId") or "").lower() for p in listed}
        props["accessPolicies"] = [p for p in cur if (p.get("objectId") or "").lower() not in gone]
    return base


# ---------------------------------------------------------------- Kubernetes
K8S = re.compile(r"^/apis/rbac\.authorization\.k8s\.io/v1/(?:namespaces/[^/]+/)?(rolebindings|clusterrolebindings)/[^/]+$|^/api/v1/namespaces/kube-system/configmaps/aws-auth$")


def k8s_kind(path):
    return "k8s-aws-auth" if path.endswith("/aws-auth") else "k8s-binding"


def _k8s_write(req, path, q):
    if req["host"] != "kube.capture.test":
        return None
    ct = (hget(req["headers"], "content-type") or "").lower()
    if req["method"] in ("PUT", "PATCH") and K8S.match(path):
        b = body_json(req) if "json" in ct else None
        return {"kind": k8s_kind(path), "key": ("k8s", path), "request_doc": b, "patch": req["method"] == "PATCH",
                "jsonpatch": "json-patch" in ct, "readable": b is not None, "ctype": ct}
    if req["method"] == "POST":
        m = re.match(r"^(.*/(?:rolebindings|clusterrolebindings|configmaps))$", path)
        if m:
            b = body_json(req) if "json" in ct else None
            name = ((b or {}).get("metadata") or {}).get("name")
            full = "%s/%s" % (path, name)
            if b is None or K8S.match(full):
                return {"kind": k8s_kind(full), "key": ("k8s", full), "request_doc": b, "incremental": True, "readable": b is not None, "ctype": ct}
    return None


def _k8s_read(req, resp, path):
    if req["host"] != "kube.capture.test" or resp["status"] != 200 or not K8S.match(path) or req["method"] not in ("GET", "PUT", "PATCH"):
        return None
    try:
        return ("k8s", path), json.loads(resp["body"]), None
    except ValueError:
        return None


def _k8s_own(w, req):
    return [{"method": "GET", "host": "kube.capture.test", "target": w["key"][1], "headers": [["Authorization", hget(req["headers"], "authorization") or ""], ["Accept", "application/json"]], "body": b""}]


def merge_patch(before, patch):
    """A strategic merge patch on these objects: maps merge, lists are replaced (RBAC subjects have no
    merge key), and null deletes."""
    if not isinstance(before, dict) or not isinstance(patch, dict):
        return patch
    out = dict(before)
    for k, v in patch.items():
        if k.startswith("$"):
            continue
        if v is None:
            out.pop(k, None)
        else:
            out[k] = merge_patch(before.get(k), v) if isinstance(v, dict) else v
    return out


# ---------------------------------------------------------------- one interface
def write_of(req):
    path, q = split(req)
    for f in (_gcp_write, _aws_write, _az_write, _k8s_write):
        w = f(req, path, q)
        if w:
            return w
    return None


def read_of(req, resp):
    path, q = split(req)
    try:
        return (_gcp_read(req, resp, path) or _aws_read(req, resp, path, q) or _az_read(req, resp, path) or _k8s_read(req, resp, path))
    except (ValueError, KeyError, TypeError):
        return None


def own_read(w, req):
    return {"gcp": _gcp_own, "aws": _aws_own, "az": _az_own, "k8s": _k8s_own}[w["key"][0]](w, req)


def needs_signing(w):
    return w["key"][0] == "aws"


def after_doc(w, before):
    """The document the provider will hold if the write goes through, given the one before (or None)."""
    from grants import gcp_after
    k = w["kind"]
    if k == "gcp-iam":
        return gcp_after(w["request_doc"], w.get("mask"), before)
    if k == "az-vault":
        return az_vault_after(w, before)
    if k.startswith("k8s-") and w.get("jsonpatch"):
        import mock  # the same JSON Patch the provider applies
        return mock.json_patch(before, w["request_doc"]) if before is not None else None
    if k.startswith("k8s-") and w.get("patch"):
        return merge_patch(before, w["request_doc"]) if before is not None else w["request_doc"]
    return w["request_doc"]
