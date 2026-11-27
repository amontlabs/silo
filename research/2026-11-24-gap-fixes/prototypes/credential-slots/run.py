"""Runs every experiment of the credential-slot prototype and prints what each one measured.

usage: python3 run.py [--claude] [--boto PYTHON]
  --claude       also drive the installed Claude Code against the fake Messages API
  --boto PYTHON  a Python with boto3 installed, for the SigV4 tests (skipped without it)

Every token here is made up when the run starts. Child programs get a clean environment, so no
variable of the machine running this reaches them.
"""
import base64, gzip, hashlib, json, os, secrets, shutil, socket, string, subprocess, sys, tempfile, time
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from mkph import placeholder

ARGS = sys.argv[1:]
WORK = tempfile.mkdtemp(prefix="cslots-", dir=os.environ.get("TMPDIR"))
PROCS = []
RESULTS = []


def rnd(n, alphabet=string.ascii_letters + string.digits):
    return "".join(secrets.choice(alphabet) for _ in range(n))


REAL = {
    "REAL_GH": "ghp_" + rnd(36),
    "REAL_ANTHROPIC": "sk-ant-api03-" + rnd(80, string.ascii_letters + string.digits + "-_") + "AA",
    "REAL_OPENAI": "sk-proj-" + rnd(48),
    "REAL_GOOGLE": "AIza" + rnd(35),
    "REAL_SESSION": rnd(40),
    "REAL_AWS_AKID": "AKIA" + rnd(16, string.ascii_uppercase + string.digits),
    "REAL_AWS_SECRET": rnd(40, string.ascii_letters + string.digits + "+/"),
}
PH = {"PH_" + k[5:]: placeholder(v) for k, v in REAL.items()}
CLEAN_ENV = {"PATH": "/usr/local/bin:/usr/bin:/bin:/opt/node22/bin", "LANG": "C.UTF-8"}

PORTS = {"proxy": 0, "git": 0, "echo": 0, "evil": 0, "echotls": 0, "s3": 0, "anthropic": 0}
for k in PORTS:
    s = socket.socket(); s.bind(("127.0.0.1", 0)); PORTS[k] = s.getsockname()[1]; s.close()
PROXY = f"http://127.0.0.1:{PORTS['proxy']}"


def check(name, ok, detail=""):
    RESULTS.append((name, ok, detail))
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""), flush=True)


def sh(cmd, env=None, cwd=None, inp=None):
    e = dict(CLEAN_ENV, HOME=f"{WORK}/home", **(env or {}))
    return subprocess.run(cmd, env=e, cwd=cwd, input=inp, capture_output=True, shell=isinstance(cmd, str))


def logs(name):
    p = f"{WORK}/{name}.jsonl"
    return [json.loads(l) for l in open(p)] if os.path.exists(p) else []


def any_real(blob):
    blob = blob if isinstance(blob, bytes) else blob.encode()
    return [k for k, v in REAL.items() if v.encode() in blob]


# certificates --------------------------------------------------------------------------------

def make_certs():
    d = f"{WORK}/certs"; os.makedirs(d)
    run = lambda c: subprocess.run(c, shell=True, check=True, capture_output=True, cwd=d)
    run("openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout ca.key -out ca.pem -days 2 -subj /CN=silo-test-ca -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign,cRLSign")
    for name in ["echo.test", "echo-tls.test", "api.anthropic.test", "s3.test", "evil.test", "git.test"]:
        run(f"openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout {name}.key -out {name}.csr -subj /CN={name}")
        open(f"{d}/{name}.ext", "w").write(f"subjectAltName=DNS:{name}\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n")
        run(f"openssl x509 -req -in {name}.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out {name}.pem -days 2 -extfile {name}.ext")
    return d


def start(cmd, env):
    p = subprocess.Popen(cmd, env=dict(CLEAN_ENV, **env), stdout=subprocess.DEVNULL, stderr=open(f"{WORK}/stderr.txt", "a"))
    PROCS.append(p)


def wait_port(port):
    for _ in range(100):
        try:
            socket.create_connection(("127.0.0.1", port), 0.2).close(); return
        except OSError:
            time.sleep(0.05)
    raise SystemExit(f"port {port} never opened")


