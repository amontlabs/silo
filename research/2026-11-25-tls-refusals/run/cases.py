"""Runs each client case in computer c1 of a run started by up.sh, one after the other, and prints
what the client said and the record lines the boundary wrote for it.

usage: cases.py <out> [case ...]          (as root; prints JSON lines, one per case)

A client "trusts" Silo when it has Silo's authority (in.sh gives every client the authority); it
"refuses" when its store is the system's alone. Every key is a fake one from #395's config.py.
"""
import json, os, subprocess, sys, time

R = os.path.dirname(os.path.abspath(__file__))
E = os.path.join(os.path.dirname(os.path.dirname(R)), "2026-11-25-boundary-end-to-end")
SILOREC = os.path.join(os.path.dirname(E), "2026-11-25-record-format", "proto", "zig-out", "bin", "silorec")
CODEX = os.environ.get("CODEX", "/root/tools/codex/node_modules/.bin/codex")
SYS = "/etc/ssl/certs/ca-certificates.crt"
# A store without Silo: every variable a client reads for its store points at the system's.
UNTRUSTED = ["env", "-u", "NODE_EXTRA_CA_CERTS", f"SSL_CERT_FILE={SYS}", f"CURL_CA_BUNDLE={SYS}", f"REQUESTS_CA_BUNDLE={SYS}", f"GIT_SSL_CAINFO={SYS}"]
URL = "https://api.anthropic.com/v1/models"
KEY = "sk-ant-api03-silo-placeholder-c1-0000"
OKEY = "sk-proj-silo-placeholder-c1-0000"

def py_raw(script):
    return ["python3", "-I", "-c", "import socket,time,ssl\ns=socket.create_connection(('127.0.0.1',3128))\ns.sendall(b'CONNECT api.anthropic.com:443 HTTP/1.1\\r\\nHost: api.anthropic.com\\r\\n\\r\\n')\nprint(s.recv(200).split(b'\\r\\n')[0].decode())\n" + script]

CLAUDE = ["bash", "-c", f"cd $HOME && ANTHROPIC_API_KEY={KEY} IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 timeout 120 claude -p 'Say hello.' --output-format json --max-turns 1 --model claude-opus-5-5"]
CODEX_RUN = lambda limit: ["bash", "-c", f"cd $HOME && mkdir -p cx && CODEX_HOME=$HOME/cx OPENAI_API_KEY={OKEY} timeout {limit} {CODEX} exec --json --skip-git-repo-check -m gpt-5.5 Say_hello. </dev/null"]

CASES = {
    # Programs that trust Silo: the boundary must record nothing that looks like a refusal.
    "curl-trusted": ["curl", "-sS", "-m", "10", URL, "-o", "/dev/null", "-w", "%{http_code}"],
    "claude-trusted": CLAUDE,
    "codex-trusted": CODEX_RUN(60),
    "python-trusted": ["python3", "-I", "-c", f"import urllib.request as u;r=u.urlopen(u.Request('{URL}'));print(r.status)"],
    "go-trusted": ["/root/e2e/goget", URL],
    "git-trusted": ["bash", "-c", "GIT_SSL_CAINFO=$SSL_CERT_FILE git ls-remote https://api.anthropic.com/x.git"],
    # The same programs without Silo's authority.
    "curl-untrusted": UNTRUSTED + ["curl", "-sS", "-m", "10", URL, "-o", "/dev/null", "-w", "%{http_code}"],
    "python-untrusted": UNTRUSTED + ["python3", "-I", "-c", f"import urllib.request as u;r=u.urlopen(u.Request('{URL}'));print(r.status)"],
    "node-untrusted": UNTRUSTED + ["node", "-e", f"fetch('{URL}').then(r=>console.log(r.status),e=>{{console.log(String(e.cause||e));process.exit(1)}})"],
    "go-untrusted": UNTRUSTED + ["/root/e2e/goget", URL],
    "git-untrusted": UNTRUSTED + ["git", "ls-remote", "https://api.anthropic.com/x.git"],
    "claude-untrusted": UNTRUSTED + CLAUDE,
    "codex-untrusted": UNTRUSTED + CODEX_RUN(60),
    "codex-untrusted-long": UNTRUSTED + CODEX_RUN(int(os.environ.get("CASE_LIMIT", "300"))),
    # Failures that are not a refused certificate, and must not be shown as one.
    "nothing-sent": py_raw("time.sleep(12)"),
    "closed-at-once": py_raw("s.close()"),
    "not-tls": py_raw("s.sendall(b'GET / HTTP/1.1\\r\\nHost: x\\r\\n\\r\\n');time.sleep(1)"),
    "no-common-cipher": ["curl", "-sS", "-m", "10", "--tls-max", "1.2", "--ciphers", "ECDHE-RSA-AES128-GCM-SHA256", URL, "-o", "/dev/null", "-w", "%{http_code}"],
    "old-tls-only": ["curl", "-sS", "-m", "10", "--tlsv1.0", "--tls-max", "1.1", "--ciphers", "DEFAULT@SECLEVEL=0", URL, "-o", "/dev/null", "-w", "%{http_code}"],
    "name-differs": ["bash", "-c", "openssl s_client -proxy 127.0.0.1:3128 -connect api.anthropic.com:443 -servername storage.googleapis.com </dev/null 2>&1 | grep -m1 -E 'alert|CONNECTED|error' "],
    "pinned-key": ["curl", "-sS", "-m", "10", "--pinnedpubkey", "sha256//AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", URL, "-o", "/dev/null", "-w", "%{http_code}"],
    "stalls-mid-handshake": py_raw("import ssl\nh=bytes.fromhex('16030100')\ns.sendall(h)\ntime.sleep(12)"),
}

def record_lines(out):
    p = subprocess.run([SILOREC, "print", os.path.join(out, "record", "c1")], capture_output=True, text=True)
    return [json.loads(l) for l in p.stdout.splitlines() if l.strip()]

def main():
    out = sys.argv[1]
    names = sys.argv[2:] or list(CASES)
    for name in names:
        before = len(record_lines(out))
        t0 = time.time()
        p = subprocess.run([os.path.join(E, "run", "in.sh"), out, "c1"] + CASES[name], capture_output=True, text=True)
        secs = round(time.time() - t0, 1)
        time.sleep(1.5)  # the boundary writes its close, the recorder acknowledges
        lines = record_lines(out)[before:]
        said = (p.stdout.strip().splitlines() or [""])[-1][:160] + " | " + (p.stderr.strip().splitlines() or [""])[-1][:160]
        conn = [l for l in lines if l.get("kind") == "connection"]
        groups = {}
        for l in conn:
            if l.get("what") != "closed": continue
            k = json.dumps({k: l[k] for k in l if k in ("why", "alert", "tls_client", "agent", "sni")}, sort_keys=True)
            groups[k] = groups.get(k, 0) + 1
        closed = [dict(json.loads(k), n=n) for k, n in groups.items()]
        print(json.dumps({"case": name, "exit": p.returncode, "s": secs, "said": said, "connections": sum(1 for l in conn if l.get("what") == "opened"), "closed": closed}), flush=True)

if __name__ == "__main__":
    main()
