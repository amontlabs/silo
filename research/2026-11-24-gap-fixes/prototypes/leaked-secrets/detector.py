"""A detector for credentials Silo never held, in text Silo already reads.

Each rule has a grade:
  checked  - the bytes pass a check the issuer built into the format (a CRC32 checksum, a private
             key that parses, a JWT whose header decodes). A random string passes with a
             probability near 1 in 4 billion or less. What it proves is the format, not that the
             credential is live.
  prefix   - a fixed prefix, length and alphabet the issuer documents, with no checksum. Example and
             test values with the same shape exist and cannot be told apart offline.
  shape    - a shape with a password in it (a database URL). Many are local defaults.
  entropy  - a long random-looking string next to a word such as "token". An opinion.

The detector never prints a value: callers get the rule, the span and a fingerprint.
"""
import base64, binascii, hashlib, json, math, re, zlib

B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def b62_6(n):
    s = ""
    while n:
        n, r = divmod(n, 62)
        s = B62[r] + s
    return s.rjust(6, "0")


def crc_b62(body):
    return b62_6(zlib.crc32(body.encode()) & 0xFFFFFFFF)


# Values that vendors publish in their docs as examples. A match on one of these is never reported.
DOC_EXAMPLES = {
    "AKIAIOSFODNN7EXAMPLE",
    "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
    "ghp_16C7e42F292c6912E7710c838347Ae178B4a",  # docs.github.com example
}

# Silo's own placeholders: a fixed marker inside a provider-shaped value, so that programs which
# check the prefix and the length accept it, while the provider's checksum fails on purpose and
# Silo's marker identifies it exactly. See make_placeholder().
SILO_MARK = "S1Lo"


def make_placeholder(prefix, body_len, secret_id):
    """A placeholder shaped like the provider's token, with Silo's marker and a checksum that is
    deliberately wrong for the provider's format (so GitHub-style scanners do not report it)."""
    core = (SILO_MARK + hashlib.sha256(secret_id.encode()).hexdigest())[: body_len - 6]
    core = core.ljust(body_len - 6, "0")
    good = crc_b62(core)
    bad = good[:-1] + ("0" if good[-1] != "0" else "1")
    return prefix + core + bad


def entropy(s):
    if not s:
        return 0.0
    n = len(s)
    counts = {}
    for c in s:
        counts[c] = counts.get(c, 0) + 1
    return -sum(v / n * math.log2(v / n) for v in counts.values())


def _github_classic(m):
    tok = m.group(0)
    body, chk = tok[4:34], tok[34:40]
    return crc_b62(body) == chk


def _github_fine(m):
    right = m.group(2)
    return crc_b62(right[:53]) == right[53:59]


def _npm(m):
    tok = m.group(0)
    body, chk = tok[4:34], tok[34:40]
    return crc_b62(body) == chk


def _b64url_json(seg):
    try:
        raw = base64.urlsafe_b64decode(seg + "=" * (-len(seg) % 4))
        return json.loads(raw)
    except Exception:
        return None


def _jwt(m):
    h = _b64url_json(m.group(1))
    p = _b64url_json(m.group(2))
    return isinstance(h, dict) and "alg" in h and isinstance(p, dict)


def _pem_key(m):
    block = m.group(0).replace("\\n", "\n")
    try:
        from cryptography.hazmat.primitives import serialization
        if "OPENSSH" in block:
            serialization.load_ssh_private_key(block.encode(), password=None)
        else:
            serialization.load_pem_private_key(block.encode(), password=None)
        return True
    except TypeError:  # encrypted key: still a key, protected by a passphrase
        return True
    except Exception:
        return False


TRIVIAL_PW = {"password", "pass", "postgres", "root", "secret", "changeme", "example", "admin",
              "user", "test", "pwd", "mysql", "redis", "guest", "xxx", "xxxx", "****", "******",
              "mypassword", "yourpassword", "dbpass", "db_password", "<password>", "s3cr3t"}


def _db_url(m):
    pw = m.group("pw")
    if pw.lower() in TRIVIAL_PW or pw.startswith(("${", "$", "<", "{{", "%(")) or len(pw) < 6:
        return False
    if re.fullmatch(r"[\*x\.]+", pw):
        return False
    return True