def setup():
    certs = make_certs()
    # one test authority plays two parts: the one the computer trusts and the upstream one.
    # In Silo they are separate: the computer's CA key never signs real hosts.
    cfg = {
        "port": PORTS["proxy"], "record": f"{WORK}/record.jsonl", "certs_dir": certs,
        "upstream_ca": f"{certs}/ca.pem", "upstream_tls": ["echo-tls.test", "api.anthropic.test"],
        "hosts": {"git.test": f"127.0.0.1:{PORTS['git']}", "echo.test": f"127.0.0.1:{PORTS['echo']}",
                  "evil.test": f"127.0.0.1:{PORTS['evil']}", "echo-tls.test": f"127.0.0.1:{PORTS['echotls']}",
                  "s3.test": f"127.0.0.1:{PORTS['s3']}", "api.anthropic.test": f"127.0.0.1:{PORTS['anthropic']}"},
        "secrets": [
            {"name": "GITHUB_TOKEN", "value_env": "REAL_GH", "placeholder": PH["PH_GH"],
             "hosts": ["git.test", "echo.test", "echo-tls.test"],
             "slots": [{"header": "authorization", "scheme": "basic-password"},
                       {"header": "authorization", "scheme": "bearer"},
                       {"header": "authorization", "scheme": "token"}]},
            {"name": "ANTHROPIC_API_KEY", "value_env": "REAL_ANTHROPIC", "placeholder": PH["PH_ANTHROPIC"],
             "hosts": ["api.anthropic.test"], "slots": [{"header": "x-api-key"}]},
            {"name": "OPENAI_API_KEY", "value_env": "REAL_OPENAI", "placeholder": PH["PH_OPENAI"],
             "hosts": ["echo.test"], "slots": [{"header": "authorization", "scheme": "bearer"}]},
            {"name": "GOOGLE_API_KEY", "value_env": "REAL_GOOGLE", "placeholder": PH["PH_GOOGLE"],
             "hosts": ["echo.test"], "slots": [{"query": "key"}, {"header": "x-goog-api-key"}]},
            {"name": "SESSION", "value_env": "REAL_SESSION", "placeholder": PH["PH_SESSION"],
             "hosts": ["echo.test"], "slots": [{"header": "cookie", "scheme": "cookie", "cookie": "sid"}]},
            {"name": "AWS", "kind": "sigv4", "value_env": "REAL_AWS_SECRET", "placeholder": PH["PH_AWS_SECRET"],
             "akid_env": "REAL_AWS_AKID", "placeholder_akid": PH["PH_AWS_AKID"], "hosts": ["s3.test"],
             "scopes": ["us-east-1/s3"], "slots": []},
        ],
    }
    json.dump(cfg, open(f"{WORK}/proxy.json", "w"), indent=1)
    allenv = dict(REAL, **PH)
    py = sys.executable
    repo_root = f"{WORK}/srv"; os.makedirs(repo_root)
    start([py, f"{HERE}/servers.py", "git", str(PORTS["git"]), f"{WORK}/git.jsonl", repo_root], allenv)
    start([py, f"{HERE}/servers.py", "echo", str(PORTS["echo"]), f"{WORK}/echo.jsonl"], allenv)
    start([py, f"{HERE}/servers.py", "echo", str(PORTS["evil"]), f"{WORK}/evil.jsonl"], allenv)
    start([py, f"{HERE}/servers.py", "echo", str(PORTS["echotls"]), f"{WORK}/echotls.jsonl", f"{certs}/echo-tls.test.pem", f"{certs}/echo-tls.test.key"], allenv)
    start([py, f"{HERE}/servers.py", "s3", str(PORTS["s3"]), f"{WORK}/s3.jsonl"], allenv)
    start([py, f"{HERE}/servers.py", "anthropic", str(PORTS["anthropic"]), f"{WORK}/anthropic.jsonl", f"{certs}/api.anthropic.test.pem", f"{certs}/api.anthropic.test.key"], allenv)
    start([py, f"{HERE}/slotproxy.py", f"{WORK}/proxy.json"], REAL)  # only the boundary gets real values
    for k in PORTS:
        wait_port(PORTS[k])
    os.makedirs(f"{WORK}/home")
    return certs, repo_root


