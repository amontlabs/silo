"""Lists every operation that can change something, per provider, from the provider's own
published description. Writes ops/<provider>.json.

Sources (fetched 2026-11-25 into SPECS, default /tmp/claude-0/specs, not committed because of size):
  vercel      https://openapi.vercel.sh/
  supabase    https://api.supabase.com/api/v1-json
  cloudflare  https://raw.githubusercontent.com/cloudflare/api-schemas/main/openapi.json
              (only the products an agent reaches through wrangler or for a web app: see CF_SCOPE)
  railway     introspection of https://backboard.railway.com/graphql/v2 (no token)
  fly-gql     introspection of https://api.fly.io/graphql (no token)
  fly         Machines REST paths read from github.com/superfly/fly-go v0.12.8 flaps/*.go
              (the version flyctl v0.5.17 builds with); Fly publishes no spec we could fetch
              (https://docs.machines.dev/spec/openapi3.json redirects to a 404 on 2026-11-25)

Run: python3 extract.py [SPECS]
"""
import json, os, re, sys

SPECS = sys.argv[1] if len(sys.argv) > 1 else "/tmp/claude-0/specs"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ops")
MUT = ("post", "put", "patch", "delete")

CF_SCOPE = [
    r"^/accounts/\{account_id\}/workers/", r"^/accounts/\{account_id\}/d1/", r"^/accounts/\{account_id\}/storage/kv/",
    r"^/accounts/\{account_id\}/r2/", r"^/accounts/\{account_id\}/pages/", r"^/accounts/\{account_id\}/queues",
    r"^/accounts/\{account_id\}/vectorize/", r"^/accounts/\{account_id\}/hyperdrive/", r"^/accounts/\{account_id\}/workflows",
    r"^/accounts/\{account_id\}/containers", r"^/accounts/\{account_id\}/secrets_store/", r"^/accounts/\{account_id\}/cfd_tunnel",
    r"^/accounts/\{account_id\}/tokens", r"^/user/tokens", r"^/accounts/\{account_id\}$", r"^/accounts/\{account_id\}/members",
    r"^/zones$", r"^/zones/\{zone_id\}$", r"^/zones/\{zone_id\}/dns_records", r"^/zones/\{zone_id\}/settings",
    r"^/zones/\{zone_id\}/workers/", r"^/zones/\{zone_id\}/purge_cache", r"^/zones/\{zone_id\}/rulesets",
    r"^/zones/\{zone_id\}/custom_hostnames", r"^/zones/\{zone_id\}/email/routing",
]


def openapi(provider, fname, host, prefix="", scope=None):
    d = json.load(open(os.path.join(SPECS, fname)))
    ops = []
    for path, item in sorted(d["paths"].items()):
        if scope and not any(re.search(s, path) for s in scope):
            continue
        for m, op in item.items():
            if m not in MUT:
                continue
            ops.append({"provider": provider, "kind": "rest", "host": host, "method": m.upper(),
                        "path": prefix + path, "name": op.get("operationId") or "",
                        "summary": (op.get("summary") or op.get("description") or "").strip().split("\n")[0][:160]})
    return ops


def graphql(provider, fname, host, path):
    d = json.load(open(os.path.join(SPECS, fname)))["data"]["__schema"]
    mt = d["mutationType"]["name"]
    t = next(x for x in d["types"] if x["name"] == mt)
    return [{"provider": provider, "kind": "graphql", "host": host, "method": "POST", "path": path,
             "name": f["name"], "args": [a["name"] for a in f["args"]],
             "summary": (f.get("description") or "").strip().split("\n")[0][:160]} for f in t["fields"]]


