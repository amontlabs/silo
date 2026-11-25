"""Three redactors, each returning the spans of a text it would replace.

- `Exact`: #386's redactor (`proto/src/redact.zig` of research/2026-11-25-record-format), ported
  line for line: each known value as it is, the longest one that starts at a place wins, values
  under 8 bytes refused.
- `Forms`: knows the same values, and also looks for the forms a value takes on the way to the
  record (JSON escaped once or twice, Go's escapes, percent-encoded, base64 at its three
  alignments, hex, shell-quoted). It anchors on windows of 16 characters of each form and extends a
  match across line breaks and escaped line breaks, so a value cut, rewrapped or split in two is
  still taken.
- `Shapes`: knows no value. It finds what looks like a secret by its format, for the values Silo was
  never given.
"""
import base64, collections, math, re, urllib.parse

from corpus import json_escaped, go_escaped, shell_quoted

MIN = 8
WIN = 16
STRIDE = 8


class Exact:
    def __init__(self, values):
        for v in values:
            if len(v.encode()) < MIN:
                raise ValueError("SecretTooShort")
        self.values = values
        self.starts = {v[:2] for v in values}

    def spans(self, text):
        out, i = [], 0
        while i < len(text):
            if text[i:i + 2] not in self.starts:
                i += 1
                continue
            best = None
            for v in self.values:
                if text.startswith(v, i) and (best is None or len(v) > len(best)):
                    best = v
            if best:
                out.append((i, i + len(best)))
                i += len(best)
            else:
                i += 1
        return out