def record():
    return logs("record")


# (a) git clone and push over HTTP with Basic auth -------------------------------------------

def test_git(repo_root):
    print("\n== (a) git clone and push, the computer holding only the placeholder")
    seed = f"{WORK}/seed"
    sh(f"git init -q -b main {seed} && cd {seed} && git -c user.email=a@b -c user.name=a commit -q --allow-empty -m first && git clone -q --bare {seed} {repo_root}/repo.git")
    sh(f"git -C {repo_root}/repo.git config http.receivepack true")
    url = f"http://x-access-token:{PH['PH_GH']}@git.test/repo.git"
    genv = {"GIT_TERMINAL_PROMPT": "0", "http_proxy": PROXY}
    n0 = len(logs("git"))
    r = sh(["git", "clone", "-q", url, f"{WORK}/clone"], genv)
    check("git clone through the boundary", r.returncode == 0, r.stderr.decode()[-200:])
    cfg = open(f"{WORK}/clone/.git/config").read() if r.returncode == 0 else ""
    check("the clone's .git/config holds the placeholder, not the real token", PH["PH_GH"] in cfg and not any_real(cfg))
    # a push large enough that git sends its pack with chunked framing (above http.postBuffer, 1 MiB)
    big = os.urandom(3 * 1024 * 1024)
    open(f"{WORK}/clone/blob.bin", "wb").write(big)
    r = sh("git add blob.bin && git -c user.email=a@b -c user.name=a commit -q -m big && git push -q origin main", genv, cwd=f"{WORK}/clone")
    check("git push of a 3 MiB commit through the boundary", r.returncode == 0, r.stderr.decode()[-200:])
    head_srv = subprocess.run(["git", "-C", f"{repo_root}/repo.git", "rev-parse", "main"], capture_output=True).stdout.strip()
    head_cli = sh("git rev-parse HEAD", cwd=f"{WORK}/clone").stdout.strip()
    check("the server's main is the pushed commit", head_srv == head_cli and head_srv != b"", head_srv.decode()[:12])
    gl = logs("git")[n0:]
    authed = [x for x in gl if x["auth_ok"]]
    check("every authenticated git request carried the real token, in Basic only",
          authed and all(x["seen"].get("real:REAL_GH") == ["header authorization (basic)"] for x in authed),
          f"{len(authed)} authenticated of {len(gl)} requests; encodings seen {sorted({str(x['content_encoding']) + '/' + str(x['transfer_encoding']) for x in gl})}")
    check("no git request reached the server with the placeholder", not any("ph:PH_GH" in x["seen"] for x in gl))
    recs = [x for x in record() if x.get("host") == "git.test" and x.get("event") == "request"]
    sw = [x for x in recs if x["swapped"]]
    check("the record shows each swap as a fact, slot named", sw and all(x["swapped"][0]["slot"] == "authorization: basic password" for x in sw),
          f"{len(sw)} swaps of {len(recs)} requests")
    big_posts = [x for x in recs if x["method"] == "POST" and x["body_len"] > 1_000_000]
    check("the 3 MiB push body was forwarded as it came", big_posts and big_posts[0]["body_len"] > len(big), f"{big_posts[0]['body_len'] if big_posts else 0} bytes framed")


# (b) a body holding the placeholder passes unchanged ----------------------------------------

def post_via_proxy(host, path, body, headers, tls=False, certs=None):
    hfile = f"{WORK}/body.bin"; open(hfile, "wb").write(body)
    hargs = sum([["-H", f"{k}: {v}"] for k, v in headers.items()], [])
    scheme = "https" if tls else "http"
    extra = ["--cacert", f"{certs}/ca.pem"] if tls else []
    r = sh(["curl", "-sS", "--proxy", PROXY, *extra, "-X", "POST", "--data-binary", f"@{hfile}", *hargs, f"{scheme}://{host}{path}"])
    return r


