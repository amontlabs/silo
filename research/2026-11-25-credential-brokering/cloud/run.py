"""Runs every case in each mode of the boundary, then counts what leaked.

Root on Linux. Needs: google-auth, google-cloud-storage, azure-identity, azure-mgmt-resource, msal,
PyJWT, cryptography; gcloud; az (azure-cli) at $AZ or on PATH. It adds 169.254.169.254 to lo and
cloud names to /etc/hosts, and puts /etc/hosts back at the end.

    python3 run.py [workdir]
"""
import base64, datetime, glob, http.client, urllib.parse, json, os, secrets, shutil, subprocess, sys, time
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa

HERE = os.path.dirname(os.path.abspath(__file__))
W = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "/tmp/silo-cloud")
AZ = os.environ.get("AZ", shutil.which("az") or "/opt/azcli/bin/az")
UPPORT = 9000
NAMES = ["oauth2.googleapis.com", "storage.googleapis.com", "login.microsoftonline.com", "management.azure.com",
         "hello-abc.a.run.app", "collector.example"]
MODES = {
    "real-tokens": {"metadata": "real", "exchange": "pass", "jwt": "jwt"},
    "placeholders": {"metadata": "placeholder", "exchange": "intercept", "jwt": "jwt"},
    "opaque-placeholders": {"metadata": "placeholder", "exchange": "intercept", "jwt": "opaque"},
}


def sh(cmd, **kw): return subprocess.run(cmd, shell=True, capture_output=True, text=True, **kw)


shutil.rmtree(W, ignore_errors=True); os.makedirs(W)

# the Mac's credentials, and what the clouds know of them ------------------------------------------
sa = rsa.generate_private_key(public_exponent=65537, key_size=2048)
MAC = {
    "google_sa_email": "deployer@silo-demo.iam.gserviceaccount.com", "google_sa_key_id": secrets.token_hex(20),
    "google_sa_private_pem": sa.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                              serialization.NoEncryption()).decode(),
    "google_project": "silo-demo",
    "google_refresh_token": "1//0" + secrets.token_urlsafe(75), "google_client_id": "32555940559.apps.googleusercontent.com",
    "google_client_secret": "ZmssLNjJy2998hD4CTg2ejr2",  # gcloud's own, public
    "azure_tenant": "72f988bf-0000-4000-8000-0000000000aa", "azure_client_id": "04b07795-8ddb-461a-bbee-02f9e1bf7b46",
    "azure_client_secret": "Q~" + secrets.token_urlsafe(30), "azure_refresh_token": "0.AXoA" + secrets.token_urlsafe(200),
}
json.dump(dict(MAC, google_sa_public_pem=sa.public_key().public_bytes(
    serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo).decode()),
    open(f"{W}/real_credentials.json", "w"))

# Silo's authority, which the computer trusts
ca_key = ec.generate_private_key(ec.SECP256R1()); now = datetime.datetime.now(datetime.timezone.utc)
name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Silo boundary (research)")])
ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(ca_key.public_key())
      .serial_number(1).not_valid_before(now - datetime.timedelta(hours=1)).not_valid_after(now + datetime.timedelta(days=3))
      .add_extension(x509.BasicConstraints(ca=True, path_length=None), True)
      .add_extension(x509.KeyUsage(True, False, False, False, False, True, True, False, False), True)
      .add_extension(x509.SubjectKeyIdentifier.from_public_key(ca_key.public_key()), False)
      .sign(ca_key, hashes.SHA256()))
open(f"{W}/ca.pem", "wb").write(ca.public_bytes(serialization.Encoding.PEM))
open(f"{W}/ca.key", "wb").write(ca_key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                                    serialization.NoEncryption()))
# the computer trusts Silo's authority and the system's
open(f"{W}/trust.pem", "w").write(open("/etc/ssl/certs/ca-certificates.crt").read() + open(f"{W}/ca.pem").read())

# the network of the computer --------------------------------------------------------------------
sh("ip addr add 169.254.169.254/32 dev lo")
hosts_orig = open("/etc/hosts").read()
open("/etc/hosts", "w").write(hosts_orig + "\n# silo research\n127.0.0.1 " + " ".join(NAMES) +
                              "\n169.254.169.254 metadata.google.internal metadata\n")

