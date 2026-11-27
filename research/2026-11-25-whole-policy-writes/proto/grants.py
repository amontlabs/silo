"""What a whole-document access write grants, and what changed against the document before it.

A whole-document write sends every grant of a resource, not only the one it changes: Google's
setIamPolicy, AWS's PutRolePolicy, UpdateAssumeRolePolicy, CreatePolicyVersion, PutBucketPolicy and
PutKeyPolicy, an Azure Key Vault PUT with its access policies, an Azure custom role definition, and
a Kubernetes binding or the aws-auth ConfigMap sent whole or as a patch that replaces its list.

Each kind of document is turned into a set of grants. A grant is a tuple whose first item is who
gets it. Comparing the set before with the set after gives what is added and what is removed, and
each added grant is classed as public, powerful or ordinary. Frozen before any run (see README).
"""
import base64
import json
import re

import yaml

PUBLIC, POWERFUL, ORDINARY = "public", "powerful", "ordinary"


# ---------------------------------------------------------------- Google Cloud IAM policies
GCP_PUBLIC = {"allusers", "allauthenticatedusers"}
GCP_POWERFUL_ROLES = {
    "roles/owner", "roles/editor",
    "roles/iam.serviceaccounttokencreator", "roles/iam.serviceaccountuser",
    "roles/iam.workloadidentityuser", "roles/iam.serviceaccountopenidtokencreator",
    "roles/cloudfunctions.developer", "roles/run.developer",  # deploy code that runs as a service account
}


def gcp_role_class(role):
    r = role.lower()
    if r in GCP_POWERFUL_ROLES:
        return POWERFUL
    if not r.startswith("roles/"):
        return POWERFUL  # a custom role: what it allows is not in the policy
    leaf = r.split("/", 1)[1].split(".")[-1]
    if leaf.endswith("admin") or leaf.startswith("iam") or ".iam" in r or r.startswith("roles/resourcemanager."):
        return POWERFUL
    return ORDINARY


def gcp_grants(policy):
    out = set()
    for b in (policy or {}).get("bindings") or []:
        cond = json.dumps(b.get("condition") or {}, sort_keys=True) if b.get("condition") else ""
        for m in b.get("members") or []:
            out.add((m, b.get("role", ""), cond))
    return out


def gcp_audit(policy):
    out = set()
    for a in (policy or {}).get("auditConfigs") or []:
        for c in a.get("auditLogConfigs") or []:
            out.add((a.get("service", ""), c.get("logType", ""), tuple(sorted(c.get("exemptedMembers") or []))))
    return out


def gcp_after(request_policy, update_mask, before):
    """The policy the provider will hold after the write. setIamPolicy changes only the fields of
    its update mask, and with no mask only bindings and etag: audit configs stay as they were."""
    mask = {p.strip() for p in (update_mask or "bindings,etag").split(",")}
    after = {"bindings": request_policy.get("bindings") or []}
    if "auditConfigs" in mask:
        after["auditConfigs"] = request_policy.get("auditConfigs") or []
    elif before is not None:
        after["auditConfigs"] = before.get("auditConfigs") or []
    return after


def gcp_class(g):
    member, role, _ = g
    if member.lower() in GCP_PUBLIC:
        return PUBLIC
    return gcp_role_class(role)


def etag_bytes(e):
    """An etag as the bytes it encodes. gcloud decodes and re-encodes the etag it read, so the
    string it sends can differ from the one it got ("BwX=" comes back as "BwU=")."""
    if not e:
        return None
    try:
        return base64.b64decode(e + "=" * (-len(e) % 4))
    except Exception:
        return e.encode()


# ---------------------------------------------------------------- AWS policy documents
IAM_ESCALATION = {
    "iam:passrole", "iam:createaccesskey", "iam:createloginprofile", "iam:updateloginprofile",
    "iam:attachuserpolicy", "iam:attachrolepolicy", "iam:attachgrouppolicy", "iam:putuserpolicy",
    "iam:putrolepolicy", "iam:putgrouppolicy", "iam:createpolicyversion", "iam:setdefaultpolicyversion",
    "iam:updateassumerolepolicy", "iam:addusertogroup", "sts:assumerole", "kms:putkeypolicy",
    "kms:creategrant", "s3:putbucketpolicy", "lambda:addpermission",
}


def _list(v):
    return [v] if isinstance(v, str) else list(v or [])


def aws_principals(st):
    if "NotPrincipal" in st:
        return [("not", json.dumps(st["NotPrincipal"], sort_keys=True))]
    if "Principal" not in st:
        return [("self", "")]
    p = st["Principal"]
    if p == "*":
        return [("AWS", "*")]
    out = []
    for k, v in p.items():
        for x in _list(v):
            if k == "AWS" and re.fullmatch(r"\d{12}", x):
                x = "arn:aws:iam::%s:root" % x
            out.append((k, x))
    return out