def test_bodies(certs):
    print("\n== (b) bodies that hold placeholders are forwarded byte for byte")
    conv = json.dumps({"messages": [{"role": "user", "content": "env output:\n" + "\n".join(f"{k[3:]}_X={v}" for k, v in PH.items())}]}).encode()
    for label, body, hdr in [
        ("plain JSON", conv, {"content-type": "application/json"}),
        ("gzip JSON", gzip.compress(conv, 6), {"content-type": "application/json", "content-encoding": "gzip"}),
    ]:
        for tls in (False, True):
            host = "echo-tls.test" if tls else "echo.test"
            sent = hashlib.sha256(body).hexdigest()
            r = post_via_proxy(host, "/echo", body, dict(hdr, Authorization="Bearer " + PH["PH_GH"]), tls, certs)
            try:
                j = json.loads(r.stdout)
            except Exception:
                check(f"{label} over {'HTTPS' if tls else 'HTTP'}", False, r.stderr.decode()[-200:]); continue
            check(f"{label} over {'HTTPS' if tls else 'HTTP'}: body hash equal before and after",
                  j["sha256_received"] == sent, sent[:16])
            check(f"{label} over {'HTTPS' if tls else 'HTTP'}: real token only in the Authorization header, placeholders still in the body",
                  [w.lower() for w in j["seen"].get("real:REAL_GH", [])] == ["header authorization"] and all(f"ph:{k}" in j["seen"] for k in PH) and
                  not any(k.startswith("real:") and k != "real:REAL_GH" for k in j["seen"]),
                  ", ".join(f"{k}->{v}" for k, v in sorted(j["seen"].items()) if k.startswith("real")))
    rec = [x for x in record() if x.get("event") == "request" and x.get("path") == "/echo"]
    check("the record lists the placeholders seen in the bodies, as facts", rec and len(rec[-1]["placeholder_seen"]) >= 6,
          f"{len(rec[-1]['placeholder_seen'])} sightings in the last request")


# (c) replies that echo the real value are scrubbed -------------------------------------------

def get_via_proxy(url, headers, certs=None, extra=()):
    hargs = sum([["-H", f"{k}: {v}"] for k, v in headers.items()], [])
    ca = ["--cacert", f"{certs}/ca.pem"] if url.startswith("https") else []
    return sh(["curl", "-sS", "-i", "--compressed", "--proxy", PROXY, *ca, *hargs, *extra, url])


def test_replies(certs):
    print("\n== (c) replies that send the real value back")
    hdr = {"Authorization": "Bearer " + PH["PH_GH"]}
    tok_ph, tok = PH["PH_GH"], REAL["REAL_GH"]
    for url in ["http://echo.test/reflect/plain", "https://echo-tls.test/reflect/plain", "http://echo.test/reflect/gzip",
                "http://echo.test/reflect/chunked", "http://echo.test/reflect/base64", "http://echo.test/reflect/first-half",
                "http://echo.test/reflect/masked"]:
        r = get_via_proxy(url, hdr, certs)
        out = r.stdout
        leaked = any_real(out)
        ph_back = tok_ph.encode() in out or base64.b64encode(tok_ph.encode()) in out or tok_ph[:18].encode() in out
        check(f"{url.split('/reflect/')[1]:>10} ({url.split(':')[0]}): no real value reaches the computer", not leaked and r.returncode == 0,
              ("placeholder form returned" if ph_back else "neither form found") + f"; {len(out)} bytes")
    sc = [x for x in record() if x.get("event") == "reply" and x["scrubbed"]]
    check("the record lists each scrub, by secret and form", len(sc) >= 6, "; ".join(sorted({h['form'] + ' in ' + h['at'] for x in sc for h in x['scrubbed']})))
    r = get_via_proxy("http://echo.test/reflect/reversed", hdr, certs)
    check("a reversed echo is NOT caught (known limit)", tok[::-1].encode() in r.stdout, "the reversed real token reached the computer")
    # a 302 to another host whose Location carries the token
    n0 = len(logs("evil"))
    r = get_via_proxy("http://echo.test/redirect", hdr, certs, ["-L", "--location-trusted"])
    ev = logs("evil")[n0:]
    check("redirect to another host, client resending Authorization: the other host gets placeholders only",
          ev and not any(k.startswith("real:") for x in ev for k in x["seen"]), json.dumps([x["seen"] for x in ev]))