PH_GOOGLE_REFRESH = "1//0" + "siloPH_" + secrets.token_urlsafe(80)[:len(MAC["google_refresh_token"]) - 11]
PH_AZURE_SECRET = "Q~siloPH_" + secrets.token_urlsafe(40)[:len(MAC["azure_client_secret"]) - 9]
PH_AZURE_REFRESH = "0.AXoAsiloPH_" + secrets.token_urlsafe(200)[:len(MAC["azure_refresh_token"]) - 13]

results, leaks = [], {}
procs = []


def guest_env(home):
    e = {k: v for k, v in os.environ.items() if "proxy" not in k.lower() and not k.startswith(("GOOGLE", "AZURE", "CLOUDSDK"))}
    e.update(HOME=home, REQUESTS_CA_BUNDLE=f"{W}/trust.pem", SSL_CERT_FILE=f"{W}/trust.pem",
             CLOUDSDK_CONFIG=f"{home}/.config/gcloud", CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE=f"{W}/trust.pem",
             CLOUDSDK_COMPONENT_MANAGER_DISABLE_UPDATE_CHECK="1", CLOUDSDK_CORE_DISABLE_USAGE_REPORTING="1",
             AZURE_CONFIG_DIR=f"{home}/.azure", AZURE_CORE_COLLECT_TELEMETRY="0", AZURE_CORE_ONLY_SHOW_ERRORS="1",
             NO_PROXY="*")
    return e


def outside(tok):
    """Is this token usable by someone outside the computer? Asked straight of the cloud, not
    through the boundary."""
    if not tok: return None
    for host, path in (("storage.googleapis.com", "/storage/v1/b/demo-bucket"), ("management.azure.com", "/subscriptions"),
                       ("hello-abc.a.run.app", "/")):
        c = http.client.HTTPConnection("127.0.0.1", UPPORT, timeout=5)
        c.request("GET", path, headers={"Host": host, "Authorization": "Bearer " + tok}); r = c.getresponse(); r.read()
        if r.status == 200: return host
    return False