def aws_grants(doc):
    if isinstance(doc, str):
        doc = json.loads(doc)
    out = set()
    sts = doc.get("Statement") or []
    if isinstance(sts, dict):
        sts = [sts]
    for st in sts:
        eff = st.get("Effect", "")
        acts = [a.lower() for a in _list(st.get("Action"))] or ["not:" + a.lower() for a in _list(st.get("NotAction"))]
        res = _list(st.get("Resource")) or ["not:" + r for r in _list(st.get("NotResource"))] or [""]
        cond = json.dumps(st.get("Condition") or {}, sort_keys=True)
        for pr in aws_principals(st):
            for a in acts:
                for r in res:
                    out.add((pr, a, r, eff, cond))
    return out


def aws_account(principal):
    kind, v = principal
    m = re.match(r"arn:aws[\w-]*:(?:iam|sts)::(\d{12}):", v)
    return m.group(1) if kind == "AWS" and m else None


def aws_action_powerful(a):
    return a == "*" or a.endswith(":*") or a.startswith("not:") or a in IAM_ESCALATION


def aws_class(kind, g, before_grants):
    """kind: aws-identity (no principal: the role or user the policy is on), aws-trust (who may
    assume the role), aws-resource (a bucket or key policy)."""
    pr, act, _, eff, _ = g
    if eff != "Allow":
        return ORDINARY
    if pr[0] == "not" or pr == ("AWS", "*"):
        return PUBLIC
    known = {x[0] for x in before_grants}
    accounts = {aws_account(p) for p in known} - {None}
    if kind == "aws-identity":
        return POWERFUL if aws_action_powerful(act) else ORDINARY
    if kind == "aws-trust":
        return POWERFUL if pr[0] in ("AWS", "Federated") and pr not in known else ORDINARY
    acct = aws_account(pr)
    if acct and acct not in accounts:
        return POWERFUL
    return POWERFUL if aws_action_powerful(act) else ORDINARY


# ---------------------------------------------------------------- Azure
AZ_POWERFUL_ROLE_IDS = {
    "8e3af657-a8ff-443c-a75c-2fe8c4bcb635": "Owner",
    "b24988ac-6180-42a0-ab88-20f7382dd24c": "Contributor",
    "18d7d88d-d35e-4fb4-a5c3-7773c20a72d9": "User Access Administrator",
    "f58310d9-a9f6-439a-9e8d-f62e7b41a168": "Role Based Access Control Administrator",
}


def az_vault_grants(vault):
    props = (vault or {}).get("properties") or {}
    out = set()
    for ap in props.get("accessPolicies") or []:
        for kind, perms in (ap.get("permissions") or {}).items():
            for p in perms or []:
                out.add((ap.get("objectId", "").lower(), "%s:%s" % (kind.lower(), p.lower()), ""))
    acl = props.get("networkAcls") or {}
    if acl.get("defaultAction", "Allow").lower() == "allow" or props.get("publicNetworkAccess", "Enabled").lower() == "enabled" and not acl:
        out.add(("network", "any address", ""))
    return out


def az_vault_class(g, before_grants):
    if g[0] == "network":
        return PUBLIC
    known = {x[0] for x in before_grants}
    return POWERFUL if g[0] not in known else ORDINARY


AZ_POWERFUL_ACTION = re.compile(r"^(\*|microsoft\.authorization/\*.*|.*/roleassignments/write|.*/roledefinitions/write|\*/write|microsoft\.authorization/.*/write)$")


def az_roledef_grants(rd):
    props = (rd or {}).get("properties") or rd or {}
    out = set()
    for perm in props.get("permissions") or []:
        for key in ("actions", "dataActions"):
            for a in perm.get(key) or []:
                out.add(("role", "allow:" + a.lower(), ""))
        for key in ("notActions", "notDataActions"):
            for a in perm.get(key) or []:
                out.add(("role", "except:" + a.lower(), ""))
    for s in props.get("assignableScopes") or []:
        out.add(("scope", s.lower(), ""))
    return out


def az_roledef_class(g):
    kind, what = g[0], g[1]
    if kind == "role" and what.startswith("allow:") and AZ_POWERFUL_ACTION.match(what[6:]):
        return POWERFUL
    return ORDINARY


def az_roledef_removed_class(g):
    """A removed exception widens the role: it is classed as the grant it gives back."""
    if g[0] == "role" and g[1].startswith("except:") and AZ_POWERFUL_ACTION.match(g[1][7:]):
        return POWERFUL
    return ORDINARY


def az_assignment_grants(body):
    p = (body or {}).get("properties") or {}
    rid = (p.get("roleDefinitionId") or "").rsplit("/", 1)[-1].lower()
    return {(p.get("principalId", "").lower(), rid, p.get("condition") or "")}


