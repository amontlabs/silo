"""Turns every labelled operation (and each variant of an operation labelled cond) into one request,
so a rule set can be scored on the whole documented surface. Same idea as #383's surface.py.

AWS requests are made by botocore itself (awscli's bundled copy), signed with made-up keys, and
caught before they are sent, so each has the wire form the CLI would send: query, JSON with
X-Amz-Target, REST-XML (S3 path-style), REST-JSON. The others are built from the path template.
"""
import json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import variants as VAR  # noqa: E402

PROVIDERS = ["aws", "gcloud", "azure", "github", "github-gql", "kubernetes"]
HEADERS = [["content-type", "application/json"], ["authorization", "Bearer <redacted>"]]
SAMPLE = {"owner": "acme", "repo": "app", "org": "acme", "namespace": "prod", "subscriptionId": "sub1",
          "resourceGroupName": "rg1", "project": "proj1", "projectsId": "proj1", "zone": "us-central1-a",
          "region": "us-central1", "locationsId": "us-central1", "bucket": "b1", "object": "o1"}


def read_labels(prov):
    out = {}
    for line in open(os.path.join(HERE, "labels", prov + ".txt")):
        if line.startswith("#") or not line.strip():
            continue
        p = line.rstrip("\n").split(" ")
        if prov in ("aws",):
            key, lab, why = p[0] + " " + p[1], p[2], " ".join(p[3:])
        elif prov in ("gcloud", "github-gql"):
            key, lab, why = p[0], p[1], " ".join(p[2:])
        else:
            key, lab, why = p[0] + " " + p[1], p[2], " ".join(p[3:])
        out[key] = (lab, why)
    return out


def fill(path):
    def rep(m):
        n = m.group(1).rstrip("+")
        return SAMPLE.get(n) or (re.sub(r"[^A-Za-z0-9]", "", n)[:12].lower() + "1")
    return re.sub(r"\{([^}]+)\}", rep, path)


# ---------------------------------------------------------------- AWS through botocore
_aws = {}


def _shape_value(shape, depth=0):
    t = shape.type_name
    if t == "structure":
        return {k: _shape_value(shape.members[k], depth + 1) for k in shape.required_members if depth < 6}
    if t == "list":
        return [_shape_value(shape.member, depth + 1)] if depth < 6 else []
    if t == "map":
        return {}
    if t in ("integer", "long"):
        return max(1, shape.metadata.get("min", 1))
    if t in ("double", "float"):
        return 1.0
    if t == "boolean":
        return False
    if t == "timestamp":
        return "2026-11-25T00:00:00Z"
    if t == "blob":
        return b"x"
    if shape.enum:
        return shape.enum[0]
    n = shape.metadata.get("min", 1) or 1
    return ("x" * max(n, 3))[:max(n, 3)] if shape.name not in ("BucketName",) else "b1"


def aws_request(service, opname, params=None):
    sys.path.insert(0, "/usr/local/lib/python3.13/site-packages/awscli")
    import botocore.session
    from botocore.awsrequest import AWSResponse
    if service not in _aws:
        s = botocore.session.get_session()
        cfg = botocore.config.Config(s3={"addressing_style": "path"}, retries={"max_attempts": 0},
                                     parameter_validation=False)
        c = s.create_client(service, region_name="us-east-1", aws_access_key_id="TESTKEY",
                            aws_secret_access_key="testsecret", config=cfg)
        box = {}

        def grab(request, **kw):
            box["req"] = request
            return AWSResponse(request.url, 200, {}, None)
        c.meta.events.register("before-send", grab)
        _aws[service] = (c, box)
    c, box = _aws[service]
    op = c.meta.service_model.operation_model(opname)
    args = _shape_value(op.input_shape) if op.input_shape else {}
    args.update(params or {})
    box.pop("req", None)
    fn = getattr(c, re.sub(r"(?<!^)(?=[A-Z])", "_", opname).lower().replace("__", "_"), None)
    if fn is None:
        fn = getattr(c, c.meta.method_to_api_mapping and {v: k for k, v in c.meta.method_to_api_mapping.items()}[opname])
    try:
        fn(**args)
    except Exception:
        pass
    r = box.get("req")
    if r is None:
        return None
    from urllib.parse import urlsplit
    u = urlsplit(r.url)
    body = r.body if isinstance(r.body, (bytes, bytearray)) else (r.body.read() if hasattr(r.body, "read") else (r.body or "").encode())
    hdrs = [[k, v.decode() if isinstance(v, bytes) else v] for k, v in r.headers.items()]
    hdrs = [[k, ("AWS4-HMAC-SHA256 Credential=TESTKEY/" + v.split("Credential=TESTKEY/")[1].split(",")[0] + ", SignedHeaders=host, Signature=<redacted>")
             if k.lower() == "authorization" else v] for k, v in hdrs]
    return {"host": u.hostname, "method": r.method, "target": u.path + ("?" + u.query if u.query else ""),
            "headers": hdrs, "body": bytes(body), "facts": {}}