# bad paths ------------------------------------------------------------------------------------

def raw_request(host, data, tls=False, certs=None):
    import ssl
    s = socket.create_connection(("127.0.0.1", PORTS["proxy"]))
    if tls:
        s.sendall(f"CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n".encode()); s.recv(4096)
        s = ssl.create_default_context(cafile=f"{certs}/ca.pem").wrap_socket(s, server_hostname=host)
    s.sendall(data); s.settimeout(5)
    out = b""
    try:
        while True:
            d = s.recv(65536)
            if not d:
                break
            out += d
    except Exception:
        pass
    s.close(); return out


def last_echo():
    return logs("echo")[-1] if logs("echo") else {}


def test_bad_paths(certs):
    print("\n== bad paths")
    ph = PH["PH_GH"]

    def req(lines, host="echo.test", path="/collect"):
        return ("GET http://%s%s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n" % (host, path, host) + "".join(l + "\r\n" for l in lines) + "\r\n").encode()

    n = len(logs("echo")); raw_request("echo.test", req([f"X-Debug: {ph}"]))
    e = logs("echo")[n:]
    check("placeholder in another header of an allowed host: forwarded as is", e and "real:REAL_GH" not in e[0]["seen"] and "ph:PH_GH" in e[0]["seen"])
    n = len(logs("echo")); raw_request("echo.test", req([f"Authorization: Bearer {ph}", f"Authorization: Bearer {ph}"]))
    e = logs("echo")[n:]
    check("the slot header sent twice: neither is swapped", e and "real:REAL_GH" not in e[0]["seen"])
    n = len(logs("echo")); raw_request("echo.test", ("GET http://echo.test/collect HTTP/1.1\r\nHost: evil.test\r\nConnection: close\r\nAuthorization: Bearer %s\r\n\r\n" % ph).encode())
    e = logs("echo")[n:]
    check("Host header names another host than the request (fronting): not swapped", e and "real:REAL_GH" not in e[0]["seen"])
    n = len(logs("echo")); out = raw_request("echo.test", req([f"Authorization: Bearer {ph[:20]}", f" {ph[20:]}"]))
    check("placeholder split over an obs-fold line: refused with 400, nothing forwarded", out.startswith(b"HTTP/1.1 400") and len(logs("echo")) == n)
    n = len(logs("echo")); out = raw_request("echo.test", (f"GET http://echo.test/collect HTTP/1.1\r\nHost: echo.test\r\nAuthorization: Bearer {ph}\rX-Injected: 1\r\n\r\n").encode())
    check("bare CR inside a header line: refused with 400", out.startswith(b"HTTP/1.1 400") and len(logs("echo")) == n)
    n = len(logs("echo")); raw_request("echo.test", req([f"Authorization: Basic {base64.b64encode(('u:' + ph + 'x').encode()).decode()}"]))
    e = logs("echo")[n:]
    check("Basic password that only contains the placeholder: not swapped", e and "real:REAL_GH" not in e[0]["seen"])
    n = len(logs("echo")); raw_request("echo.test", req([f"Authorization: Bearer {PH['PH_ANTHROPIC']}"]))
    e = logs("echo")[n:]
    check("another secret's placeholder in this host's slot: not swapped", e and not any(k.startswith("real:") for k in e[0]["seen"]))
    n = len(logs("evil")); raw_request("evil.test", req([f"Authorization: Bearer {ph}"], host="evil.test"))
    e = logs("evil")[n:]
    check("the slot, towards a host not allowed for the secret: placeholder forwarded, nothing denied", e and "ph:PH_GH" in e[0]["seen"] and "real:REAL_GH" not in e[0]["seen"])
    # query and cookie slots
    n = len(logs("echo")); raw_request("echo.test", req([], path=f"/collect?alt=json&key={PH['PH_GOOGLE']}"))
    e = logs("echo")[n:]
    check("query slot ?key= swapped on its host", e and e[0]["seen"].get("real:REAL_GOOGLE") == ["path"])
    n = len(logs("echo")); raw_request("echo.test", req([], path=f"/collect?other={PH['PH_GOOGLE']}"))
    e = logs("echo")[n:]
    check("placeholder in another query parameter: not swapped", e and "real:REAL_GOOGLE" not in e[0]["seen"])
    n = len(logs("echo")); raw_request("echo.test", req([f"Cookie: theme=dark; sid={PH['PH_SESSION']}; x=1"]))
    e = logs("echo")[n:]
    check("cookie slot sid= swapped, other cookies untouched", e and e[0]["seen"].get("real:REAL_SESSION") == ["header Cookie"])
    rec = record()
    check("the record never holds a real value", not any_real(open(f"{WORK}/record.jsonl", "rb").read()))


