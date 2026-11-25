"""A corpus of secret formats and the places they appear, generated at run time.

Every value is random and made when the script runs; no value is ever written to this repository.
Each format is shaped like the real one (prefix, alphabet, length, line wrapping), so a redactor or a
detector sees what it would see on a real night. None of them is valid at any service.

A place takes a value and returns a `Sample`: the text the recorder would receive, and the spans of
that text that carry the value. A redactor is scored on how many of those characters it removes.
"""
import base64, json, random, string, urllib.parse
from dataclasses import dataclass, field

ALNUM = string.ascii_letters + string.digits
URLSAFE = ALNUM + "-_"
B64 = ALNUM + "+/"
B32 = string.ascii_uppercase + "234567"


def pick(rng, alphabet, n):
    return "".join(rng.choice(alphabet) for _ in range(n))


def b64url(b):
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def pem(rng, label, nbytes, width):
    body = base64.b64encode(rng.randbytes(nbytes)).decode()
    lines = [body[i:i + width] for i in range(0, len(body), width)]
    return f"-----BEGIN {label}-----\n" + "\n".join(lines) + f"\n-----END {label}-----\n"


def jwt(rng):
    head = b64url(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    body = b64url(json.dumps({"sub": pick(rng, string.digits, 10), "name": "dev", "iat": 1764000000 + rng.randrange(10**6)}).encode())
    return f"{head}.{body}.{b64url(rng.randbytes(32))}"


def password(rng):
    # Database and service passwords carry the characters that every encoding changes.
    return pick(rng, ALNUM, 10) + rng.choice("@:/%#?&") + pick(rng, ALNUM, 4) + rng.choice("$\"'\\+= ") + pick(rng, ALNUM, 4)


@dataclass
class Format:
    name: str
    make: callable
    # What the detector should call it; None when no pattern can know it without context.
    shaped: bool = True


FORMATS = [
    Format("aws-access-key-id", lambda r: "AKIA" + pick(r, B32, 16)),
    Format("aws-secret-access-key", lambda r: pick(r, B64, 40), shaped=False),
    Format("github-classic", lambda r: "ghp_" + pick(r, ALNUM, 36)),
    Format("github-fine-grained", lambda r: "github_pat_" + pick(r, ALNUM, 22) + "_" + pick(r, ALNUM, 59)),
    Format("gitlab", lambda r: "glpat-" + pick(r, URLSAFE, 20)),
    Format("stripe-live", lambda r: "sk_live_" + pick(r, ALNUM, 99)),
    Format("openai-project", lambda r: "sk-proj-" + pick(r, URLSAFE, 74) + "T3BlbkFJ" + pick(r, URLSAFE, 74)),
    Format("anthropic", lambda r: "sk-ant-api03-" + pick(r, URLSAFE, 93) + "AA"),
    Format("slack-bot", lambda r: f"xoxb-{pick(r, string.digits, 11)}-{pick(r, string.digits, 13)}-{pick(r, ALNUM, 24)}"),
    Format("google-api", lambda r: "AIza" + pick(r, URLSAFE, 35)),
    Format("npm", lambda r: "npm_" + pick(r, ALNUM, 36)),
    Format("sendgrid", lambda r: "SG." + pick(r, URLSAFE, 22) + "." + pick(r, URLSAFE, 43)),
    Format("jwt", jwt),
    Format("azure-storage-key", lambda r: base64.b64encode(r.randbytes(64)).decode()),
    Format("database-password", password, shaped=False),
    Format("ssh-ed25519-private", lambda r: pem(r, "OPENSSH PRIVATE KEY", 399, 70)),
    Format("rsa-private-pkcs8", lambda r: pem(r, "PRIVATE KEY", 1218, 64)),
]


@dataclass
class Sample:
    """Strings the recorder receives, in order; spans index the string they are in."""
    texts: list
    spans: list = field(default_factory=list)  # (text index, start, end)
    # When pieces are carried in separate deltas, a reader joins them: score them together.
    joined: bool = False

    def __post_init__(self):
        self.spans = [s if len(s) == 3 else (0, *s) for s in self.spans]


def embed(before, carried, after, extra=None):
    """One string with `carried` in it; `extra` adds spans inside `carried` instead of the whole."""
    text = before + carried + after
    if extra is None:
        return Sample([text], [(len(before), len(before) + len(carried))])
    return Sample([text], [(len(before) + s, len(before) + e) for s, e in extra])


def find_all(hay, needle):
    out, i = [], hay.find(needle)
    while i >= 0:
        out.append((i, i + len(needle)))
        i = hay.find(needle, i + 1)
    return out


def body_lines(v):
    """The lines of a value that carry it: all of them but a PEM's armour."""
    return [l for l in v.splitlines() if l and not l.startswith("-----")]


def spans_of_lines(text, v):
    out = []
    for l in body_lines(v) or [v]:
        out += find_all(text, l)
    return out


def json_escaped(v):
    return json.dumps(v)[1:-1]


def go_escaped(v):
    # Go's encoding/json escapes <, > and & by default, and so do many web frameworks.
    return json_escaped(v).replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")


def shell_quoted(v):
    # What `printf %q` and `set -x` traces print for a value with special characters.
    return "".join("\\" + c if c in " \"'\\$&#?=" else c for c in v)


def b64_core(prefix, v, suffix, enc=base64.b64encode):
    """Base64 of a value inside other bytes, and the characters of it that depend on the value alone."""
    whole = enc((prefix + v + suffix).encode()).decode()
    start = -(-len(prefix.encode()) * 4 // 3)
    end = (len(prefix.encode()) + len(v.encode())) * 4 // 3
    return whole, start, end


def place_prompt(v, rng):
    return embed("I lost access, here is the key I use in prod: ", v, " can you check why it fails?")


def place_env(v, rng):
    return embed("HOME=/root\nPATH=/usr/bin:/bin\nAPP_SECRET=", v, "\nLANG=C.UTF-8\n")


def place_file_in_request(v, rng):
    # A file's content read by a tool, inside the model request body the record keeps as a blob.
    content = f"# config\nsecret = \"{v}\"\nregion = eu-west-3\n"
    body = json.dumps({"model": "m", "messages": [{"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": content}]}]})
    return Sample([body], spans_of_lines(body, json_escaped(v).replace("\\n", "\n")) if "\n" in v else find_all(body, json_escaped(v)))