def aws_name(fn_name):
    return fn_name


# ---------------------------------------------------------------- others
def rest_request(op, body=None):
    q = ""
    if isinstance(body, dict) and "_query" in body:
        q = "?" + "&".join("%s=%s" % kv for kv in body["_query"].items())
        body = None
    target = fill(op["path"])
    if op["provider"] == "azure":
        target += ("&" if "?" in target else "?") + "api-version=2024-01-01"
        if q:
            target += "&" + q[1:]
    else:
        target += q
    return {"host": op["host"], "method": op["method"], "target": target, "headers": HEADERS,
            "body": json.dumps(body).encode() if body is not None else b"", "facts": {}}


def gql_lit(v):
    if isinstance(v, dict):
        return "{" + ", ".join("%s: %s" % (k, gql_lit(x)) for k, x in v.items()) + "}"
    if isinstance(v, list):
        return "[" + ", ".join(gql_lit(x) for x in v) + "]"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return "null"
    if isinstance(v, (int, float)):
        return str(v)
    return json.dumps(v)


def gql_request(op, args=None):
    q = "mutation { %s%s { clientMutationId } }" % (op["name"], "(" + gql_lit(args)[1:-1] + ")" if args else "")
    return {"host": op["host"], "method": "POST", "target": op["path"], "headers": HEADERS,
            "body": json.dumps({"query": q}).encode(), "facts": {}}


def variants_for(prov, key, op):
    if prov == "aws":
        return VAR.AWS.get(key)
    if prov == "gcloud":
        return VAR.GCP.get(key) or VAR.GCP_GENERIC.get(key.split(".")[-1])
    if prov == "azure":
        segs = op["path"].strip("/").split("/")
        typ = segs[-2].lower() if segs[-1].startswith("{") else segs[-1].lower()
        return VAR.AZ_TYPE.get(typ)
    if prov in ("github", "github-gql"):
        return VAR.GITHUB.get(key)
    if prov == "kubernetes":
        lits = [s for s in op["path"].strip("/").split("/") if not s.startswith("{")]
        return VAR.K8S_TYPE.get(lits[-1])


def build(prov, op, body):
    if prov == "aws":
        return aws_request(op["service"], op["name"], body)
    if prov == "github-gql":
        return gql_request(op, (body or {}).get("_args") if body else None)
    return rest_request(op, body)


def key_of(prov, op):
    if prov == "aws":
        return op["service"] + " " + op["name"]
    if prov in ("gcloud", "github-gql"):
        return op["name"]
    return op["method"] + " " + op["path"]


def cases(providers=PROVIDERS):
    out = []
    for prov in providers:
        ops = json.load(open(os.path.join(HERE, "ops", prov + ".json")))
        labels = read_labels(prov)
        seen = set()
        for op in ops:
            key = key_of(prov, op)
            if key in seen:
                continue
            seen.add(key)
            label, why = labels[key]
            if label == "cond":
                vs = variants_for(prov, key, op)
                assert vs, (prov, key)
                for i, (lab, note, body) in enumerate(vs):
                    req = build(prov, op, body)
                    if req is None:
                        continue
                    out.append({"id": "%s %s #%d" % (prov, key, i + 1), "provider": prov, "op": key,
                                "label": lab, "why": note, "req": req})
                continue
            req = build(prov, op, None)
            if req is None:
                continue
            out.append({"id": "%s %s" % (prov, key), "provider": prov, "op": key, "label": label, "why": why, "req": req})
    return out


if __name__ == "__main__":
    import collections
    cs = cases(sys.argv[1:] or PROVIDERS)
    c = collections.Counter((x["provider"], x["label"].split(":")[0]) for x in cs)
    for p in PROVIDERS:
        print(p, {k[1]: v for k, v in sorted(c.items()) if k[0] == p})
    print(len(cs))