def forms(v):
    """Every form a value takes on its way to the record, as the strings to look for."""
    out = {v, json_escaped(v), json_escaped(json_escaped(v)), go_escaped(v), shell_quoted(v)}
    for safe in ("", "/"):
        q = urllib.parse.quote(v, safe=safe)
        out |= {q, q.lower() if "%" in q else q, urllib.parse.quote_plus(v, safe=safe)}
    out |= {v.encode().hex(), v.encode().hex().upper()}
    for enc in (base64.b64encode, base64.urlsafe_b64encode):
        for a in range(3):
            whole = enc(b"\0" * a + v.encode()).decode().rstrip("=")
            start = -(-a * 4 // 3)
            end = (a + len(v.encode())) * 4 // 3
            out.add(whole[start:end])
    # A value of several lines is also looked for line by line and as one run without line breaks.
    if "\n" in v:
        lines = [l for l in v.splitlines() if l]
        out |= set(lines)
        out.add("".join(l for l in lines if not l.startswith("-----")))
    return {f for f in out if len(f) >= MIN}


# What may sit between two characters of a value when the record receives it cut or rewrapped.
BREAK = re.compile(r"(?:\r?\n|\\r\\n|\\n|\\\\n| )")


class Forms:
    def __init__(self, values):
        for v in values:
            if len(v.encode()) < MIN:
                raise ValueError("SecretTooShort")
        self.anchors = {}  # first WIN characters of a window -> [(form, offset in form)]
        self.short = []
        for v in values:
            for f in forms(v):
                if len(f) < WIN:
                    self.short.append(f)
                    continue
                for off in list(range(0, len(f) - WIN + 1, STRIDE)) + [len(f) - WIN]:
                    self.anchors.setdefault(f[off:off + WIN], []).append((f, off))
        self.exact = Exact(self.short) if self.short else None

    def extend(self, text, i, f, off):
        """From a window of `f` at text[i:], the span its whole form covers, across line breaks."""
        # Forward.
        j, k = i + WIN, off + WIN
        while k < len(f) and j < len(text):
            if text[j] == f[k]:
                j, k = j + 1, k + 1
                continue
            m = BREAK.match(text, j)
            if m and f[k] not in "\r\n ":
                j = m.end()
                continue
            break
        end = j
        # Backward.
        j, k = i, off
        while k > 0 and j > 0:
            if text[j - 1] == f[k - 1]:
                j, k = j - 1, k - 1
                continue
            back = None
            for sep in ("\r\n", "\n", "\\r\\n", "\\\\n", "\\n", " "):
                if text.endswith(sep, 0, j):
                    back = j - len(sep)
                    break
            if back is not None and f[k - 1] not in "\r\n ":
                j = back
                continue
            break
        return j, end

    def spans(self, text):
        out = []
        i = 0
        while i <= len(text) - WIN:
            hits = self.anchors.get(text[i:i + WIN])
            if not hits:
                i += 1
                continue
            s, e = min((self.extend(text, i, f, off) for f, off in hits), key=lambda se: (se[0], -se[1]))
            out.append((s, e))
            i = max(e, i + 1)
        if self.exact:
            out += self.exact.spans(text)
        return merge(out)


def merge(spans):
    out = []
    for s, e in sorted(spans):
        if out and s <= out[-1][1]:
            out[-1] = (out[-1][0], max(out[-1][1], e))
        else:
            out.append((s, e))
    return out


# Formats with a fixed prefix or structure. Each pattern's group 1, when it has one, is what is
# removed; otherwise the whole match.
RULES = [
    ("aws-access-key-id", r"\b((?:AKIA|ASIA|ABIA|ACCA)[A-Z2-7]{16})\b"),
    ("aws-secret-access-key", r"(?i)aws.{0,24}?(?:secret|private).{0,24}?[\"'=:\s]+([A-Za-z0-9/+]{40})(?![A-Za-z0-9/+])"),
    ("github", r"\b((?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{82})\b"),
    ("gitlab", r"\b(glpat-[A-Za-z0-9_-]{20})"),
    ("stripe", r"\b((?:sk|rk)_(?:live|test)_[A-Za-z0-9]{24,})"),
    ("anthropic", r"\b(sk-ant-[a-z]{3}\d\d-[A-Za-z0-9_-]{80,})"),
    ("openai", r"\b(sk-(?:proj|svcacct|admin)-[A-Za-z0-9_-]{40,}|sk-[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20})"),
    ("slack", r"\b(xox[abposr]-[0-9A-Za-z-]{10,})"),
    ("google-api", r"\b(AIza[0-9A-Za-z_-]{35})"),
    ("npm", r"\b(npm_[A-Za-z0-9]{36})"),
    ("sendgrid", r"\b(SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43})"),
    ("jwt", r"\b(eyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{16,})"),
    # A private key, whole or cut, with real or escaped line breaks.
    ("private-key", r"(-----BEGIN [A-Z ]{0,24}PRIVATE KEY-----(?:[A-Za-z0-9+/=\s]|\\r|\\n|\\\\n){16,}?(?:-----END [A-Z ]{0,24}PRIVATE KEY-----|$|(?=[^A-Za-z0-9+/=\s\\])))"),
    ("url-password", r"\b[a-z][a-z0-9+.-]{1,20}://[^\s:/@\"']{1,64}:([^\s@/\"']{3,128})@"),
    ("azure-account-key", r"(?i)AccountKey=([A-Za-z0-9+/=]{40,})"),
    ("basic-authorization", r"(?i)Authorization:\s*Basic\s+([A-Za-z0-9+/=]{8,})"),
    ("bearer-authorization", r"(?i)Authorization:\s*Bearer\s+([A-Za-z0-9._~+/=-]{16,})"),
    ("assigned-secret", r"(?i)(?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key)[\"']?\s*[:=]\s*[\"']?([^\s\"'&,;]{8,})"),
]
COMPILED = [(n, re.compile(p)) for n, p in RULES]

# Rules that read a name and not a format fire on code (`let password = read()`). Their value must
# also look random: letters and digits, no call or template syntax, and enough entropy.
GATED = {"assigned-secret", "url-password", "basic-authorization", "bearer-authorization"}


def looks_random(v):
    if len(v) < 8 or any(c in v for c in "()[]{}$<>`") or v.endswith(":"):
        return False
    if not (any(c.isdigit() for c in v) and any(c.isalpha() for c in v)):
        return False
    counts = collections.Counter(v)
    h = -sum(n / len(v) * math.log2(n / len(v)) for n in counts.values())
    return h >= 3.0


class Shapes:
    def __init__(self, rules=COMPILED, decode=False, gate=True):
        self.rules = rules
        self.decode = decode
        self.gate = gate

    def keep(self, name, text, span):
        return not (self.gate and name in GATED) or looks_random(text[span[0]:span[1]])

    def spans(self, text):
        out = []
        for name, rx in self.rules:
            for m in rx.finditer(text):
                sp = m.span(1 if rx.groups else 0)
                if self.keep(name, text, sp):
                    out.append(sp)
        if self.decode:
            out += self.decoded(text)
        return merge(out)

    def spans_of(self, text):
        return [sp for name, rx in self.rules for m in rx.finditer(text)
                if self.keep(name, text, sp := m.span(1 if rx.groups else 0))]

    def decoded(self, text):
        """Runs of base64 or percent-encoding are decoded and looked at again."""
        out = []
        for m in re.finditer(r"[A-Za-z0-9+/]{20,}={0,2}", text):
            try:
                d = base64.b64decode(m.group() + "=" * (-len(m.group()) % 4)).decode("utf-8")
            except Exception:
                continue
            if self.spans_of(d):
                out.append(m.span())
        for m in re.finditer(r"(?:[^\s%]*%[0-9A-Fa-f]{2}){2,}[^\s%&]*", text):
            d = urllib.parse.unquote(m.group())
            if d != m.group() and self.spans_of(d):
                out.append(m.span())
        return out

    def hits(self, text):
        """(rule, start, end) of each match, for counting where a detector fires on clean text."""
        out = []
        for name, rx in self.rules:
            for m in rx.finditer(text):
                sp = m.span(1 if rx.groups else 0)
                if self.keep(name, text, sp):
                    out.append((name, *sp))
        return out
