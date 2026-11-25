"""Writes the run's config.json: #374's budgets and this folder's boundary in one file.

usage: config.py <out dir> [policy] [total budget of c1 in USD] [extra JSON merged into "boundary"]
Two computers: c1 (main) comes in by CONNECT through its relay, c2 (a fork) through gvproxy's
hand-off. Every secret value here is a fake; the stand-in hosts accept exactly these.
"""
import base64, json, os, sys

E = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.abspath(sys.argv[1]); TLS = os.path.join(E, "run", "tls")
policy = sys.argv[2] if len(sys.argv) > 2 else "finish"
total = float(sys.argv[3]) if len(sys.argv) > 3 and sys.argv[3] else None
extra = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}

def b64(o): return base64.urlsafe_b64encode(json.dumps(o, separators=(",", ":")).encode()).rstrip(b"=").decode()
# Azure's token is a JWT that programs decode (#379, finding 5): the placeholder keeps the real
# header and claims and has another signature.
az_head = b64({"typ": "JWT", "alg": "RS256", "kid": "test"})
az_claims = b64({"aud": "https://management.azure.com/", "iss": "https://sts.windows.net/00000000/", "oid": "11111111", "tid": "00000000", "exp": 4102444800})
AZ_REAL = f"{az_head}.{az_claims}.REALSIGNATURE0123456789abcdef"
AZ_PH = f"{az_head}.{az_claims}.silo-placeholder-signature"

def secrets(c):
    return [
        {"name": "anthropic", "placeholder": f"sk-ant-api03-silo-placeholder-{c}-0000", "value": "sk-ant-api03-REAL-anthropic-key-9f8e7d6c", "hosts": ["api.anthropic.com"]},
        {"name": "openai", "placeholder": f"sk-proj-silo-placeholder-{c}-0000", "value": "sk-proj-REAL-openai-key-5a4b3c2d", "hosts": ["api.openai.com"]},
        {"name": "gemini", "placeholder": f"AIzaSiloPlaceholder{c}000", "value": "AIzaREALgeminiKey11223344", "hosts": ["generativelanguage.googleapis.com"]},
        {"name": "gcp", "placeholder": f"ya29.silo-placeholder-{c}", "value": "ya29.REAL-google-access-token-aabbccdd", "hosts": ["storage.googleapis.com"]},
        {"name": "azure", "placeholder": AZ_PH, "value": AZ_REAL, "hosts": ["management.azure.com"]},
    ] + ([
        {"name": "github", "placeholder": f"ghp_siloPlaceholder{c}0000", "value": "ghp_REALgithubToken55667788", "hosts": ["api.github.com"]},
        {"name": "vercel", "placeholder": f"vercel_silo_placeholder_{c}", "value": "vercel_REAL_token_99aabbcc", "hosts": ["api.vercel.com"]},
    ] if os.environ.get("PLATFORMS") else [])

hosts = [
    {"name": "api.anthropic.com", "mode": "read"}, {"name": "api.openai.com", "mode": "read"},
    {"name": "generativelanguage.googleapis.com", "mode": "read"},
    {"name": "storage.googleapis.com", "mode": "read"}, {"name": "management.azure.com", "mode": "read"},
    {"name": "github.com", "mode": "pass", "ports": [443, 22]},
]
# The platforms whose writes #383's rules hold, read by Silo so the precaution sees each request.
if os.environ.get("PLATFORMS"):
    hosts += [{"name": n, "mode": "read"} for n in ("api.github.com", "api.vercel.com", "backboard.railway.com")]
STANDIN = "127.0.0.1:9443"
resolve = {h["name"]: STANDIN for h in hosts}
resolve["github.com:22"] = "127.0.0.1:2022"
limits = {"total": total} if total else {}
cfg = {
    "policy": policy, "output_tps": 800 if policy == "finish" else 0,
    "prices": os.path.join(os.path.dirname(E), "2026-11-24-spend-budgets", "prices.json"),
    "computers": [{"name": "main", "address": "-", "person": "polar", "limits": limits}, {"name": "fork", "address": "-", "person": "polar"}],
    "people": [{"name": "polar"}],
    "boundary": {
        "computers": [
            {"name": "main", "id": "c1", "connect": f"{OUT}/c1.connect.sock", "secrets": secrets("c1"), "google_token": "gcp", "azure_token": "azure"},
            {"name": "fork", "id": "c2", "handoff": f"{OUT}/c2.handoff.sock", "dns": "127.0.0.1:5302", "secrets": secrets("c2"), "google_token": "gcp", "azure_token": "azure"},
        ],
        "hosts": hosts, "resolve": resolve, "recorder": f"{OUT}/recorder.sock",
        **({"precaution": f"{OUT}/precaution.sock"} if os.environ.get("PRECAUTION") else {}),
        "ca_cert": f"{TLS}/silo-ca.crt", "ca_key": f"{TLS}/silo-ca.key", "leaf_key": f"{TLS}/silo-leaf.key", "upstream_ca": f"{TLS}/up-ca.crt",
    },
}
cfg["boundary"].update(extra)
os.makedirs(OUT, exist_ok=True)
json.dump(cfg, open(os.path.join(OUT, "config.json"), "w"), indent=1)