def place_go_json(v, rng):
    body = '{"output":"' + go_escaped(f"value={v}&next=1") + '"}'
    enc = go_escaped(v)
    return Sample([body], find_all(body, enc) if "\\n" not in enc else [s for l in enc.split("\\n") if len(l) > 4 and not l.startswith("-----") for s in find_all(body, l)])


def place_tool_arguments(v, rng):
    # Chat Completions carries tool arguments as a JSON string inside JSON: the value is escaped twice.
    cmd = f"curl -s -H 'Authorization: Bearer {v}' https://api.example.com/v1/me"
    body = json.dumps({"tool_calls": [{"function": {"name": "bash", "arguments": json.dumps({"command": cmd})}}]})
    enc = json_escaped(json_escaped(v))
    if "\\\\n" in enc:
        return Sample([body], [s for l in enc.split("\\\\n") if len(l) > 4 and not l.startswith("-----") for s in find_all(body, l)])
    return Sample([body], find_all(body, enc))


def place_url_query(v, rng):
    q = urllib.parse.quote(v, safe="")
    return embed("GET https://api.example.com/v1/items?limit=5&api_key=", q, "&page=2")


def place_url_userinfo(v, rng):
    q = urllib.parse.quote(v, safe="")
    return embed("DATABASE_URL=postgres://app:", q, "@db.internal:5432/app?sslmode=require\n")


def place_basic_auth(v, rng):
    whole, s, e = b64_core("app:", v, "")
    return embed("GET /repo.git/info/refs HTTP/1.1\nAuthorization: Basic ", whole, "\n", extra=[(s, e)])