# AWS SigV4 ------------------------------------------------------------------------------------

BOTO = r'''
import sys, os, json, boto3, botocore
from botocore.config import Config
mode = sys.argv[1]
tls = os.environ.get("TLS") == "1"
c = boto3.client("s3", endpoint_url=("https" if tls else "http") + "://s3.test", region_name="us-east-1",
    aws_access_key_id=os.environ["AKID"], aws_secret_access_key=os.environ["SECRET"], verify=os.environ.get("CA") if tls else None,
    config=Config(proxies={("https" if tls else "http"): os.environ["PX"]}, s3={"addressing_style": "path"}, signature_version=os.environ.get("SIGV") or None,
                  request_checksum_calculation=os.environ.get("CK", "when_supported"), retries={"max_attempts": 1}))
out = {}
try:
    if mode == "put":
        data = os.urandom(int(os.environ["SIZE"]))
        c.put_object(Bucket="b", Key="k", Body=data)
        got = c.get_object(Bucket="b", Key="k")["Body"].read()
        out["roundtrip"] = got == data
    if mode == "upload":
        import io
        data = os.urandom(int(os.environ["SIZE"]))
        c.upload_fileobj(io.BytesIO(data), "b", "u")
        out["roundtrip"] = c.get_object(Bucket="b", Key="u")["Body"].read() == data
    if mode == "presign":
        out["url"] = c.generate_presigned_url("get_object", Params={"Bucket": "b", "Key": "k"}, ExpiresIn=300)
    if mode == "otherregion":
        c2 = boto3.client("s3", endpoint_url="http://s3.test", region_name="eu-west-3",
            aws_access_key_id=os.environ["AKID"], aws_secret_access_key=os.environ["SECRET"],
            config=Config(proxies={"http": os.environ["PX"]}, s3={"addressing_style": "path"}, retries={"max_attempts": 1}))
        c2.get_object(Bucket="b", Key="k"); out["ok"] = True
except botocore.exceptions.ClientError as e:
    out["error"] = e.response["Error"]["Code"]
print(json.dumps(out))
'''