def az_assignment_class(g):
    return POWERFUL if g[1] in AZ_POWERFUL_ROLE_IDS else ORDINARY


# ---------------------------------------------------------------- Kubernetes
K8S_POWERFUL_ROLES = {"cluster-admin", "admin"}


def k8s_binding_grants(obj):
    out = set()
    ref = (obj or {}).get("roleRef") or {}
    for s in (obj or {}).get("subjects") or []:
        out.add(("%s:%s/%s" % (s.get("kind"), s.get("namespace", ""), s.get("name")), "%s:%s" % (ref.get("kind"), ref.get("name")), ""))
    return out


def k8s_binding_class(g):
    return POWERFUL if g[1].split(":", 1)[1] in K8S_POWERFUL_ROLES else ORDINARY


def k8s_awsauth_grants(cm):
    out = set()
    data = (cm or {}).get("data") or {}
    for key, arnkey in (("mapRoles", "rolearn"), ("mapUsers", "userarn")):
        try:
            entries = yaml.safe_load(data.get(key) or "") or []
        except yaml.YAMLError:
            out.add(("unreadable", key, ""))
            continue
        for e in entries if isinstance(entries, list) else []:
            for grp in e.get("groups") or []:
                out.add((e.get(arnkey, ""), grp, e.get("username", "")))
    return out


def k8s_awsauth_class(g):
    if g[0] == "unreadable":
        return POWERFUL
    return POWERFUL if g[1] == "system:masters" else ORDINARY


# ---------------------------------------------------------------- one interface
def grants_of(kind, doc):
    if doc is None:
        return None
    return {
        "gcp-iam": gcp_grants, "aws-identity": aws_grants, "aws-trust": aws_grants, "aws-resource": aws_grants,
        "az-vault": az_vault_grants, "az-roledef": az_roledef_grants, "az-assignment": az_assignment_grants,
        "k8s-binding": k8s_binding_grants, "k8s-aws-auth": k8s_awsauth_grants,
    }[kind](doc)


def class_of(kind, g, before):
    if kind == "gcp-iam":
        return gcp_class(g)
    if kind.startswith("aws-"):
        return aws_class(kind, g, before or set())
    if kind == "az-vault":
        return az_vault_class(g, before or set())
    if kind == "az-roledef":
        return az_roledef_class(g)
    if kind == "az-assignment":
        return az_assignment_class(g)
    if kind == "k8s-binding":
        return k8s_binding_class(g)
    return k8s_awsauth_class(g)


def judge_diff(kind, before_doc, after_doc):
    """Hold when the write adds a public or powerful grant, removes an owner, or turns off audit logs.
    Returns (hold, reasons)."""
    b = grants_of(kind, before_doc) if before_doc is not None else set()
    a = grants_of(kind, after_doc)
    reasons = []
    for g in sorted(a - b, key=str):
        c = class_of(kind, g, b)
        if c != ORDINARY:
            reasons.append("adds %s grant %s" % (c, fmt(g)))
    for g in sorted(b - a, key=str):
        if kind == "gcp-iam" and g[1].lower() == "roles/owner":
            reasons.append("removes owner %s" % g[0])
        if kind == "az-roledef" and az_roledef_removed_class(g) != ORDINARY:
            reasons.append("removes the exception %s" % g[1][7:])
        if kind == "aws-resource" and g[3] == "Deny":
            removed_deny = (g[0], g[1], g[2], "Allow", g[4])
            if class_of(kind, removed_deny, b) != ORDINARY:
                reasons.append("removes a deny on %s" % fmt(g))
    if kind == "gcp-iam":
        lost = gcp_audit(before_doc or {}) - gcp_audit(after_doc)
        for s, t, _ in sorted(lost):
            reasons.append("turns off %s audit logs of %s" % (t, s))
    return bool(reasons), reasons


def judge_whole(kind, after_doc):
    """Without the document before: hold when the document holds any public or powerful grant."""
    a = grants_of(kind, after_doc)
    reasons = ["holds %s grant %s" % (class_of(kind, g, a), fmt(g)) for g in sorted(a, key=str) if class_of(kind, g, a) != ORDINARY]
    return bool(reasons), reasons


def judge_public(kind, after_doc):
    """What the rules in force read from the body alone: is anything in it public."""
    a = grants_of(kind, after_doc)
    reasons = ["holds public grant %s" % fmt(g) for g in sorted(a, key=str) if class_of(kind, g, a) == PUBLIC]
    return bool(reasons), reasons


def fmt(g):
    if isinstance(g[0], tuple):
        return "%s %s on %s" % ("%s:%s" % g[0], g[1], g[2] or "-")
    return "%s %s" % (g[0], g[1])
