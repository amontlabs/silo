"""What breaks when main's clock is behind the Mac's by D seconds.

Each check runs the real tool where there is one (openssl, gpg, make) with the
time main would have, `now - D`; where the rule is a server's (TOTP, a GitHub
App's JWT, AWS's signature), it applies the rule as the provider documents it.
A check prints `ok` or what fails. Usage: breaks.py [D ...] (default 0 5 60 900 5400).
"""
import base64, datetime as dt, hashlib, hmac, json, os, struct, subprocess, sys, tempfile, time
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID

W = tempfile.mkdtemp(prefix="breaks-")


def run(*a, **k):
    return subprocess.run(a, capture_output=True, text=True, **k)


# 1. The boundary's certificates (#347): it signs a leaf per host as main connects.
def _cert(subject, issuer_key, key, not_before, ca=False, issuer=None):
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, subject)])
    b = (x509.CertificateBuilder().subject_name(name).issuer_name(issuer or name)
         .public_key(key.public_key()).serial_number(x509.random_serial_number())
         .not_valid_before(not_before).not_valid_after(not_before + dt.timedelta(days=30))
         .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True))
    if not ca:
        b = b.add_extension(x509.SubjectAlternativeName([x509.DNSName(subject)]), critical=False)
    return b.sign(issuer_key, hashes.SHA256())


def _pem(path, c):
    open(path, "wb").write(c.public_bytes(serialization.Encoding.PEM))


ca_key, leaf_key = ec.generate_private_key(ec.SECP256R1()), ec.generate_private_key(ec.SECP256R1())
# the boundary's CA is made at install, long before the night
ca = _cert("Silo boundary CA", ca_key, ca_key, dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=7), ca=True)
_pem(f"{W}/ca.pem", ca)


def tls(D, backdate):
    nb = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=1 if backdate else 0)
    leaf = _cert("api.anthropic.com", ca_key, leaf_key, nb, issuer=ca.subject)
    _pem(f"{W}/leaf.pem", leaf)
    r = run("openssl", "verify", "-attime", str(int(time.time() - D)), "-CAfile", f"{W}/ca.pem", f"{W}/leaf.pem")
    if r.returncode == 0:
        return "ok"
    err = [l for l in (r.stdout + r.stderr).splitlines() if "error" in l]
    return "refused: " + (err[0].split(": ", 2)[-1] if err else "verify failed")


# 2. TOTP (RFC 6238): a code made in main, checked by a server that allows one step either side.
def totp(key, t):
    m = hmac.new(key, struct.pack(">Q", int(t) // 30), hashlib.sha1).digest()
    o = m[-1] & 15
    return (struct.unpack(">I", m[o:o + 4])[0] & 0x7FFFFFFF) % 10**6


def totp_check(D):
    key, now = b"12345678901234567890", time.time()
    code = totp(key, now - D)
    return "ok" if code in {totp(key, now + s * 30) for s in (-1, 0, 1)} else "code refused (outside the server's +/-1 step)"


# 3. Signing a commit with a GPG key made on the Mac today: gpg in main refuses a key from its future.
def gpg_check(D):
    home = tempfile.mkdtemp(prefix="gnupg-", dir=W)
    env = dict(os.environ, GNUPGHOME=home)
    run("gpg", "--batch", "--passphrase", "", "--quick-gen-key", "dev@example.com", "ed25519", "sign", "never", env=env)
    open(f"{home}/msg", "w").write("tree 0\n")
    r = run("gpg", "--batch", "--faked-system-time", f"{int(time.time() - D)}!", "--local-user", "dev@example.com",
            "--detach-sign", f"{home}/msg", env=env)
    if r.returncode == 0:
        return "ok"
    line = [l for l in r.stderr.splitlines() if "future" in l or "unusable" in l or "skipped" in l]
    return "refused: " + (line[0].split("gpg: ")[-1] if line else r.stderr.strip().splitlines()[-1])


# 4. A GitHub App's JWT made in main: GitHub refuses exp in the past or more than 10 min ahead.
def github_jwt(D):
    gnow, now = time.time() - D, time.time()
    iat, exp = gnow - 60, gnow + 600
    if exp <= now:
        return "refused: 'exp' is in the past for GitHub"
    if exp > now + 600:
        return "refused: 'exp' too far ahead"
    return "ok"


# 5. AWS Signature Version 4 made in main: refused when its time is more than 15 min off.
def aws_sigv4(D):
    return "ok" if abs(D) <= 15 * 60 else "refused: RequestTimeTooSkewed"


# 6. make, with a target the Mac wrote (a file put back, or a shared folder) and a source main edits after.
def make_check(D):
    d = tempfile.mkdtemp(prefix="make-", dir=W)
    open(f"{d}/Makefile", "w").write("out: src\n\tcp src out\n")
    open(f"{d}/out", "w").write("v0\n"); open(f"{d}/src", "w").write("v1\n")
    now = time.time()
    os.utime(f"{d}/out", (now - 2, now - 2))          # the Mac wrote `out` two seconds ago, by the Mac's clock
    os.utime(f"{d}/src", (now - 1 - D, now - 1 - D))  # an agent in main edited `src` a second later, by main's clock
    r = run("make", "-C", d, "-q", "out")
    return "ok" if r.returncode != 0 else "stale: make thinks out is up to date after src changed"


CHECKS = [
    ("TLS through the boundary, leaf dated now", lambda D: tls(D, False)),
    ("TLS through the boundary, leaf dated a day back", lambda D: tls(D, True)),
    ("TOTP code made in main", totp_check),
    ("GPG commit signature, key made on the Mac today", gpg_check),
    ("GitHub App JWT made in main", github_jwt),
    ("AWS SigV4 request signed in main", aws_sigv4),
    ("make after main edits a source of a target the Mac wrote", make_check),
]

if __name__ == "__main__":
    Ds = [float(x) for x in sys.argv[1:]] or [0, 5, 60, 900, 5400]
    out = {}
    print("| check | " + " | ".join(f"{int(d)} s behind" for d in Ds) + " |")
    print("|---|" + "---|" * len(Ds))
    for name, f in CHECKS:
        res = [f(D) for D in Ds]
        out[name] = dict(zip([str(int(d)) for d in Ds], res))
        print(f"| {name} | " + " | ".join(res) + " |")
    json.dump(out, open(os.path.join(os.path.dirname(__file__), "..", "results", "breaks.json"), "w"), indent=1)
