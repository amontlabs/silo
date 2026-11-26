"""`silo chain` under option A: from a file change to the tool call, the turn, what you asked and who
else used the path, with no process in between.

    python3 chain.py <night> [computer] <path | change id> [--json]

A path gives its last change in the record; a folder gives the changes under it in the last window
between two checkpoints that holds any. Each link says what it is:

  ●  a fact Silo saw: a file change between two checkpoints, a model call, a tool call's text, a
     tool result's text
  ◇  Silo's reading: which tool call made the change (#399's `author-from-traffic` rules), and
     who else uses a path

The link file → tool call is drawn only on #399's rules that rest on what one tool call said
(content, named, under, implied). On a weaker rule (only one conversation running, the program's
own files, a tool result naming the path) the chain stops at the file and lists the tool calls that
were open, because on a second run of #399's night those rules named a wrong conversation.
"""
import collections, json, os, re, sys

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import night_a as N  # noqa: E402

R = N.R
READER = "author-from-traffic@0.1 (#399)"


def target(night, computer, what):
    """The file facts the chain is about: one change, or the last window's changes under a path."""
    facts = night.facts(computer, "file")
    if what.startswith(computer + ":f"):
        f = night.fact(what)
        if not f or f["line"] != "file":
            raise ValueError(f"{what} is not a file change")
        return [f]
    p = night.untilde(computer, what)
    hits = [f for f in facts if f["path"] == p or f["path"].startswith(p + "/")]
    if not hits:
        raise ValueError(f"the record has no change to {what}")
    last = hits[-1]["between"]
    return [f for f in hits if f["between"] == last]


def who_else(night, computer, fs, root):
    """Files of the computer, outside the path, that point at it: a path written in them that, read
    from their own folder or from ~, is the path or a folder inside it. In the checkpoint before
    the change. #371 matched the folder's name alone, which also finds `fixtures` in
    `../shared-fixtures`."""
    n = int(fs[0]["between"][0].split("@ck")[1])
    m = night.manifest(computer, n)
    name = os.path.basename(root)
    home = night.home(computer)
    hits = []
    for p, e in sorted(m.items()):
        if e.get("t") != "f" or e["size"] > 200_000 or p == root or p.startswith(root + "/"):
            continue
        if any(x in p for x in ("/.git/", "/.claude/", "/.codex/", "/node_modules/")):
            continue
        src = night.content(e["sha"])
        if not src:
            continue
        with open(src, errors="replace") as fh:
            body = fh.read()
        if name not in body:
            continue
        for line in body.splitlines():
            for tok in re.findall(r"[~\w./-]*" + re.escape(name) + r"[\w./-]*", line):
                q = home + tok[1:] if tok.startswith("~") else tok if tok.startswith("/") else os.path.join(os.path.dirname(p), tok)
                q = os.path.normpath(q)
                if q == root or q.startswith(root + "/"):
                    hits.append((p, line.strip()))
                    break
            if hits and hits[-1][0] == p:
                break
    return hits[:5]


def chain(night, computer, what):
    fs = target(night, computer, what)
    root = os.path.commonpath([f["path"] for f in fs]) if len(fs) > 1 else fs[0]["path"]
    if len(fs) > 1 and what.startswith(("~", "/")):
        root = night.untilde(computer, what) if night.untilde(computer, what) != root and root.startswith(night.untilde(computer, what)) else root
    cks = {cid: t for t, cid, _, _ in night.checkpoints(computer)}
    a, b = cks[fs[0]["between"][0]], cks[fs[0]["between"][1]]
    authors = collections.Counter()
    for f in fs:
        x = night.author(f["id"])
        authors[(x["by"] if x["strong"] else None, x["tool_call"] if x["strong"] else None, x["how"] if x["strong"] else None)] += 1
    out = {"computer": computer, "path": root, "facts": [f["id"] for f in fs], "between": fs[0]["between"],
           "window": [a, b], "changes": dict(collections.Counter(f["change"] for f in fs)), "links": []}
    for (conv, tc, how), n in authors.most_common():
        link = {"files": n}
        if conv:
            link["opinion"] = {"by": READER, "says": {"conversation": conv, "tool_call": tc}, "rests_on": how}
            call = night.call.get(tc) if tc else None
            if call:
                m = night.model_call_of.get(tc)
                link["tool_call"] = {"id": tc, "tool": call.tool, "text": night.summary(call), "sent": call.start, "answered": call.end,
                                     "exit": night.exit_of(call), "model_call": m["id"] if m else None}
                if m:
                    link["turn"] = {"fact": m["id"], "turn": m.get("turn"), "model": m.get("model_answered"), "tokens_out": (m.get("tokens") or {}).get("out"),
                                    "cost_usd": m.get("cost_usd") if m.get("price_known") else None,
                                    "said": R.text(night.root, m.get("reply_text_ref"))}
            else:
                link["tool_calls"] = sorted({c.id for c in night.open_calls(fs[0]["id"]) if c.conversation == conv})
            cv = night.conversation(conv)
            link["asked"] = {"conversation": conv, "label": night.label(conv), "at": R.ts_of(cv["calls"][0]["started"]), "text": cv.get("asked")}
        else:
            open_ = [c for c in night.open_calls(fs[0]["id"])]
            link["open"] = [{"id": c.id, "conversation": c.conversation, "label": night.label(c.conversation), "text": night.summary(c)} for c in open_]
            link["weak"] = night.author(fs[0]["id"])["how"]
        out["links"].append(link)
    out["who_else"] = [{"path": p, "line": l} for p, l in who_else(night, computer, fs, root)]
    return out