A62 = r"[A-Za-z0-9]"
RULES = [
    # name, grade, regex, validator (None: the regex is the rule)
    ("github-token", "checked", re.compile(r"\bgh[pousr]_" + A62 + r"{36}\b"), _github_classic),
    ("github-fine-grained-token", "checked",
     re.compile(r"\bgithub_pat_(" + A62 + r"{22})_(" + A62 + r"{59})\b"), _github_fine),
    ("npm-token", "checked", re.compile(r"\bnpm_" + A62 + r"{36}\b"), _npm),
    ("private-key", "checked",
     re.compile(r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----"
                r"(?:[A-Za-z0-9+/=\s]|\\n|\\r)+?-----END (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----"),
     _pem_key),
    ("jwt", "checked",
     re.compile(r"\b(eyJ[A-Za-z0-9_-]{10,})\.(eyJ[A-Za-z0-9_-]{10,})\.([A-Za-z0-9_-]{16,})"), _jwt),
    ("anthropic-key", "prefix", re.compile(r"\bsk-ant-(?:api03|admin01)-[A-Za-z0-9_-]{93}AA\b"), None),
    ("anthropic-sign-in-token", "prefix", re.compile(r"\bsk-ant-(?:oat01|ort01)-[A-Za-z0-9_-]{60,200}"), None),
    ("openai-key", "prefix",
     re.compile(r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,74}T3BlbkFJ[A-Za-z0-9_-]{20,74}\b"), None),
    ("stripe-live-key", "prefix", re.compile(r"\b(?:sk|rk)_live_[A-Za-z0-9]{24,99}\b"), None),
    ("stripe-test-key", "prefix", re.compile(r"\b(?:sk|rk)_test_[A-Za-z0-9]{24,99}\b"), None),
    ("slack-token", "prefix", re.compile(r"\bxox[baprs]-[0-9]{8,13}-[0-9]{8,13}(?:-[0-9]{8,13})?-[A-Za-z0-9]{24,34}\b"), None),
    ("slack-webhook", "prefix",
     re.compile(r"https://hooks\.slack\.com/services/T[A-Z0-9]{8,12}/B[A-Z0-9]{8,12}/[A-Za-z0-9]{24}"), None),
    ("aws-access-key-id", "prefix", re.compile(r"\b(?:AKIA|ASIA)[A-Z2-7]{16}\b"), None),
    ("google-api-key", "prefix", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b"), None),
    ("database-url", "shape",
     re.compile(r"\b(?:postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|rediss?|amqps?)://"
                r"[A-Za-z0-9._%-]{1,64}:(?P<pw>[^\s:@/'\"`]{1,128})@[A-Za-z0-9.-]+"), _db_url),
]

KEYWORD = re.compile(r"(?i)(api[_-]?key|secret|token|passw(?:or)?d|bearer|auth|credential|private[_-]?key)")
CANDIDATE = re.compile(r"[A-Za-z0-9+/_\-]{32,}={0,2}")
HEX = re.compile(r"[0-9a-f]+")


def _entropy_hits(text, taken, keyword_window=40):
    for m in CANDIDATE.finditer(text):
        if any(a <= m.start() < b for a, b in taken):
            continue
        s = m.group(0)
        if HEX.fullmatch(s):  # hashes, git ids: hex is the commonest false positive
            continue
        if not (re.search(r"[A-Z]", s) and re.search(r"[a-z]", s) and re.search(r"[0-9]", s)):
            continue
        if entropy(s) < 4.5:
            continue
        before = text[max(0, m.start() - keyword_window): m.start()]
        kw = bool(KEYWORD.search(before))
        yield ("high-entropy-near-keyword" if kw else "high-entropy", "entropy", m.start(), m.end(), s)


def fingerprint(value):
    return hashlib.sha256(value.encode()).hexdigest()[:12]


def mask(value):
    return value[:6] + "…" + f"({len(value)})"


def scan(text, entropy_mode="keyword", placeholders=frozenset()):
    """Yields dicts: rule, grade, start, end, fp. entropy_mode: off | keyword | all."""
    taken = []
    for name, grade, rx, check in RULES:
        if name == "private-key" and "PRIVATE KEY-----" not in text:
            continue
        for m in rx.finditer(text):
            v = m.group(0)
            if v in DOC_EXAMPLES or v in placeholders or SILO_MARK in v:
                kind = "silo-placeholder" if (v in placeholders or SILO_MARK in v) else "doc-example"
                yield {"rule": name, "grade": "ignored:" + kind, "start": m.start(), "end": m.end(), "fp": fingerprint(v)}
                taken.append((m.start(), m.end()))
                continue
            if check is not None and not check(m):
                if grade == "checked":
                    yield {"rule": name, "grade": "failed-check", "start": m.start(), "end": m.end(), "fp": fingerprint(v)}
                continue
            taken.append((m.start(), m.end()))
            yield {"rule": name, "grade": grade, "start": m.start(), "end": m.end(), "fp": fingerprint(v)}
    if entropy_mode != "off":
        for name, grade, a, b, s in _entropy_hits(text, taken):
            if entropy_mode == "keyword" and name != "high-entropy-near-keyword":
                continue
            yield {"rule": name, "grade": grade, "start": a, "end": b, "fp": fingerprint(s)}


# A prefilter: every rule but the entropy one starts with one of these literals. A boundary that
# runs only the prefilter on most bytes and the rules near a hit is how the per-request cost stays low.
PREFIX = re.compile(r"gh[pousr]_|github_pat_|npm_|PRIVATE KEY-----|eyJ|sk-ant-|T3BlbkFJ|_live_|_test_|xox[baprs]-|hooks\.slack|AKIA|ASIA|AIza|://[^\s/@]{1,64}:[^\s@/]{1,128}@")


def scan_fast(text, placeholders=frozenset()):
    """The exact rules only, run on a window around each prefix hit, so the whole text is read once
    by one pattern. Spans are offsets into text."""
    out, seen, last_end = [], set(), -1
    for m in PREFIX.finditer(text):
        if m.start() < last_end:
            continue
        a = max(0, m.start() - 120)
        b = min(len(text), m.end() + (12000 if m.group(0) == "PRIVATE KEY-----" else 400))
        last_end = b
        for h in scan(text[a:b], entropy_mode="off", placeholders=placeholders):
            h["start"] += a; h["end"] += a
            if (h["start"], h["rule"]) not in seen:
                seen.add((h["start"], h["rule"])); out.append(h)
    return out


if __name__ == "__main__":
    import sys
    data = open(sys.argv[1], encoding="utf-8", errors="replace").read()
    for h in scan(data):
        print(h["rule"], h["grade"], h["start"], h["fp"])