def test_sigv4(boto_py):
    print("\n== AWS SigV4: the computer signs with a placeholder secret key, the boundary re-signs")
    open(f"{WORK}/boto_t.py", "w").write(BOTO)
    benv = {"AKID": PH["PH_AWS_AKID"], "SECRET": PH["PH_AWS_SECRET"], "PX": PROXY, "CA": f"{WORK}/certs/ca.pem"}
    short = lambda p: p if p.startswith(("STREAMING", "UNSIGNED", "presigned")) else "sha256 of body"
    for tls in ("0", "1"):
        for size, ck in [(1000, "when_supported"), (1000, "when_required"), (9 * 1024 * 1024, "when_supported")]:
            n = len(logs("s3"))
            r = sh([boto_py, f"{WORK}/boto_t.py", "put"], dict(benv, SIZE=str(size), CK=ck, TLS=tls))
            out = r.stdout.decode().strip() or r.stderr.decode()[-300:]
            s3 = logs("s3")[n:]
            check(f"{'HTTPS' if tls == '1' else 'HTTP'} put_object {size} bytes, checksums {ck}: object round-trips", '"roundtrip": true' in out,
                  out + " | payload: " + ", ".join(sorted({x["method"] + " " + short(x["payload"]) for x in s3})))
        n = len(logs("s3"))
        r = sh([boto_py, f"{WORK}/boto_t.py", "upload"], dict(benv, SIZE=str(20 * 1024 * 1024), TLS=tls))
        s3 = logs("s3")[n:]
        check(f"{'HTTPS' if tls == '1' else 'HTTP'} upload_fileobj 20 MiB (multipart): round-trips", '"roundtrip": true' in r.stdout.decode(),
              r.stdout.decode().strip()[:80] + " | " + ", ".join(sorted({x["method"] + " " + short(x["payload"]) for x in s3})) + f" | {len(s3)} requests")
    rs = [x for x in record() if any(s.get("slot", "").startswith("sigv4") for s in x.get("swapped", []))]
    check("before re-signing, the boundary verified the computer's signature with the placeholder key",
          rs and all(s.get("placeholder_signature_valid", s.get("placeholder_chain_valid")) for x in rs for s in x["swapped"]), f"{len(rs)} requests re-signed")
    for sigv in ("", "s3v4"):
        r = sh([boto_py, f"{WORK}/boto_t.py", "presign"], dict(benv, SIGV=sigv))
        url = json.loads(r.stdout)["url"]
        kind = "SigV2 (boto3's default for presign)" if "AWSAccessKeyId=" in url else "SigV4"
        r1 = sh(["curl", "-sS", "-o", "/dev/null", "-w", "%{http_code}", "--proxy", PROXY, url])
        r2 = sh(["curl", "-sS", "-o", "/dev/null", "-w", "%{http_code}", url.replace("http://s3.test", f"http://127.0.0.1:{PORTS['s3']}")])
        check(f"a presigned URL made inside, {kind}: works through the boundary", r1.stdout == b"200", r1.stdout.decode())
        check(f"the same {kind} URL used outside the boundary is refused", r2.stdout == b"403", r2.stdout.decode())
    if "--awsjs" in ARGS:
        # over plain HTTP, proxy-agent writes the request head twice (seen on a raw socket): HTTPS only
        for tls in ("1",):
            n = len(logs("s3"))
            r = sh(["node", f"{HERE}/aws_js.cjs"], dict(benv, TLS=tls, NODE_PATH=ARGS[ARGS.index("--awsjs") + 1]))
            s3 = logs("s3")[n:]
            check(f"AWS SDK for JavaScript v3 over {'HTTPS' if tls == '1' else 'HTTP'}: buffer and stream PutObject round-trip",
                  r.stdout.strip() == b'{"buffer":true,"stream":true}', r.stdout.decode().strip() + r.stderr.decode()[-200:] + " | " + ", ".join(sorted({x["method"] + " " + short(x["payload"]) for x in s3})))
    rs = [x for x in record() if any(s.get("slot", "").startswith("sigv4") for s in x.get("swapped", []))]
    ch = [s for x in rs for s in x["swapped"] if s["slot"] == "sigv4 aws-chunked"]
    print(f"INFO signed aws-chunked bodies (STREAMING-AWS4-HMAC-SHA256-PAYLOAD) seen: {len(ch)}")
    if ch:
        check("aws-chunked bodies re-signed chunk by chunk, length kept", all(s["length_kept"] and s["placeholder_chain_valid"] for s in ch), f"{sum(s['chunks'] for s in ch)} chunks")
    r = sh([boto_py, f"{WORK}/boto_t.py", "otherregion"], benv)
    check("a region outside the secret's scopes is not signed: AWS answers 403", "SignatureDoesNotMatch" in r.stdout.decode(), r.stdout.decode().strip())


# (d) Claude Code ------------------------------------------------------------------------------

