"""Extracts the write operations of five control planes into ops/<provider>.json.

Each op: {provider, kind, name, method, path, host, summary, ...}. Sources, all read on 2026-11-25:
  aws        botocore's service models, as bundled with awscli 1.46.3 (botocore 1.43.62)
  gcloud     the API clients bundled with Google Cloud SDK 570.0.0 (generated_clients/apis)
  azure      the azure-mgmt-* SDKs that azure-cli 2.93.0 installs (the operations each az command calls)
  github     github/rest-api-description, api.github.com.json (OpenAPI, version 1.1.4)
  kubernetes kubernetes/kubernetes v1.37.2, api/openapi-spec/swagger.json

Run: python3 extract.py [provider ...]. Needs the CLIs installed where SOURCES says.
"""
import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCES = {
    "botocore": "/usr/local/lib/python3.13/site-packages/awscli",
    "gcloud": "/opt/google-cloud-sdk/lib/googlecloudsdk/generated_clients/apis",
    "azure": glob.glob("/opt/azvenv/lib/python3*/site-packages/azure/mgmt")[0] if glob.glob("/opt/azvenv/lib/python3*/site-packages/azure/mgmt") else "",
    "github": "/opt/specs/github.json",
    "kubernetes": "/opt/specs/k8s.json",
}

READ_NAME = re.compile(r"^(Describe|Get|List|Head|Search|Query|Scan|Select|Lookup|Check|Validate|Test|Estimate|"
                       r"Preview|Simulate|BatchGet|Poll|Receive|Download|Verify|Filter|Discover|Detect)")

# ---------------------------------------------------------------- AWS
AWS_SERVICES = ["s3", "ec2", "rds", "dynamodb", "lambda", "iam", "ecs", "eks", "ecr", "cloudformation", "route53",
                "secretsmanager", "ssm", "kms", "sqs", "sns", "logs", "cloudfront", "elbv2", "organizations"]


def aws():
    sys.path.insert(0, SOURCES["botocore"])
    import botocore.session
    s = botocore.session.get_session()
    out = []
    for sv in AWS_SERVICES:
        m = s.get_service_model(sv)
        for name in m.operation_names:
            if READ_NAME.match(name):
                continue
            op = m.operation_model(name)
            out.append({"provider": "aws", "kind": "aws", "service": sv, "name": name,
                        "method": op.http.get("method", "POST"), "path": op.http.get("requestUri", "/"),
                        "protocol": m.metadata.get("protocol"),
                        "summary": re.sub(r"<[^>]+>", "", (op.documentation or ""))[:200].strip()})
    return out


# ---------------------------------------------------------------- Google Cloud
GCP_APIS = {  # api: version gcloud uses for the commands an agent runs most
    "compute": "v1", "storage": "v1", "sqladmin": "v1beta4", "container": "v1", "run": "v1",
    "iam": "v1", "cloudresourcemanager": "v3", "secretmanager": "v1", "pubsub": "v1", "bigquery": "v2",
    "cloudfunctions": "v2", "dns": "v1", "cloudkms": "v1", "artifactregistry": "v1", "logging": "v2",
    "firestore": "v1",
}
GCP_HOSTS = {"compute": "compute.googleapis.com", "storage": "storage.googleapis.com",
             "sqladmin": "sqladmin.googleapis.com", "bigquery": "bigquery.googleapis.com",
             "run": "us-central1-run.googleapis.com"}
METHOD_RX = re.compile(r"ApiMethodInfo\((.*?)\)\s*\n", re.S)


def gcp():
    out = []
    for api, ver in GCP_APIS.items():
        f = os.path.join(SOURCES["gcloud"], api, ver, "%s_%s_client.py" % (api, ver))
        src = open(f).read()
        base = re.search(r"BASE_URL = '([^']+)'", src).group(1)
        host = re.match(r"https://([^/]+)/", base).group(1)
        prefix = "/" + base.split("/", 3)[3] if base.count("/") > 3 else "/"
        for blk in METHOD_RX.findall(src):
            kv = dict(re.findall(r"(\w+)=u?'([^']*)'", blk))
            meth = kv.get("http_method")
            if not meth or meth == "GET":
                continue
            path = kv.get("flat_path") or kv.get("relative_path")
            name = kv.get("method_id")
            last = name.split(".")[-1]
            if re.match(r"^(get|list|search|query|test|aggregated|batchGet|lookup|fetch|check|validate|export)", last) and meth == "POST":
                ro = True
            else:
                ro = False
            out.append({"provider": "gcloud", "kind": "rest", "api": api, "name": name, "method": meth,
                        "host": host, "path": (prefix.rstrip("/") + "/" + path.lstrip("/")).replace("//", "/"),
                        "reads": ro, "summary": ""})
    return out


# ---------------------------------------------------------------- Azure (ARM)
AZ_PROVIDERS = ["Microsoft.Resources", "Microsoft.Authorization", "Microsoft.Compute", "Microsoft.Storage",
                "Microsoft.Network", "Microsoft.Sql", "Microsoft.KeyVault", "Microsoft.Web",
                "Microsoft.ContainerService", "Microsoft.DocumentDB", "Microsoft.ContainerRegistry",
                "Microsoft.DBforPostgreSQL", "Microsoft.ManagedIdentity", "Microsoft.Cache"]