def render(night, ch):
    c = ch["computer"]
    t = night.tilde
    a, b = ch["window"]
    ckA, ckB = (x.split("@")[1] for x in ch["between"])
    ch_words = ", ".join(f"{k} {v}" for k, v in ch["changes"].items())
    nf = len(ch["facts"])
    out = [f"{t(c, ch['path'])}" + (f"  · {nf} files: {ch_words}" if nf > 1 else f"  · {next(iter(ch['changes']))}"),
           f"  ● file       between {N.hm(a)} and {N.hm(b)} ({ckA} → {ckB}), the grain of two checkpoints"]
    for L in ch["links"]:
        pre = f"  [{L['files']} of {nf}]" if len(ch["links"]) > 1 else ""
        if pre:
            out.append(pre)
        if "opinion" in L:
            op = L["opinion"]
            out.append(f"  ◇ by         {N.RULE_WORDS[op['rests_on']]}   Silo's reading, {READER}")
            tc = L.get("tool_call")
            if tc:
                out.append(f"  ● tool call  {tc['tool']} {N.mark(N.words(tc['text'], 70))}")
                out.append(f"               sent {N.hm(tc['sent'])}" + (f", answered {N.hm(tc['answered'])}" if tc["answered"] else ", no answer yet") + f" · {tc['exit']}, from the tool result")
                tu = L.get("turn")
                if tu:
                    cost = f"${tu['cost_usd']:.3f}" if tu["cost_usd"] is not None else "spend not known"
                    out.append(f"  ● turn       {tu['turn']} of {L['asked']['label']} · {tu['model']} · {tu['tokens_out']} tokens out · {cost}")
                    if tu["said"]:
                        out.append(f"               its model said {N.mark(N.words(tu['said'], 90))}")
            else:
                out.append(f"  ● tool calls {len(L['tool_calls'])} of {L['asked']['label']} name it; the record cannot tell which one")
            out.append(f"  ● you asked  {N.hm(L['asked']['at'])} {N.mark(N.words(L['asked']['text'] or '', 100))}")
        else:
            out.append(f"  ◇ by         not known from the traffic" + (f" (the weaker reading, «{L['weak']}», is not drawn)" if L.get("weak") not in (None, "none") else ""))
            if L["open"]:
                out.append(f"               tool calls open between the two checkpoints:")
                for o in L["open"][:5]:
                    out.append(f"                 {o['label']}  {N.mark(N.words(o['text'], 60))}")
                if len(L["open"]) > 5:
                    out.append(f"                 … {len(L['open']) - 5} more")
            else:
                out.append("               no tool call was open: a program the agents left running, or you")
    if ch["who_else"]:
        out.append("  ◇ who else uses it")
        for w in ch["who_else"]:
            out.append(f"               {t(c, w['path'])}  {N.mark(N.words(w['line'], 60))}")
    out.append(f"  put back:    silo put-back {ch['facts'][0]} {t(c, ch['path'])}")
    out.append("  ● a fact Silo saw   ◇ Silo's reading. No process is recorded: the link from a file to a tool call is read from the call's text.")
    return "\n".join(out)


def main(argv):
    as_json = "--json" in argv
    argv = [x for x in argv if x != "--json"]
    night = N.Night(argv[0])
    rest = argv[1:]
    computer = rest.pop(0) if len(rest) > 1 and rest[0] in night.homes else "main"
    try:
        ch = chain(night, computer, rest[0])
    except ValueError as e:
        print(f"silo: {e}")
        return 2
    print(json.dumps(ch, default=str) if as_json else render(night, ch))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
