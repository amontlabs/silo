"""Reads one program's fork run (requests.jsonl, ledger.jsonl, steps.jsonl) and answers:

1. What each side of a fork sends that could tell it apart (identifiers).
2. Which earlier request each request continues, by content, as `recognising-a-conversation` reads
   it (the format read, cache markers left out).
3. How three ways of recognising a conversation group the requests:
   - across:  the answer as written, with no notion of computer: content, then the session join.
   - fresh:   one computer per conversation; a fork's first request starts a new one at zero
              (#373's proposed text, #385's `fresh`).
   - linked:  one computer per conversation; a fork's first request that continues a request its
              parent sent before the fork starts a new conversation linked to the parent's at that
              request, and its budget starts at what the parent's had spent there (#347, #385's
              `carry`).
4. What each counts against a conversation budget, from what the stand-in billed.
5. What a cache estimate kept per computer gets wrong on a fork's first request.
6. What the record stores, messages once by hash, per computer or per Mac.

Usage: python3 analyse.py <run folder>   (prints Markdown)
"""
import hashlib, json, sys

RUN = sys.argv[1]
reqs = [json.loads(l) for l in open(f"{RUN}/requests.jsonl")]
reqs = [r for r in reqs if r["method"] == "POST" and isinstance(r["body"], dict)
        and (r["path"].split("?")[0].endswith("/v1/messages") or r["path"].endswith("/responses"))]
bill = {o["rid"]: o for o in map(json.loads, open(f"{RUN}/ledger.jsonl"))}
steps = [json.loads(l) for l in open(f"{RUN}/steps.jsonl")]
PROG = "Claude Code" if reqs[0]["path"].split("?")[0].endswith("/v1/messages") else "Codex"

# What Silo knows without asking the program: which computer each is, and what it was made from.
# old is a fork of a checkpoint of main taken after step 2; after case G, main's disk is fork's.
step_of = {}
for i, s in enumerate(steps):
    for n in range(s["first"], s["last"] + 1): step_of[n] = (i, s)
G_FIRST = next((s["first"] for s in steps if s["branch"] == "G"), 10**9)
def made_from(comp, n):
    """The parent computer and the last request number of the parent the fork's disk holds."""
    first = lambda b: next(s["first"] for s in steps if s["branch"] == b)
    if comp == "fork": return ("main", first("B") - 1)
    if comp == "forkid": return ("main", first("C") - 1)
    if comp == "old": return ("main", steps[1]["last"])           # checkpoint after step 2
    if comp == "fork2": return ("fork", first("E") - 1)
    if comp == "main" and n >= G_FIRST: return ("fork", G_FIRST - 1)  # fork became main
    return None

def computer_of(r):
    # After case G, requests from main come from the disk that was fork's: a new computer in the
    # record's eyes, made from fork, which takes main's name.
    return "main*" if r["computer"] == "main" and r["n"] >= G_FIRST else r["computer"]

# ---- the content of a request, as the provider's format reads it ----

def norm(x):
    if isinstance(x, dict):
        return {k: norm(v) for k, v in x.items() if k not in ("cache_control", "id")}
    if isinstance(x, list):
        return [norm(v) for v in x]
    return x

def items(r):
    b = r["body"]
    if PROG == "Claude Code":
        out = []
        for m in b["messages"]:
            c = m["content"]
            if isinstance(c, str): c = [{"type": "text", "text": c}]
            out.append({"role": m["role"], "content": norm(c)})
        return out
    return [norm(i) for i in b["input"]]

def reply_items(r):
    """The reply to a request, as the next request of the same conversation would hold it."""
    return None  # filled from the next request: see continues()