# Resource types an agent's az commands touch most; the others of these providers are left out.
AZ_TYPES = re.compile(r"/(resourcegroups|deployments|locks|roleassignments|roledefinitions|policyassignments|"
                      r"virtualmachines|virtualmachinescalesets|disks|snapshots|images|availabilitysets|"
                      r"storageaccounts|blobservices|containers|fileservices|shares|managementpolicies|"
                      r"networksecuritygroups|securityrules|publicipaddresses|virtualnetworks|subnets|"
                      r"networkinterfaces|loadbalancers|applicationgateways|privateendpoints|dnszones|"
                      r"servers|databases|firewallrules|elasticpools|vaults|secrets|accesspolicies|sites|"
                      r"serverfarms|slots|config|managedclusters|agentpools|databaseaccounts|sqldatabases|"
                      r"registries|replications|webhooks|tokens|flexibleservers|userassignedidentities|"
                      r"federatedidentitycredentials|redis|backup|deletedvaults|deletedsites|providers|tags)", re.I)


def _az_keep(path):
    if path.lower() in ("/subscriptions/{subscriptionid}/resourcegroups/{resourcegroupname}",
                        "/subscriptions/{subscriptionid}/resourcegroups/{resourcegroupname}/exportTemplate".lower()):
        return True
    m = re.search(r"/providers/(Microsoft\.\w+)/(.*)$", path, re.I)
    if not m or m.group(1).lower() not in {p.lower() for p in AZ_PROVIDERS}:
        return "resourcegroups" in path.lower() and "/providers/" not in path.lower()
    return bool(AZ_TYPES.search("/" + m.group(2)))


def azure():
    out = []
    seen = {}
    root = SOURCES["azure"]
    site = os.path.dirname(os.path.dirname(root))
    # 1. the commands of az itself (AAZ): one file per command, with its URL and method
    for f in sorted(glob.glob(os.path.join(site, "azure", "cli", "command_modules", "*", "aaz", "latest", "**", "_*.py"), recursive=True)):
        src = open(f).read()
        for m in re.finditer(r'format_url\(\s*"([^"]+)".*?def method\(self\):\s*return "([A-Z]+)"', src, re.S):
            path, meth = m.group(1), m.group(2)
            if meth in ("GET", "HEAD") or not _az_keep(path):
                continue
            key = (meth, path.lower())
            if key in seen:
                continue
            cmd = "az " + " ".join(f.split("/aaz/latest/")[1][:-3].replace("/_", "/").split("/"))
            seen[key] = len(out)
            out.append({"provider": "azure", "kind": "rest", "name": cmd, "method": meth,
                        "host": "management.azure.com", "path": path, "summary": "aaz"})
    # 2. the management SDKs the other az commands call, every API version they ship
    for f in sorted(glob.glob(os.path.join(root, "**", "operations", "_*operations.py"), recursive=True), reverse=True):
        if "/aio/" in f:
            continue
        src = open(f).read()
        for m in re.finditer(r"def (build_\w+_request)\((.*?)\n(?=def |class |\Z)", src, re.S):
            body = m.group(0)
            um = re.search(r'_url(?: = kwargs\.pop\(\s*"template_url",\s*| = )"([^"]+)"', body)
            mm = re.search(r'method="([A-Z]+)"', body)
            if not um or not mm or mm.group(1) in ("GET", "HEAD") or not _az_keep(um.group(1)):
                continue
            path, meth = um.group(1), mm.group(1)
            key = (meth, path.lower())
            if key in seen:
                continue
            seen[key] = len(out)
            out.append({"provider": "azure", "kind": "rest", "name": m.group(1)[len("build_"):-len("_request")],
                        "method": meth, "host": "management.azure.com", "path": path,
                        "summary": f.split("/azure/mgmt/")[1].split("/")[0]})
    return out


# ---------------------------------------------------------------- GitHub REST
def github():
    g = json.load(open(SOURCES["github"]))
    out = []
    for p, v in g["paths"].items():
        for meth, o in v.items():
            if meth not in ("post", "put", "patch", "delete"):
                continue
            out.append({"provider": "github", "kind": "rest", "name": o.get("operationId"), "method": meth.upper(),
                        "host": "api.github.com", "path": p, "summary": (o.get("summary") or "")[:160]})
    return out


# ---------------------------------------------------------------- Kubernetes
def kubernetes():
    k = json.load(open(SOURCES["kubernetes"]))
    out = []
    for p, v in k["paths"].items():
        for meth, o in v.items():
            if meth not in ("post", "put", "patch", "delete"):
                continue
            gvk = o.get("x-kubernetes-group-version-kind") or {}
            out.append({"provider": "kubernetes", "kind": "rest", "name": o.get("operationId"), "method": meth.upper(),
                        "host": "kube.capture.test", "path": p, "action": o.get("x-kubernetes-action"),
                        "resource": gvk.get("kind"), "group": gvk.get("group"),
                        "summary": (o.get("description") or "")[:120]})
    return out


ALL = {"aws": aws, "gcloud": gcp, "azure": azure, "github": github, "kubernetes": kubernetes}

if __name__ == "__main__":
    for prov in sys.argv[1:] or ALL:
        ops = ALL[prov]()
        with open(os.path.join(HERE, "ops", prov + ".json"), "w") as f:
            json.dump(ops, f, indent=0)
        print(prov, len(ops))