def test_claude(certs):
    print("\n== (d) Claude Code against the fake Messages API, with placeholders only")
    home = f"{WORK}/cchome"; os.makedirs(home); os.makedirs(f"{WORK}/ccwork")
    env = {"HOME": home, "CLAUDE_CONFIG_DIR": f"{home}/.claude", "HTTPS_PROXY": PROXY, "NODE_EXTRA_CA_CERTS": f"{certs}/ca.pem",
           "ANTHROPIC_BASE_URL": "https://api.anthropic.test", "ANTHROPIC_API_KEY": PH["PH_ANTHROPIC"],
           "OPENAI_API_KEY": PH["PH_OPENAI"], "GITHUB_TOKEN": PH["PH_GH"], "IS_SANDBOX": "1", "DISABLE_TELEMETRY": "1",
           "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_AUTOUPDATER": "1"}
    e = dict(CLEAN_ENV, **env)
    t0 = time.time()
    r = subprocess.run("timeout 120 claude -p 'Do the task.' --dangerously-skip-permissions --output-format stream-json --verbose",
                       shell=True, env=e, cwd=f"{WORK}/ccwork", capture_output=True, stdin=subprocess.DEVNULL)
    al = logs("anthropic")
    check("Claude Code finished its turn", r.returncode == 0 and b'"Done."' in r.stdout, f"exit {r.returncode}, {len(al)} requests, {time.time() - t0:.1f}s")
    check("every request reached the provider with the real key in x-api-key", al and all(x["key_ok"] for x in al))
    check("the real key was never anywhere else in a request", all(x["seen"].get("real:REAL_ANTHROPIC") == ["header x-api-key"] and
                                                                   not any(k.startswith("real:") and k != "real:REAL_ANTHROPIC" for k in x["seen"]) for x in al))
    later = [x for x in al if any(k == "ph:PH_GH" for k in x["seen"])]
    check("after `env`, the placeholders travel in the body and arrive as placeholders", later,
          f"{len(later)} requests; e.g. {later[0]['seen'] if later else ''}")
    recs = {x["body_sha256"]: x for x in record() if x.get("host") == "api.anthropic.test" and x.get("event") == "request"}
    check("each body arrived with the hash it left with", al and all(x["sha256_received"] in recs for x in al),
          f"encodings {sorted({str(x['content_encoding']) for x in al})}")
    check("the conversation Claude Code kept holds no real value", not any_real(r.stdout) and not any(any_real(open(os.path.join(dp, f), 'rb').read()) for dp, _, fs in os.walk(home) for f in fs))


def test_gh(certs):
    print("\n== gh, the GitHub CLI, with a placeholder token for an enterprise host")
    n = len(logs("echotls"))
    env = {"GH_HOST": "echo-tls.test", "GH_ENTERPRISE_TOKEN": PH["PH_GH"], "HTTPS_PROXY": PROXY, "SSL_CERT_FILE": f"{certs}/ca.pem",
           "GH_NO_UPDATE_NOTIFIER": "1", "GH_PROMPT_DISABLED": "1"}
    r = sh(["gh", "api", "/user"], env)
    rec = [x for x in record() if x.get("host") == "echo-tls.test" and x.get("event") == "request" and "/api/" in x.get("path", "")]
    e = logs("echotls")[n:]
    check("gh sends the token in a slot the boundary swaps", rec and rec[-1]["swapped"] and e and e[-1]["seen"].get("real:REAL_GH"),
          f"slot {rec[-1]['swapped'] if rec else None}, path {rec[-1]['path'] if rec else None}")


def main():
    try:
        certs, repo_root = setup()
        if "--only-aws" not in ARGS:
            test_git(repo_root)
            test_bodies(certs)
            test_replies(certs)
            test_bad_paths(certs)
        if "--boto" in ARGS:
            test_sigv4(ARGS[ARGS.index("--boto") + 1])
        if "--gh" in ARGS:
            test_gh(certs)
        if "--claude" in ARGS:
            test_claude(certs)
        errs = [x for x in record() if x.get("event") in ("error",)]
        if errs:
            print("\nboundary errors:", *[x["why"] for x in errs][:5], sep="\n  ")
    finally:
        for p in PROCS:
            p.terminate()
        ok = sum(1 for _, o, _ in RESULTS if o)
        print(f"\n{ok} of {len(RESULTS)} checks passed. Work folder: {WORK}")
        if "--keep" not in ARGS:
            shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    main()
