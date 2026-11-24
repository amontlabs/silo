"""A synthetic corpus: made-up credentials of each format, planted in the contexts where they leak
(a curl command, a .env file, a pasted chat line, JSON, a settings.local.json allow rule), plus
near-misses that must not be reported as a credential. Every value is generated here, at random.

Writes corpus.jsonl: one line per sample, {"text", "expect": rule name or null, "kind"}.
"""
import base64, hashlib, hmac, json, os, random, secrets, string, sys, time
from detector import crc_b62, make_placeholder

A62 = string.ascii_letters + string.digits
R = random.Random(7)


def rnd(n, alphabet=A62):
    return "".join(R.choice(alphabet) for _ in range(n))


def gh(prefix="ghp_"):
    body = rnd(30)
    return prefix + body + crc_b62(body)


def gh_bad(prefix="ghp_"):
    body = rnd(30)
    c = crc_b62(body)
    return prefix + body + (c[:-1] + ("A" if c[-1] != "A" else "B"))


def gh_fine():
    right = rnd(53)
    return "github_pat_" + rnd(22) + "_" + right + crc_b62(right)


def npm():
    body = rnd(30)
    return "npm_" + body + crc_b62(body)


def anthropic():
    return "sk-ant-api03-" + rnd(93, A62 + "-_") + "AA"


def openai():
    return "sk-proj-" + rnd(74, A62 + "-_") + "T3BlbkFJ" + rnd(74, A62 + "-_")


def stripe(live=True):
    return ("sk_live_" if live else "sk_test_") + rnd(99)


def slack():
    return f"xoxb-{R.randrange(10**10, 10**11)}-{R.randrange(10**12, 10**13)}-{rnd(24)}"


def slack_hook():
    return f"https://hooks.slack.com/services/T{rnd(10, string.ascii_uppercase + string.digits)}/B{rnd(10, string.ascii_uppercase + string.digits)}/{rnd(24)}"


def aws():
    return "AKIA" + rnd(16, "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"), rnd(40, A62 + "+/")


def google():
    return "AIza" + rnd(35, A62 + "-_")


def b64u(b):
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def jwt():
    h = b64u(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    p = b64u(json.dumps({"sub": rnd(12), "exp": int(time.time()) + 3600, "role": "service_role"}).encode())
    s = b64u(hmac.new(os.urandom(32), f"{h}.{p}".encode(), hashlib.sha256).digest())
    return f"{h}.{p}.{s}"


def keys():
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric import rsa, ec, ed25519
    out = []
    k = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    out.append(k.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.TraditionalOpenSSL, serialization.NoEncryption()).decode())
    out.append(k.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()).decode())
    e = ec.generate_private_key(ec.SECP256R1())
    out.append(e.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.TraditionalOpenSSL, serialization.NoEncryption()).decode())
    d = ed25519.Ed25519PrivateKey.generate()
    out.append(d.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.OpenSSH, serialization.NoEncryption()).decode())
    return out


CONTEXTS = [
    "curl -H \"Authorization: Bearer {v}\" https://api.example.com/v1/things",
    "export TOKEN={v}",
    "API_KEY={v}\nDEBUG=true\n",
    "here is my key, use it: {v}",
    "{{\"permissions\": {{\"allow\": [\"Bash(curl -s -H 'Authorization: token {v}' https://api.github.com/user)\"]}}}}",
    "const client = new Client({{ apiKey: \"{v}\" }});",
    "   {v}   ",
]


def main(out):
    samples = []

    def add(v, expect, kind="planted"):
        for c in CONTEXTS:
            samples.append({"text": c.format(v=v), "expect": expect, "kind": kind})

    for _ in range(30):
        add(gh("ghp_"), "github-token")
        add(gh(R.choice(["gho_", "ghu_", "ghs_", "ghr_"])), "github-token")
        add(gh_fine(), "github-fine-grained-token")
        add(npm(), "npm-token")
        add(anthropic(), "anthropic-key")
        add(openai(), "openai-key")
        add(stripe(True), "stripe-live-key")
        add(stripe(False), "stripe-test-key")
        add(slack(), "slack-token")
        add(slack_hook(), "slack-webhook")
        add(google(), "google-api-key")
        add(jwt(), "jwt")
        kid, sec = aws()
        add(kid, "aws-access-key-id")
        samples.append({"text": f"[default]\naws_access_key_id = {kid}\naws_secret_access_key = {sec}\n", "expect": "aws-access-key-id", "kind": "planted"})
        pw = rnd(20)
        add(f"postgres://app:{pw}@db.internal.example.com:5432/prod", "database-url")
        # near-misses: must not be reported by a rule
        add(gh_bad("ghp_"), None, "near-miss: checksum wrong")
        add("ghp_" + rnd(36), None, "near-miss: random ghp_")
        add(f"postgres://postgres:postgres@localhost:5432/dev", None, "near-miss: local default")
        add("postgres://app:${DB_PASSWORD}@db:5432/app", None, "near-miss: variable")
        add(make_placeholder("ghp_", 36, f"secret-{R.random()}"), None, "silo placeholder")
    for v in ["AKIAIOSFODNN7EXAMPLE", "ghp_16C7e42F292c6912E7710c838347Ae178B4a"]:
        add(v, None, "near-miss: vendor doc example")
    for k in keys():
        for _ in range(3):
            samples.append({"text": "cat ~/.ssh/id\n" + k, "expect": "private-key", "kind": "planted"})
            samples.append({"text": json.dumps({"type": "service_account", "project_id": "p", "private_key": k, "client_email": "x@p.iam.gserviceaccount.com"}), "expect": "private-key", "kind": "planted: gcp json"})
    # a key block that does not parse
    samples.append({"text": "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAexampleexampleexample\n-----END RSA PRIVATE KEY-----", "expect": None, "kind": "near-miss: broken key"})
    with open(out, "w") as f:
        for s in samples:
            f.write(json.dumps(s) + "\n")
    print(len(samples), "samples")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "corpus.jsonl")
