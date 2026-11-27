"""Each case, sent to four HTTP parsers and, for JSON bodies, read by three JSON parsers. A case is
ambiguous when two of them read it differently; the strict reader must refuse every such case and
read every ordinary one. Needs parsers/ running (run.sh starts them).
usage: diff.py <strictcheck binary>"""
import glob, json, os, re, socket, subprocess, sys, tempfile
P = {"node": 9101, "go": 9102, "h11": 9103, "lenient": 9104}
def http_view(port, data):
    s = socket.create_connection(("127.0.0.1", port)); s.sendall(data); s.settimeout(1.0); got = b""
    try:
        while True:
            d = s.recv(65536)
            if not d: break
            got += d
    except socket.timeout: pass
    s.close()
    out = [json.loads(m) for m in re.findall(rb'\{"parsed": ?\{[^}]*\}\}', got)]
    if not out: return "refused" if got.startswith(b"HTTP/1.1 4") or got.startswith(b"HTTP/1.0 4") or not got else "?"
    return " | ".join(f"{p['parsed']['method']} {p['parsed']['target']} host={p['parsed']['host']} body={p['parsed']['len']}:{p['parsed']['sha'][:6]}" for p in out)
H = os.path.dirname(os.path.abspath(__file__))
bodies = json.load(open(f"{H}/cases/json-bodies.json"))
def json_views(body):
    f = tempfile.NamedTemporaryFile(delete=False); f.write(body.encode("latin-1")); f.close()
    v = {"python": subprocess.run(["python3", "-I", f"{H}/parsers/json_python.py", f.name], capture_output=True, text=True).stdout.strip(),
         "node": subprocess.run(["node", f"{H}/parsers/json_node.js", f.name], capture_output=True, text=True).stdout.strip(),
         "go": subprocess.run([os.environ.get("JSON_GO", "json_go"), f.name], capture_output=True, text=True).stdout.strip()}
    os.unlink(f.name); return {k: json.loads(x) for k, x in v.items()}
strict_out = subprocess.run([sys.argv[1], "api.anthropic.com"] + sorted(glob.glob(f"{H}/cases/*.bin")), capture_output=True, text=True).stdout
verdicts = {}
for l in strict_out.splitlines():
    r = json.loads(l); k = r["file"][:-4]
    verdicts[k] = r["why"] if r["verdict"] == "refused" else verdicts.get(k, r["verdict"])
bad = 0; amb = 0
for f in sorted(glob.glob(f"{H}/cases/*.bin")):
    k = os.path.basename(f)[:-4]; data = open(f, "rb").read()
    hv = {p: http_view(port, data) for p, port in P.items()}
    jv = json_views(bodies[k]) if k in bodies else None
    seen = set(hv.values()) | ({json.dumps(v, sort_keys=True) for v in jv.values()} if jv else set())
    ambiguous = len(set(hv.values())) > 1 or (jv is not None and len({json.dumps(v, sort_keys=True) for v in jv.values()}) > 1) or any("attacker" in v for v in hv.values()) or (jv and any(v["web_fetch"] for v in jv.values()))
    ordinary = k.startswith("0")
    ok = (verdicts[k] in ("read", "unread")) if ordinary else (verdicts[k] not in ("read", "unread"))
    bad += not ok; amb += bool(ambiguous) and not ordinary
    print(f"== {k}   strict: {verdicts[k]}{'' if ok else '   <-- WRONG'}")
    for p, v in hv.items(): print(f"   {p:8} {v}")
    if jv:
        for p, v in jv.items(): print(f"   json/{p:6} {'parsed' if v['ok'] else 'refused'}, web_fetch={v['web_fetch']}, max_tokens={v['max_tokens']}")
print(f"\n{len(verdicts)} cases; {amb} hostile cases read in more than one way, or carrying the hidden request, by at least one parser; strict reader wrong on {bad}")
