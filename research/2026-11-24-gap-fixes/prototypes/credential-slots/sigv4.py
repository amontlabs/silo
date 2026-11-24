"""AWS Signature Version 4: compute, verify and re-sign a request, its aws-chunked body and a
presigned URL. Written from the IAM user guide ("Create a signed AWS API request") and the S3
chunked-upload page. Used by the boundary (re-sign) and by the fake S3 (verify)."""
import hashlib, hmac, re
from urllib.parse import unquote

EMPTY_SHA = hashlib.sha256(b"").hexdigest()
UNRESERVED = re.compile(rb"[A-Za-z0-9\-._~]")


def uri_encode(s, keep_slash):
    out = []
    for b in s.encode():
        c = bytes([b])
        if UNRESERVED.match(c) or (keep_slash and c == b"/"):
            out.append(c.decode())
        else:
            out.append("%%%02X" % b)
    return "".join(out)


def canonical_uri(path, service):
    # S3 does not normalise and encodes once; other services encode twice. The path arrives encoded.
    raw = unquote(path) if service == "s3" else path
    enc = uri_encode(raw, keep_slash=True)
    if service != "s3":
        enc = uri_encode(enc, keep_slash=True)
    return enc or "/"


def canonical_query(query, drop=("X-Amz-Signature",)):
    pairs = []
    if query:
        for part in query.split("&"):
            if not part:
                continue
            k, _, v = part.partition("=")
            k, v = unquote(k.replace("+", " ")), unquote(v.replace("+", " "))
            if k in drop:
                continue
            pairs.append((uri_encode(k, False), uri_encode(v, False)))
    pairs.sort()
    return "&".join(f"{k}={v}" for k, v in pairs)


def signing_key(secret, date, region, service):
    k = hmac.new(("AWS4" + secret).encode(), date.encode(), hashlib.sha256).digest()
    for part in (region, service, "aws4_request"):
        k = hmac.new(k, part.encode(), hashlib.sha256).digest()
    return k


def parse_auth(value):
    """'AWS4-HMAC-SHA256 Credential=AK/20261124/eu-west-3/s3/aws4_request, SignedHeaders=a;b, Signature=hex'"""
    m = re.fullmatch(r"AWS4-HMAC-SHA256 +Credential=([^,]+), *SignedHeaders=([^,]+), *Signature=([0-9a-f]{64})", value.strip())
    if not m:
        return None
    akid, date, region, service, term = m.group(1).split("/")
    return {"akid": akid, "date": date, "region": region, "service": service,
            "signed": m.group(2).split(";"), "sig": m.group(3)}


def string_to_sign(method, path, query, headers, signed, payload_hash, amz_date, scope):
    """headers: list of (name, value) as sent. signed: lowercase names."""
    hv = {}
    for n, v in headers:
        hv.setdefault(n.lower(), []).append(" ".join(v.strip().split()))
    ch = "".join(f"{n}:{','.join(hv.get(n, ['']))}\n" for n in signed)
    service = scope.split("/")[2]
    creq = "\n".join([method, canonical_uri(path, service), canonical_query(query), ch, ";".join(signed), payload_hash])
    return "AWS4-HMAC-SHA256\n" + amz_date + "\n" + scope + "\n" + hashlib.sha256(creq.encode()).hexdigest()


def sign(key, sts):
    return hmac.new(key, sts.encode(), hashlib.sha256).hexdigest()


def header_signature(secret, method, path, query, headers, auth):
    hv = {n.lower(): v for n, v in headers}
    amz_date = hv.get("x-amz-date", "")
    scope = f"{auth['date']}/{auth['region']}/{auth['service']}/aws4_request"
    payload = hv.get("x-amz-content-sha256", EMPTY_SHA)
    sts = string_to_sign(method, path, query, headers, auth["signed"], payload, amz_date, scope)
    key = signing_key(secret, auth["date"], auth["region"], auth["service"])
    return sign(key, sts), key, amz_date, scope


