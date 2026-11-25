"""G9: what a restore cannot bring back, as a list made from the record.

Input: the record of one computer as JSONL (one line per checkpoint, request or connection, as the
boundary saw it) and the checkpoint the restore goes back to. Output: the list shown where the
restore is offered, in three groups, each line marked with how Silo knows it.

"Only read" has the meaning `secret-limits` gives it, so the product has one: GET and HEAD, and
the requests a rule lists as reads for a host, such as a Git fetch sent as a POST.

Usage: python3 restore_list.py <record.jsonl> <checkpoint id>
"""
import json, re, sys

READ_METHODS = {"GET", "HEAD"}
# the reads a rule lists for a host, as in `secret-limits`: (host pattern, method, path pattern)
LISTED_READS = [
    (r".*", "POST", r".*/git-upload-pack$"),          # git fetch, clone
    (r".*", "GET", r".*/info/refs.*"),
]
# statements that only read, when the database protocol is read (G6); anything else is a write.
# A function call in a SELECT can still write: that is the "GET with side effects" of databases.
DB_READ = re.compile(r"^\s*(SELECT|SHOW|EXPLAIN|DESCRIBE|WITH\s+(?:(?!INSERT|UPDATE|DELETE|MERGE).)*SELECT)\b", re.I | re.S)
DB_FUNCTION_CALL = re.compile(r"\b[a-z_][a-z0-9_]*\s*\(", re.I)
DB_PURE_FUNCTIONS = {"count", "sum", "min", "max", "avg", "now", "coalesce", "lower", "upper", "length"}
# an opinion only: words in the path of a GET that often mean the GET changes something
GET_SIDE_EFFECT_WORDS = re.compile(r"(delete|remove|destroy|drop|trigger|deploy|hook|reset|purge|unsubscribe|confirm|approve)", re.I)


def is_listed_read(r):
    return any(re.fullmatch(h, r["host"]) and r.get("method") == m and re.fullmatch(p, r.get("path", ""))
               for h, m, p in LISTED_READS)


def db_reads_only(stmt):
    if not DB_READ.match(stmt):
        return False
    calls = {c.rstrip("( ").lower() for c in DB_FUNCTION_CALL.findall(stmt)}
    return not (calls - DB_PURE_FUNCTIONS - {"from", "in", "values"})


def since(record, checkpoint_id):
    lines = [json.loads(l) for l in open(record) if l.strip()] if isinstance(record, str) else record
    idx = next(i for i, x in enumerate(lines) if x.get("type") == "checkpoint" and x["id"] == checkpoint_id)
    return lines[idx + 1:]


def restore_list(lines):
    changed, unknown, maybe, reads, total_out, hosts_out = [], [], [], 0, 0, set()
    for r in lines:
        typ = r.get("type")
        if typ == "checkpoint":
            continue
        if r.get("denied"):
            continue  # nothing left the computer: the record keeps it among the denied attempts
        total_out += r.get("bytes_out", 0)
        if r.get("host"):
            hosts_out.add(r["host"])
        when = r["t"][11:16]
        sec = f", with the secret {r['secret']}" if r.get("secret") else ""
        if typ == "mac":
            changed.append({"kind": "fact", "t": when, "line": f"{r['what']}: it stays on your Mac"})
            continue
        if typ == "dns":
            continue  # counted in what was sent; a lookup changes nothing a restore could undo
        if typ == "db" and r.get("opened"):
            stmt = r["statement"].strip()
            if db_reads_only(stmt):
                reads += 1
            elif DB_READ.match(stmt):
                maybe.append({"kind": "opinion", "t": when, "line": f"{r['host']}: `{stmt}` only reads by its form, but calls a function that may write"})
            else:
                changed.append({"kind": "fact", "t": when, "line": f"{r['host']}: `{stmt}`, the database answered {r['status']}"})
            continue
        if typ == "http" and r.get("opened") and r.get("model_traffic"):
            continue  # spend stays, as `what-a-restore-leaves` says; shown with the spend, not here
        if not r.get("opened"):
            what = r.get("protocol") or "an encrypted connection"
            if what == "ssh":
                line = (f"{r['host']}:{r['port']}: an SSH session of {r.get('duration_s', '?')} s, "
                        f"{fmt(r['bytes_out'])} sent. Any command could have run there")
            else:
                line = f"{r['host']}:{r['port']}: {what}, {fmt(r['bytes_out'])} sent{sec}. Silo did not read it"
            unknown.append({"kind": "fact", "t": when, "line": line})
            if what != "ssh" and r.get("protocol") is None and r.get("port") in (5432, 3306, 6379, 27017):
                maybe.append({"kind": "opinion", "t": when, "line": f"{r['host']}:{r['port']} is the usual port of a database"})
            continue
        m = r.get("method", "")
        if m in READ_METHODS or is_listed_read(r):
            reads += 1
            if m == "GET" and GET_SIDE_EFFECT_WORDS.search(r.get("path", "")):
                maybe.append({"kind": "opinion", "t": when,
                              "line": f"{m} {r['host']}{r['path']}{sec}: a read by its method, but its path suggests it changes something"})
            continue
        answer = answer_of(r)
        if r.get("push_refs"):
            for p in r["push_refs"]:
                force = ", forced" if p.get("force") else ""
                changed.append({"kind": "fact", "t": when,
                                "line": f"pushed {p['ref']} {p['old']} -> {p['new']}{force} to {r['host']}{r['path'].rsplit('/git-receive-pack', 1)[0]}{sec}, {answer}"})
            continue
        changed.append({"kind": "fact", "t": when, "line": f"{m} {r['host']}{r['path']}{sec}, {answer}"})
    return {"changed_elsewhere": changed, "not_read": unknown, "may_have_changed": maybe,
            "reads_count": reads, "sent_total": total_out, "hosts": sorted(hosts_out)}


def answer_of(r):
    if r.get("status") is None:
        return f"no answer seen ({r.get('ended', 'none')}): it may or may not have happened"
    if 200 <= r["status"] < 300:
        return f"the host answered {r['status']}"
    return f"the host answered {r['status']}, a refusal"


def fmt(n):
    return f"{n / 1e6:.1f} MB" if n >= 1e6 else (f"{n / 1e3:.0f} kB" if n >= 1e3 else f"{n} B")


def render(res, checkpoint_label):
    out = [f"Not brought back by this restore (since {checkpoint_label})", ""]
    if res["changed_elsewhere"]:
        out.append("Sent out to change something elsewhere   [seen at the boundary]")
        out += [f"  {x['t']}  {x['line']}" for x in res["changed_elsewhere"]]
    if res["not_read"]:
        out.append("Connections Silo did not read: it cannot say what they changed   [seen at the boundary]")
        out += [f"  {x['t']}  {x['line']}" for x in res["not_read"]]
    if res["may_have_changed"]:
        out.append("Reads that may have changed something   [Silo's classifier thinks]")
        out += [f"  {x['t']}  {x['line']}" for x in res["may_have_changed"]]
    out.append(f"{res['reads_count']} other requests only asked to read. A host can still change something on a read;")
    out.append("Silo cannot see that.")
    out.append(f"Everything sent since then stays sent: {fmt(res['sent_total'])} to {len(res['hosts'])} hosts.")
    out.append("Silo sees what left this computer, not what the hosts did with it.")
    return "\n".join(out)


if __name__ == "__main__":
    res = restore_list(since(sys.argv[1], sys.argv[2]))
    print(render(res, sys.argv[2]))
