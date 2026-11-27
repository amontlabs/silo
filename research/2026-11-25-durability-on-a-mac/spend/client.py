"""One step of the restart test, as a computer through the boundary (127.0.0.7, as edges.py).

usage: client.py until-refused N | one | long
  until-refused N  up to N calls, stopping at the first one Silo refuses
  one              a single call
  long             one long streamed call, read slowly; the test kills the boundary during it
Prints one JSON line per call: status, and Silo's sentence when it refused.
"""
import json, os, socket, ssl, sys, http.client

CTX = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE"))
H = {"x-api-key": "sk-ant-api03-script-fake", "anthropic-version": "2023-06-01", "content-type": "application/json"}


class Conn(http.client.HTTPSConnection):
    def connect(self):
        s = socket.create_connection(("127.0.0.7", 443), self.timeout)
        self.sock = CTX.wrap_socket(s, server_hostname=self.host)


def call(think, max_tokens=4000, stream=False):
    c = Conn("api.anthropic.com", 443, context=CTX, timeout=120)
    body = {"model": "claude-opus-5-5", "max_tokens": max_tokens, "stream": stream,
            "messages": [{"role": "user", "content": "lorem ipsum " * 300 + f" THINK={think}"}]}
    try:
        c.request("POST", "/v1/messages", json.dumps(body).encode(), H)
        r = c.getresponse(); got = r.read(); c.close()
    except Exception as e:
        return {"status": None, "error": type(e).__name__}
    s = got.decode(errors="replace"); i = s.find("Silo:")
    return {"status": r.status, "silo": s[i:s.find('"', i)] if i >= 0 else None}


what = sys.argv[1]
if what == "until-refused":
    for k in range(int(sys.argv[2])):
        r = call(2000); r["n"] = k; print(json.dumps(r), flush=True)
        if r["status"] != 200: break
elif what == "one":
    print(json.dumps(call(2000)), flush=True)
elif what == "long":
    print(json.dumps(call(60000, max_tokens=64000, stream=True)), flush=True)