def query_signature(secret, method, path, query, headers):
    """A presigned URL: X-Amz-Credential, X-Amz-Date, X-Amz-SignedHeaders, X-Amz-Signature in the query."""
    q = {}
    for part in query.split("&"):
        k, _, v = part.partition("=")
        q[unquote(k)] = unquote(v)
    akid, date, region, service, _ = q["X-Amz-Credential"].split("/")
    scope = f"{date}/{region}/{service}/aws4_request"
    signed = q["X-Amz-SignedHeaders"].split(";")
    sts = string_to_sign(method, path, query, headers, signed, "UNSIGNED-PAYLOAD", q["X-Amz-Date"], scope)
    return sign(signing_key(secret, date, region, service), sts), q


# aws-chunked bodies -------------------------------------------------------------------------

def chunk_sts(amz_date, scope, prev, data):
    return "\n".join(["AWS4-HMAC-SHA256-PAYLOAD", amz_date, scope, prev, EMPTY_SHA, hashlib.sha256(data).hexdigest()])


def trailer_sts(amz_date, scope, prev, trailer_bytes):
    return "\n".join(["AWS4-HMAC-SHA256-TRAILER", amz_date, scope, prev, hashlib.sha256(trailer_bytes).hexdigest()])


def resign_aws_chunked(body, amz_date, scope, old_key, old_seed, new_key, new_seed):
    """Walks an aws-chunked body. Verifies each chunk-signature against the old chain, replaces it with
    the new chain's. Every byte but the 64 hex digits of each signature stays as it came. Returns
    (new_body, chunks, all_old_valid)."""
    out = bytearray(); i = 0; prev_old, prev_new = old_seed, new_seed; ok = True; n = 0
    while True:
        eol = body.index(b"\r\n", i)
        line = body[i:eol]
        m = re.fullmatch(rb"([0-9a-fA-F]+);chunk-signature=([0-9a-f]{64})", line)
        if not m:
            raise ValueError("bad aws-chunked line %r" % line[:80])
        size = int(m.group(1), 16)
        data = body[eol + 2:eol + 2 + size]
        want_old = sign(old_key, chunk_sts(amz_date, scope, prev_old, data))
        ok &= hmac.compare_digest(want_old, m.group(2).decode())
        new_sig = sign(new_key, chunk_sts(amz_date, scope, prev_new, data))
        out += m.group(1) + b";chunk-signature=" + new_sig.encode() + b"\r\n" + data
        prev_old, prev_new = m.group(2).decode(), new_sig
        i = eol + 2 + size; n += 1
        if size == 0:
            rest = body[i:]
            if rest == b"\r\n" or rest == b"":
                out += rest
                return bytes(out), n, ok
            # trailers: name:value\r\n ... x-amz-trailer-signature:hex\r\n\r\n
            tr_lines = rest.split(b"\r\n")
            kept = [l for l in tr_lines if l and not l.lower().startswith(b"x-amz-trailer-signature:")]
            tsig = [l for l in tr_lines if l.lower().startswith(b"x-amz-trailer-signature:")]
            tbytes = b"".join(l + b"\n" for l in kept)
            if tsig:
                ok &= hmac.compare_digest(sign(old_key, trailer_sts(amz_date, scope, prev_old, tbytes)), tsig[0].split(b":", 1)[1].strip().decode())
                new_t = sign(new_key, trailer_sts(amz_date, scope, prev_new, tbytes))
                out += b"".join(l + b"\r\n" for l in kept) + b"x-amz-trailer-signature:" + new_t.encode() + b"\r\n\r\n"
            else:
                out += rest
            return bytes(out), n, ok
        if body[i:i + 2] != b"\r\n":
            raise ValueError("chunk not followed by CRLF")
        out += b"\r\n"; i += 2


# Signature Version 2 presigned URLs (what boto3 still makes for S3 by default) -----------------

def v2_query_signature(secret, method, path, query):
    """?AWSAccessKeyId=..&Signature=..&Expires=..; HMAC-SHA1 over verb, MD5, type, expiry, resource."""
    import base64
    q = {}
    for part in query.split("&"):
        k, _, v = part.partition("=")
        q[unquote(k)] = unquote(v)
    sts = f"{method}\n\n\n{q['Expires']}\n{path}"
    return base64.b64encode(hmac.new(secret.encode(), sts.encode(), hashlib.sha1).digest()).decode(), q