FLY_REST = [  # method, path, name, from fly-go v0.12.8 flaps/*.go; base https://api.machines.dev/v1
    ("POST", "/apps", "CreateApp"), ("DELETE", "/apps/{app}", "DeleteApp"),
    ("POST", "/apps/{app}/machines", "Launch"), ("POST", "/apps/{app}/machines/{id}", "Update"),
    ("POST", "/apps/{app}/machines/{id}/start", "Start"), ("POST", "/apps/{app}/machines/{id}/stop", "Stop"),
    ("POST", "/apps/{app}/machines/{id}/restart", "Restart"), ("DELETE", "/apps/{app}/machines/{id}", "Destroy"),
    ("POST", "/apps/{app}/machines/{id}/signal", "Kill"), ("POST", "/apps/{app}/machines/{id}/lease", "AcquireLease"),
    ("DELETE", "/apps/{app}/machines/{id}/lease", "ReleaseLease"), ("POST", "/apps/{app}/machines/{id}/exec", "Exec"),
    ("POST", "/apps/{app}/machines/{id}/cordon", "Cordon"), ("POST", "/apps/{app}/machines/{id}/uncordon", "Uncordon"),
    ("POST", "/apps/{app}/machines/{id}/metadata/{key}", "SetMetadata"),
    ("DELETE", "/apps/{app}/machines/{id}/metadata/{key}", "DeleteMetadata"),
    ("POST", "/apps/{app}/machines/{id}/suspend", "Suspend"),
    ("POST", "/apps/{app}/volumes", "CreateVolume"), ("PUT", "/apps/{app}/volumes/{id}", "UpdateVolume"),
    ("POST", "/apps/{app}/volumes/{id}/snapshots", "CreateVolumeSnapshot"),
    ("PUT", "/apps/{app}/volumes/{id}/extend", "ExtendVolume"), ("DELETE", "/apps/{app}/volumes/{id}", "DeleteVolume"),
    ("POST", "/apps/{app}/secrets/{name}", "SetAppSecret"), ("DELETE", "/apps/{app}/secrets/{name}", "DeleteAppSecret"),
    ("POST", "/apps/{app}/secrets", "UpdateAppSecrets"),
    ("POST", "/apps/{app}/secretkeys/{name}", "SetSecretKey"), ("POST", "/apps/{app}/secretkeys/{name}/generate", "GenerateSecretKey"),
    ("DELETE", "/apps/{app}/secretkeys/{name}", "DeleteSecretKey"),
    ("POST", "/apps/{app}/secretkeys/{name}/encrypt", "EncryptSecretKey"), ("POST", "/apps/{app}/secretkeys/{name}/decrypt", "DecryptSecretKey"),
    ("POST", "/apps/{app}/secretkeys/{name}/sign", "SignSecretKey"), ("POST", "/apps/{app}/secretkeys/{name}/verify", "VerifySecretKey"),
    ("POST", "/apps/{app}/certificates/acme", "CreateACMECertificate"), ("POST", "/apps/{app}/certificates/custom", "CreateCustomCertificate"),
    ("POST", "/apps/{app}/certificates/{host}/check", "CheckCertificate"), ("DELETE", "/apps/{app}/certificates/{host}", "DeleteCertificate"),
    ("DELETE", "/apps/{app}/certificates/{host}/acme", "DeleteACMECertificate"),
    ("DELETE", "/apps/{app}/certificates/{host}/custom", "DeleteCustomCertificate"),
    ("POST", "/apps/{app}/ip_assignments", "AssignIP"), ("DELETE", "/apps/{app}/ip_assignments/{ip}", "DeleteIPAssignment"),
    ("POST", "/postgres", "CreateManagedPostgresCluster"), ("DELETE", "/postgres/{id}", "DeleteManagedPostgresCluster"),
    ("POST", "/postgres/{id}/users", "CreateManagedPostgresUser"), ("PATCH", "/postgres/{id}/users/{user}", "UpdateManagedPostgresUserRole"),
    ("POST", "/postgres/{id}/users/{user}/rotate_password", "RotateManagedPostgresUserPassword"),
    ("DELETE", "/postgres/{id}/users/{user}", "DeleteManagedPostgresUser"),
    ("POST", "/postgres/{id}/databases", "CreateManagedPostgresDatabase"),
    ("POST", "/postgres/{id}/backups", "CreateManagedPostgresBackup"), ("POST", "/postgres/{id}/restore", "RestoreManagedPostgresCluster"),
    ("POST", "/postgres/{id}/fork", "ForkManagedPostgresCluster"),
    ("POST", "/postgres/{id}/databases/{db}/extensions", "EnableManagedPostgresExtension"),
    ("DELETE", "/postgres/{id}/databases/{db}/extensions/{name}", "DisableManagedPostgresExtension"),
    ("POST", "/postgres/{id}/attachments", "CreateManagedPostgresAttachment"),
    ("DELETE", "/postgres/{id}/attachments/{app}", "DeleteManagedPostgresAttachment"),
    ("PATCH", "/postgres/{id}/maintenance_window", "UpdateManagedPostgresMaintenanceWindow"),
]


def main():
    os.makedirs(OUT, exist_ok=True)
    sets = {
        "vercel": openapi("vercel", "vercel.json", "api.vercel.com"),
        "supabase": openapi("supabase", "supabase.json", "api.supabase.com"),
        "cloudflare": openapi("cloudflare", "cloudflare.json", "api.cloudflare.com", "/client/v4", CF_SCOPE),
        "railway": graphql("railway", "railway_full.json", "backboard.railway.com", "/graphql/v2"),
        "fly-gql": graphql("fly-gql", "fly_full.json", "api.fly.io", "/graphql"),
        "fly": [{"provider": "fly", "kind": "rest", "host": "api.machines.dev", "method": m, "path": "/v1" + p,
                 "name": n, "summary": ""} for m, p, n in FLY_REST],
    }
    for k, v in sets.items():
        json.dump(v, open(os.path.join(OUT, k + ".json"), "w"), indent=0)
        print(k, len(v))


if __name__ == "__main__":
    main()
