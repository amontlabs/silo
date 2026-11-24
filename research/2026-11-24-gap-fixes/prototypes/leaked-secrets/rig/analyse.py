"""What a boundary detector costs on real Claude Code traffic, and where the planted credentials went.

Run: python3 analyse.py <out of run_cc.sh>
Prints: per request, the body size and how much of its conversation text is new; the re-send
factor; scan time for whole bodies against new messages only; where each credential was found;
and whether Claude Code's own transcript on disk holds the values.
"""
import glob, hashlib, json, os, sys, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from detector import scan_fast, fingerprint

out = sys.argv[1]
reqs = [json.loads(l) for l in open(f"{out}/requests.jsonl")]


def msg_text(m):
    c = m.get("content")
    if isinstance(c, str):
        return c
    parts = []
    for b in c or []:
        t = b.get("type")
        if t == "text":
            parts.append(b["text"])
        elif t == "tool_use":
            parts.append(json.dumps(b.get("input"), ensure_ascii=False))
        elif t == "tool_result":
            cc = b.get("content")
            if isinstance(cc, list):
                parts += [x.get("text", "") for x in cc if isinstance(x, dict)]
            else:
                parts.append(str(cc))
    return "\n".join(parts)


def mhash(m):
    # Claude Code moves cache_control markers between requests; they are not part of the message
    def strip(o):
        if isinstance(o, dict):
            return {k: strip(v) for k, v in o.items() if k != "cache_control"}
        if isinstance(o, list):
            return [strip(x) for x in o]
        return o
    return hashlib.sha256(json.dumps(strip(m), sort_keys=True).encode()).hexdigest()


seen = {}
tot_body = tot_msg = new_msg = 0
t_whole = t_new = 0.0
hits_by_fp = {}
enc = set()
print(f"{'req':>3} {'kind':5} {'msgs':>4} {'body KB':>8} {'msg KB':>7} {'new KB':>7}")
for i, r in enumerate(reqs):
    b = r["body"]
    enc.add(r["headers"].get("content-encoding") or r["headers"].get("Content-Encoding") or "none")
    if "messages" not in b:
        continue
    raw = json.dumps(b)
    tot_body += len(raw)
    main = any(t.get("name") == "Bash" for t in b.get("tools", []))
    texts_new, mk, nk = [], 0, 0
    for m in b["messages"]:
        t = msg_text(m)
        mk += len(t)
        h = mhash(m)
        if h not in seen:
            seen[h] = i
            texts_new.append(t)
            nk += len(t)
    sysp = b.get("system")
    tot_msg += mk; new_msg += nk
    t0 = time.perf_counter(); whole = scan_fast(raw); t_whole += time.perf_counter() - t0
    t0 = time.perf_counter()
    for t in texts_new:
        for h in scan_fast(t):
            if h["grade"] in ("checked", "prefix", "shape"):
                hits_by_fp.setdefault((h["rule"], h["fp"]), i)
    t_new += time.perf_counter() - t0
    print(f"{i:3} {'main' if main else 'side':5} {len(b['messages']):4} {len(raw)/1024:8.1f} {mk/1024:7.1f} {nk/1024:7.1f}")
print(f"\nbody encodings seen: {sorted(enc)}")
print(f"all request bodies: {tot_body/1024:.0f} KB; conversation text in them: {tot_msg/1024:.0f} KB; of it new: {new_msg/1024:.0f} KB")
print(f"re-send factor (conversation text sent / text new): {tot_msg/max(new_msg,1):.1f}")
print(f"scan time, whole bodies every request: {t_whole*1000:.1f} ms; new messages only: {t_new*1000:.1f} ms")
print("\ncredentials found (rule, fingerprint, first request that carried it):")
for (rule, fp), i in sorted(hits_by_fp.items(), key=lambda x: x[1]):
    print(f"  {rule:20} {fp}  request {i}")
# Claude Code's own files on disk
fps = {fp for (_, fp) in hits_by_fp}
print("\nClaude Code's files under its config dir that hold a found value verbatim:")
from detector import scan
for p in glob.glob(f"{out}/home/.claude/**/*", recursive=True):
    if os.path.isfile(p):
        try:
            t = open(p, encoding="utf-8", errors="replace").read()
        except Exception:
            continue
        found = {h["fp"] for h in scan(t, entropy_mode="off")} & fps
        if found:
            print(f"  {os.path.relpath(p, out)}: {len(found)} of {len(fps)} values")