for mode, m in MODES.items():
    if os.environ.get("ONLY_MODE") and mode != os.environ["ONLY_MODE"]: continue
    for p in procs: p.terminate(); p.wait()  # fresh clouds for each mode: refresh tokens rotate
    procs = [subprocess.Popen([sys.executable, f"{HERE}/fake_clouds.py", W, str(UPPORT)])]; time.sleep(1)
    d = f"{W}/{mode}"; home = f"{d}/home"; os.makedirs(f"{home}/.config/gcloud")
    cfg = {"mode": m, "mac": MAC, "upstream_port": UPPORT, "metadata_ip": "169.254.169.254", "state_dir": d,
           "ca_key": f"{W}/ca.key", "ca_cert": f"{W}/ca.pem", "guest_bound_log": f"{d}/guest_bound.log",
           "record": f"{d}/record.jsonl",
           "placeholders": [
               {"placeholder": PH_GOOGLE_REFRESH, "real_key": "google_refresh_token", "hosts": ["oauth2.googleapis.com"], "kind": "google_refresh"},
               {"placeholder": PH_AZURE_SECRET, "real_key": "azure_client_secret", "hosts": ["login.microsoftonline.com"], "kind": "azure_secret"},
               {"placeholder": PH_AZURE_REFRESH, "real_key": "azure_refresh_token", "hosts": ["login.microsoftonline.com"], "kind": "azure_refresh"}]}
    json.dump(cfg, open(f"{d}/boundary.json", "w"))
    b = subprocess.Popen([sys.executable, f"{HERE}/boundary.py", f"{d}/boundary.json"], stdout=subprocess.PIPE, text=True)
    assert b.stdout.readline().strip() == "ready"
    env = guest_env(home)
    # what the computer holds as files: placeholders only
    adc = f"{home}/adc_user.json"
    json.dump({"type": "authorized_user", "client_id": MAC["google_client_id"], "client_secret": MAC["google_client_secret"],
               "refresh_token": PH_GOOGLE_REFRESH}, open(adc, "w"))
    open(f"{home}/.azure-refresh-token", "w").write(PH_AZURE_REFRESH)

    def py(case, **extra):
        r = sh(f"{sys.executable} {HERE}/guest_cases.py {case}", env=dict(env, **extra), timeout=120)
        try: j = json.loads(r.stdout.strip().splitlines()[-1])
        except Exception: j = {"case": case, "ok": False, "error": (r.stderr or r.stdout)[-300:]}
        j["outside"] = [outside(t) for t in j.pop("tokens", []) if t]
        return j

    def cli(name, cmd, ok_text):
        r = sh(cmd, env=env, timeout=180)
        good = r.returncode == 0 and ok_text in r.stdout
        tok = None
        if "access-token" in cmd and good:
            tok = r.stdout.strip().splitlines()[-1].strip().strip('"')
        err = "" if good else " | ".join(l for l in r.stderr.strip().splitlines() if "ERROR" in l or "Error" in l)[:300]
        return {"case": name, "ok": good, "error": err, "outside": [outside(tok)] if tok else []}

    runs = [
        py("metadata_ssrf_guards"),
        py("google_metadata_storage"),
        py("google_id_token_cloud_run"),
        py("google_id_token_decode"),
        cli("gcloud storage ls", "gcloud storage ls gs://demo-bucket", "gs://demo-bucket/hello.txt"),
        cli("gcloud auth print-access-token", "gcloud auth print-access-token", "ya29."),
        py("google_authorized_user_exchange", GOOGLE_APPLICATION_CREDENTIALS=adc),
        py("azure_managed_identity"),
        py("azure_default_credential"),
        cli("az login --identity", f"{AZ} login --identity -o json", "0000"),
        cli("az group list", f"{AZ} group list -o json", "silo"),
        cli("az account get-access-token", f"{AZ} account get-access-token --query accessToken -o tsv", "eyJ"),
        py("azure_client_secret_exchange", AZURE_TENANT_ID=MAC["azure_tenant"], AZURE_CLIENT_ID=MAC["azure_client_id"],
           AZURE_CLIENT_SECRET=PH_AZURE_SECRET),
        py("azure_refresh_rotation", AZURE_TENANT_ID=MAC["azure_tenant"], AZURE_CLIENT_ID=MAC["azure_client_id"]),
        py("exfiltrate"),
    ]
    for r in runs:
        r["mode"] = mode; results.append(r)
        print(f"{mode:20} {'works' if r['ok'] else 'FAILS'}  {r['case']:34} outside={r['outside']} {r.get('error', '')[:160]}", flush=True)
    b.terminate(); b.wait()
    # Is the refresh token Silo holds on the Mac still good after the computer's refreshes?
    c = http.client.HTTPConnection("127.0.0.1", UPPORT, timeout=5)
    c.request("POST", f"/{MAC['azure_tenant']}/oauth2/v2.0/token", body=urllib.parse.urlencode(
        {"grant_type": "refresh_token", "client_id": MAC["azure_client_id"], "refresh_token": MAC["azure_refresh_token"],
         "scope": "https://management.azure.com/.default"}),
        headers={"Host": "login.microsoftonline.com", "Content-Type": "application/x-www-form-urlencoded"})
    r = c.getresponse(); r.read()
    print(f"{mode:20} the Mac's first Azure refresh token after the computer refreshed: HTTP {r.status}", flush=True)
    results.append({"mode": mode, "case": "mac_refresh_token_after", "status": r.status})

    # the leak count: every real value, in everything the computer received or holds
    reals = {"sa_private_key": [MAC["google_sa_private_pem"].splitlines()[1]], "google_refresh": [MAC["google_refresh_token"]],
             "azure_secret": [MAC["azure_client_secret"]], "azure_refresh_initial": [MAC["azure_refresh_token"]]}
    for l in open(f"{W}/issued.jsonl"):
        j = json.loads(l); reals.setdefault(j["kind"], []).append(j["value"])
    blobs = {"bytes sent into the computer": open(f"{d}/guest_bound.log", "rb").read()}
    fs = b""
    for f in [os.path.join(r, n) for r, _, ns in os.walk(home) for n in ns]:
        if os.path.isfile(f):
            try: fs += open(f, "rb").read()
            except OSError: pass
    blobs["files in the computer's home"] = fs
    blobs["what the outsider collected"] = open(f"{W}/collector.log", "rb").read() if os.path.exists(f"{W}/collector.log") else b""
    leaks[mode] = {}
    for where, blob in blobs.items():
        leaks[mode][where] = {k: sum(1 for v in vs if v.encode() in blob) for k, vs in reals.items() if any(v.encode() in blob for v in vs)}
    print(f"{mode:20} leaks: {json.dumps(leaks[mode])}", flush=True)
    if os.path.exists(f"{W}/collector.log"): os.rename(f"{W}/collector.log", f"{d}/collector.log")

for p in procs: p.terminate()
open("/etc/hosts", "w").write(hosts_orig)
json.dump({"results": results, "leaks": leaks}, open(f"{W}/results.json", "w"), indent=1)