def place_base64_file(v, rng):
    # `kubectl get secret -o yaml` and many config stores show values in base64, inside a whole file.
    prefix = "DEBUG=0\nTOKEN" + "_" * rng.randrange(3) + "="
    whole, s, e = b64_core(prefix, v, "\nPORT=8080\n")
    return embed("apiVersion: v1\nkind: Secret\ndata:\n  env: ", whole, "\n", extra=[(s, e)])


def place_hex(v, rng):
    h = v.encode().hex()
    return embed("$ xxd -p .env.key\n", h, "\n")


def place_shell_trace(v, rng):
    q = shell_quoted(v)
    if "\n" in v:
        q = "$'" + v.replace("\n", "\\n") + "'"
        return Sample([f"+ export KEY={q}\n"], [s for l in body_lines(v) for s in find_all(f"+ export KEY={q}\n", l)])
    return embed("+ export KEY=", q, "\n")


def place_split(v, rng):
    # The same output read in two pieces, written as two lines of the record.
    cut = rng.randrange(1, len(v))
    a, b = "output: " + v[:cut], v[cut:] + " (end)"
    return Sample([a, b], [(0, 8, len(a)), (1, 0, len(v) - cut)])


def place_crlf(v, rng):
    w = v.replace("\n", "\r\n")
    t = "type key.pem\r\n" + w
    return Sample([t], spans_of_lines(t, v))


def place_rewrapped(v, rng):
    # The same bytes wrapped at another width: `base64` wraps at 76, a terminal at its width.
    flat = "".join(body_lines(v)) if "\n" in v else v
    lines = [flat[i:i + 76] for i in range(0, len(flat), 76)]
    t = "$ base64 < key.der\n" + "\n".join(lines) + "\n"
    return Sample([t], [s for l in lines for s in find_all(t, l)])


def place_screen(v, rng):
    # What a screen of 80 columns shows: a long value is cut where the row ends.
    t = "$ echo $TOKEN\n" + v.replace("\n", " ")
    rows = [t[i:i + 80] for i in range(0, len(t), 80)]
    text = "\n".join(rows)
    flat = v.replace("\n", " ")
    out, pos = [], text.find(flat[:8])
    # Every character of the value is still on the screen, one row break apart.
    k = 0
    for i, c in enumerate(text):
        if i >= pos and k < len(flat) and c == flat[k]:
            out.append((i, i + 1))
            k += 1
    return Sample([text], [s for s in out if text[s[0]] != " "])


def place_stream(v, rng):
    # A model's reply, raw as it is streamed: the text arrives in deltas of a few characters.
    pieces, i = [], 0
    while i < len(v):
        n = rng.randrange(3, 13)
        pieces.append(v[i:i + n])
        i += n
    out, spans = "", []
    for p in pieces:
        pre = 'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"'
        enc = json_escaped(p)
        spans.append((len(out) + len(pre), len(out) + len(pre) + len(enc)))
        out += pre + enc + '"}}\n\n'
    return Sample([out], spans, joined=True)


PLACES = [
    ("prompt", place_prompt),
    ("tool output", place_env),
    ("file in a request body", place_file_in_request),
    ("JSON escaped the Go way", place_go_json),
    ("tool arguments (JSON in JSON)", place_tool_arguments),
    ("URL query, percent-encoded", place_url_query),
    ("URL password, percent-encoded", place_url_userinfo),
    ("Basic authorization", place_basic_auth),
    ("base64 inside a file", place_base64_file),
    ("hex", place_hex),
    ("shell trace", place_shell_trace),
    ("split across two lines", place_split),
    ("CRLF line ends", place_crlf),
    ("rewrapped at 76", place_rewrapped),
    ("screen of 80 columns", place_screen),
    ("raw model stream", place_stream),
]


def corpus(seed, per=3):
    rng = random.Random(seed)
    for f in FORMATS:
        for _ in range(per):
            v = f.make(rng)
            for pname, place in PLACES:
                yield f, v, pname, place(v, rng)
