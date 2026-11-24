"""Test 20's facts and outcome from what peercred's server and clients printed.

    20-analyze.py SERVE_JSONL CLIENTS_JSONL OTHER_JSONL MY_UID

SERVE_JSONL: the server's lines (one per connection), CLIENTS_JSONL: the clients' lines,
OTHER_JSONL: the other-user half (server lines and client lines mixed; empty when skipped).
Prints tab-separated lines: "fact NAME VALUE", "measure NAME VALUE UNIT", and last
"outcome KEY SENTENCE".
"""
import json
import sys


def lines(path):
    out = []
    try:
        for line in open(path):
            try:
                out.append(json.loads(line))
            except ValueError:
                pass
    except OSError:
        pass
    return out


def label_of(conn):
    h = conn.get("hello", "")
    return h.split()[1] if h.startswith("hello ") and len(h.split()) > 1 else ""


def chain_text(chain):
    return " < ".join("%s(%s)" % (p.get("comm") or "?", p.get("pid")) for p in chain or [])


def app_in(chain):
    for p in chain or []:
        path = p.get("path") or ""
        if ".app/" in path:
            return path.split(".app/")[0].rsplit("/", 1)[-1] + ".app"
    return ""


def ident(look):
    """The identities one look gives: (uid, pid, pidversion, path)."""
    uid = (look.get("getpeereid") or {}).get("uid")
    pid = look.get("local_peerpid", (look.get("so_peercred") or {}).get("pid"))
    tok = look.get("local_peertoken") or {}
    path = (look.get("chain") or [{}])[0].get("path", "")
    return uid, pid, tok.get("pidversion"), path, tok


def main():
    serve, clients, other = lines(sys.argv[1]), lines(sys.argv[2]), lines(sys.argv[3])
    my_uid = int(sys.argv[4])
    out = []
    by = {label_of(c): c for c in serve if c.get("event") == "conn"}
    cl = {c.get("label"): c for c in clients if c.get("event") == "client"}
    after = {c.get("label"): c for c in clients if c.get("event") == "after-exec"}
    problems = []

    d = by.get("direct")
    if d:
        uid, pid, ver, path, tok = ident(d["at_accept"])
        out.append(("fact", "direct.identity", "uid %s pid %s pidversion %s program %s" % (uid, pid, ver, path)))
        if uid != my_uid:
            problems.append("the server saw uid %s for a caller of uid %s" % (uid, my_uid))
        cpid = cl.get("direct", {}).get("pid")
        if cpid is not None and pid != cpid:
            problems.append("the peer pid %s is not the client's pid %s" % (pid, cpid))
        if tok and tok.get("pid") != cpid:
            problems.append("the audit token's pid %s is not the client's pid %s" % (tok.get("pid"), cpid))
        lc = d["at_accept"].get("local_peercred")
        if lc is not None:
            out.append(("fact", "direct.local_peercred", json.dumps(lc)))
        for k in ("local_peerepid", "local_peeruuid", "so_peerpidfd"):
            if k in d["at_accept"]:
                out.append(("fact", "direct." + k, json.dumps(d["at_accept"][k])))
        out.append(("fact", "direct.chain", chain_text(d["at_accept"].get("chain"))))
        out.append(("fact", "direct.app_in_chain", app_in(d["at_accept"].get("chain")) or "none"))
        out.append(("fact", "direct.after_exit", "pid %s, program %r after the client exited" % (
            ident(d["later"])[1], ident(d["later"])[3])))
    else:
        problems.append("no connection from the direct client")

    ver_change = None
    e = by.get("exec-self")
    if e:
        v0 = ident(e["at_accept"])[2]
        v1 = ident(e["later"])[2]
        own0 = (cl.get("exec-self", {}).get("own_token") or {}).get("pidversion")
        own1 = (after.get("exec-self", {}).get("own_token") or {}).get("pidversion")
        if v0 is not None and v1 is not None:
            ver_change = v0 != v1
        out.append(("fact", "exec_self.pidversion", "token at accept %s, after exec %s; the client's own token before exec %s, after %s" % (v0, v1, own0, own1)))
        out.append(("fact", "exec_self.pid", "at accept %s, after exec %s" % (ident(e["at_accept"])[1], ident(e["later"])[1])))
    s = by.get("exec-sleep")
    sleep_seen = None
    if s:
        p0, p1 = ident(s["at_accept"])[3], ident(s["later"])[3]
        sleep_seen = p1.endswith("/sleep")
        out.append(("fact", "exec_sleep.program", "at accept %s, after exec %s" % (p0, p1)))
        v0, v1 = ident(s["at_accept"])[2], ident(s["later"])[2]
        if v0 is not None:
            out.append(("fact", "exec_sleep.pidversion", "at accept %s, after exec %s" % (v0, v1)))
            if ver_change is None and v1 is not None:
                ver_change = v0 != v1
    o = by.get("orphan")
    orphan_app = None
    if o:
        ch = o["at_accept"].get("chain") or []
        orphan_app = app_in(ch)
        out.append(("fact", "orphan.chain", chain_text(ch)))
        out.append(("fact", "orphan.app_in_chain", orphan_app or "none"))

    refused = None
    blocked = None
    if other:
        for x in other:
            if x.get("event") == "client" and x.get("label") == "other-private":
                blocked = x.get("connect") != "ok"
                out.append(("fact", "other_user.private_socket", "connect: %s" % x.get("connect")))
            if x.get("event") == "client" and x.get("label") == "other-open":
                out.append(("fact", "other_user.open_socket", "connect: %s, answer: %s" % (x.get("connect"), x.get("answer"))))
                refused = x.get("answer") == "refused"
            if x.get("event") == "conn":
                out.append(("fact", "other_user.server_saw", "uid %s, refused %s" % (ident(x["at_accept"])[0], x.get("refused"))))
    else:
        out.append(("fact", "other_user", "skipped"))

    app = app_in((d or {}).get("at_accept", {}).get("chain"))
    parts = []
    if d:
        parts.append("a caller of your user is identified by user %s and pid, and its parent chain %s" % (
            my_uid, "reaches %s" % app if app else "reaches no app (%s)" % chain_text(d["at_accept"].get("chain"))[:200]))
    if ver_change is not None:
        parts.append("the audit token's pid version %s on exec" % ("changes" if ver_change else "does not change"))
    if sleep_seen is not None:
        parts.append("a look after exec %s the new program" % ("sees" if sleep_seen else "does not see"))
    if o:
        parts.append("a double-forked caller's chain %s" % ("still reaches %s" % orphan_app if orphan_app else "ends without the app"))
    if refused is not None or blocked is not None:
        parts.append("another user is %s at the private socket and %s at an open one" % (
            "stopped" if blocked else "NOT stopped", "refused" if refused else "NOT refused"))
    text = "; ".join(parts) + "."
    if problems:
        key, text = "inconclusive", "; ".join(problems) + ". " + text
    elif blocked is False or refused is False:
        key = "other-user-not-refused"
    elif ver_change is False:
        key = "pidversion-stays"
    elif ver_change is None:
        key = "inconclusive"
        text = "No audit token on this system (Linux analogue, or LOCAL_PEERTOKEN failed): " + text
    else:
        key = "identity-holds"
    for row in out:
        print("\t".join(str(x) for x in row))
    print("outcome\t%s\t%s" % (key, text))


if __name__ == "__main__":
    main()