H = lambda o: hashlib.sha256(json.dumps(o, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
seq = {r["n"]: [H(i) for i in items(r)] for r in reqs}
model = {r["n"]: r["body"].get("model") for r in reqs}
def session(r):
    h = r["headers"]
    return h.get("x-claude-code-session-id") or h.get("session-id")

def continues(a, b):
    """Request b continues request a: same model, and b's items begin with all of a's items and at
    least one more (the reply). Read from b, since a's reply is in b."""
    return model[a] == model[b] and len(seq[b]) > len(seq[a]) and seq[b][:len(seq[a])] == seq[a]

def content_parent(r, pool):
    """The latest request in pool that r continues, preferring the longest."""
    best = None
    for p in pool:
        if p["n"] < r["n"] and continues(p["n"], r["n"]):
            if best is None or len(seq[p["n"]]) > len(seq[best["n"]]) or (len(seq[p["n"]]) == len(seq[best["n"]]) and p["n"] > best["n"]):
                best = p
    return best

# ---- 1. identifiers ----

def ids(r):
    h, b = r["headers"], r["body"]
    if PROG == "Claude Code":
        md = json.loads(b["metadata"]["user_id"])
        return {"session header": h.get("x-claude-code-session-id"), "metadata session": md.get("session_id"),
                "device_id": md.get("device_id")}
    cm = b.get("client_metadata") or {}; tm = json.loads(cm.get("x-codex-turn-metadata") or "{}")
    return {"session-id / thread-id": h.get("session-id") + " / " + h.get("thread-id"), "prompt_cache_key": b.get("prompt_cache_key"),
            "window": h.get("x-codex-window-id"), "installation_id": cm.get("x-codex-installation-id"),
            "history_initialization": tm.get("history_initialization"),
            "forked_from_thread_id": tm.get("forked_from_thread_id")}

print(f"# {PROG}: {len(reqs)} model requests in {len(steps)} steps\n")
print("## What each step sent\n")
first_ids = None
short = {}
def sh(v):
    if v is None: return "none"
    if v not in short: short[v] = chr(ord("a") + len(short)) if len(short) < 26 else str(len(short))
    return f"`{short[v]}`"
keys = list(ids(reqs[0]).keys())
print("| Step | Computer | Case | " + " | ".join(keys) + " |")
print("|---" * (3 + len(keys)) + "|")
for i, s in enumerate(steps):
    r = next(x for x in reqs if x["n"] == s["first"])
    d = ids(r)
    print(f"| {i+1} | {computer_of(r)} | {s['branch']} | " + " | ".join(sh(d[k]) if k not in ("history_initialization",) else (d[k] or "none") for k in keys) + " |")
print("\nEach letter stands for one distinct value; the same letter is the same value.\n")

# ---- 2. what each step's first request continues ----

print("## What each step's first request continues, by content\n")
print("| Step | Computer | Case | Continues request | Sent by | Of step |")
print("|---|---|---|---|---|---|")
byn = {r["n"]: r for r in reqs}
for i, s in enumerate(steps):
    r = byn[s["first"]]
    p = content_parent(r, reqs)
    if p: print(f"| {i+1} | {computer_of(r)} | {s['branch']} | {p['n']} | {computer_of(p)} | {step_of[p['n']][0]+1} ({step_of[p['n']][1]['branch']}) |")
    else: print(f"| {i+1} | {computer_of(r)} | {s['branch']} | none | | |")
print()

# ---- 3. three ways of recognising a conversation ----

def recognise(mode, split=True):
    """Returns conv[n] -> conversation name, link[conv] -> (parent conv, counted up to request n).
    mode "now" is #385's `carry` as built: the parent's conversations are searched as they stand
    when the fork's first request comes, one level up, and the fork starts at what that
    conversation has counted by then. split=False: main keeps its name and history after a fork
    replaces it, as if Silo did not know its disk changed."""
    global computer_of
    keep = computer_of
    if not split: computer_of = lambda r: r["computer"]
    try: return _recognise(mode)
    finally: computer_of = keep

def _recognise(mode):
    conv, link, k = {}, {}, [0]
    def new(r):
        k[0] += 1; return f"{computer_of(r) if mode != 'across' else ''}#{k[0]}".lstrip("#") if mode != "across" else f"#{k[0]}"
    for r in reqs:
        comp = computer_of(r)
        pool = reqs[:reqs.index(r)] if mode == "across" else [p for p in reqs[:reqs.index(r)] if computer_of(p) == comp]
        p = content_parent(r, pool)
        if p: conv[r["n"]] = conv[p["n"]]; continue
        same = [q for q in pool if session(q) == session(r)]
        if same: conv[r["n"]] = conv[same[-1]["n"]]; continue
        c = new(r); conv[r["n"]] = c
        if mode == "now":
            src = made_from(r["computer"], r["n"])
            if src:
                pcomp = src[0]
                ppool = [q for q in reqs[:reqs.index(r)] if q["computer"] == pcomp]
                pp = content_parent(r, ppool)
                if pp: link[c] = (conv[pp["n"]], r["n"] - 1)
        if mode == "linked":
            # A fork's first request: look in what the fork's disk was made from, before the fork.
            src = made_from(r["computer"], r["n"])
            while src:
                pcomp, upto = src
                ppool = [q for q in reqs if q["n"] <= upto and computer_of(q) == (pcomp if pcomp != "main" or upto < G_FIRST else "main")]
                pp = content_parent(r, ppool)
                if pp: link[c] = (conv[pp["n"]], pp["n"]); break
                src = made_from(pcomp, upto) if pcomp != "main" else None
    return conv, link

def spent_upto(conv, c, n, link):
    """What conversation c has counted up to and including request n, with what it carries."""
    own = sum(bill[q]["cost"] for q in conv if conv[q] == c and q <= n)
    if c in link:
        pc, at = link[c]; own += spent_upto(conv, pc, at, link)
    return own

print("## How each way groups the requests\n")
modes = {"across": "Across computers (the answer as written)", "fresh": "One computer, fork starts at zero",
         "linked": "One computer, linked to the parent at the fork"}
results = {m: recognise(m) for m in modes}
EXTRA = {"now": ("#385's carry as built: the parent's spend when the fork first calls", recognise("now")),
         "linked-unsplit": ("Linked, but main keeps its history when a fork replaces it", recognise("linked", split=False))}
print("| Step | Computer | Case | " + " | ".join(modes.values()) + " |")
print("|---|---|---|" + "---|" * len(modes))
for i, s in enumerate(steps):
    n = s["first"]; row = []
    for m in modes:
        conv, link = results[m]
        c = conv[n]; extra = f" (from {link[c][0]} at {link[c][1]})" if c in link and m == "linked" else ""
        row.append(f"`{c}`{extra}")
    print(f"| {i+1} | {computer_of(byn[n])} | {s['branch']} | " + " | ".join(row) + " |")
print()
for m in modes:
    conv, _ = results[m]
    mixed = {c for c in set(conv.values()) if len({computer_of(byn[n]) for n in conv if conv[n] == c}) > 1}
    print(f"- {modes[m]}: {len(set(conv.values()))} conversations, {len(mixed)} holding requests of more than one computer.")
print()

# ---- 4. what each counts against a conversation budget ----

print("## What a conversation budget counts at the end of each step\n")
print("Dollars as the stand-in billed. `across` counts every computer's turns in one conversation;")
print("`fresh` starts a fork's at zero; `linked` starts it at what the parent's had spent at the fork.\n")
allm = list(modes) + list(EXTRA)
res = dict(results, **{m: v[1] for m, v in EXTRA.items()})
print("`now` is #385's `carry` as built; `linked-unsplit` is `linked` when Silo does not treat main")
print("as a new computer after a fork replaces it.\n")
print("| Step | Computer | Case | Step's own cost | " + " | ".join(allm) + " |")
print("|---|---|---|---|" + "---|" * len(allm))
for i, s in enumerate(steps):
    n = s["last"]; own = sum(bill[q]["cost"] for q in range(s["first"], s["last"] + 1))
    row = []
    for m in allm:
        conv, link = res[m]
        row.append(f"{spent_upto(conv, conv[n], n, link if m in ('linked', 'now', 'linked-unsplit') else {}):.4f}")
    print(f"| {i+1} | {computer_of(byn[n])} | {s['branch']} | {own:.4f} | " + " | ".join(row) + " |")
print()

# ---- 5. the cache on a fork's first request ----

print("## A fork's first request and the provider's cache\n")
print("The provider keeps a cache per key, not per computer. An estimate that keeps the cache per")
print("computer prices a fork's first request as if nothing were cached.\n")
print("| Step | Computer | Case | Cache read, billed | Billed | Estimate, nothing cached | Error |")
print("|---|---|---|---|---|---|---|")
PRICE_IN = 5.0; PRICE_W = 6.25 if PROG == "Claude Code" else 5.0
for i, s in enumerate(steps):
    r = byn[s["first"]]; b = bill[r["n"]]
    comp = computer_of(r)
    if any(computer_of(q) == comp for q in reqs if q["n"] < r["n"]): continue
    if i == 0: continue
    tot_in = b["input"] + b["cache_read"] + b["cache_write"]
    # Nothing cached: what was read would be written (Anthropic) or plain input (OpenAI).
    est = (b["input"] * PRICE_IN + (b["cache_read"] + b["cache_write"]) * PRICE_W + b["output"] * 25.0) / 1e6
    print(f"| {i+1} | {comp} | {s['branch']} | {b['cache_read']} of {tot_in} tokens | {b['cost']:.4f} | {est:.4f} | {100*(est-b['cost'])/b['cost']:+.0f}% |")
print()

# ---- 6. the record: messages once by hash ----

print("## What the record stores, each message once by hash\n")
size = {}
for r in reqs:
    for it in items(r):
        size[H(it)] = len(json.dumps(it, separators=(",", ":")).encode())
raw = sum(len(json.dumps(r["body"], separators=(",", ":")).encode()) for r in reqs)
per_comp = {}
for r in reqs:
    per_comp.setdefault(computer_of(r), set()).update(seq[r["n"]])
once_per_computer = sum(size[h] for hs in per_comp.values() for h in hs)
once_per_mac = sum(size[h] for h in set().union(*per_comp.values()))
print(f"| Stored | Bytes |\n|---|---|\n| Every request as sent | {raw} |\n| Messages once, a store per computer | {once_per_computer} |\n| Messages once, one store per Mac | {once_per_mac} |\n")
