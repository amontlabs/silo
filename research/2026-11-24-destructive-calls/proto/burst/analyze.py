"""Summarise the destructive requests of each trace, and replay grouping policies on them.

    python3 analyze.py traces/destroy.jsonl ...          a summary per trace
    python3 analyze.py --simulate traces/destroy.jsonl   policies replayed offline

The offline replay keeps each request's arrival time from a run without holds. That is right for
requests that do not wait on each other and wrong for those that do (a held delete delays what
depends on it), so the live runs (`POLICY=... ./run.sh`) are the measure; this replay only shows
the upper bound of how much one ask could gather.
"""
import json
import re
import sys
from collections import Counter, defaultdict

DESTRUCTIVE = re.compile(r"[a-z0-9-]+:(Delete|Revoke|Detach|Terminate|Remove|Disassociate|Deregister|DELETE )")


def load(path):
    return [json.loads(l) for l in open(path) if l.strip()]


def is_destructive(r):
    if DESTRUCTIVE.match(r["op"]):
        return True
    if r["op"] == "s3:PutBucketVersioning" and r.get("body") and "Suspended" in r["body"]:
        return True        # a protection switched off
    return False


def name_of(r):
    """The thing a request deletes, from the request alone."""
    b = r.get("body") or ""
    for k in ("DBInstanceIdentifier", "DBSubnetGroupName", "GroupId", "SubnetId", "VpcId",
              "RouteTableId", "InternetGatewayId", "RoleName", "PolicyArn", "SecurityGroupRuleIds.1",
              "SecurityGroupRuleId.1"):
        m = re.search(r"(?:^|&)" + re.escape(k) + r"=([^&]+)", b)
        if m:
            return m.group(1)
    m = re.search(r'"(?:TableName|logGroupName)"\s*:\s*"([^"]+)"', b)
    if m:
        return m.group(1)
    return r["path"]


def concurrency(rs):
    ev = sorted([(r["start"], 1) for r in rs] + [(r["end"], -1) for r in rs])
    cur = best = 0
    for _, d in ev:
        cur += d
        best = max(best, cur)
    return best


def waves(rs, gap=1.0):
    out, cur, last = [], [], None
    for r in sorted(rs, key=lambda r: r["start"]):
        if last is not None and r["start"] - last > gap:
            out.append(cur)
            cur = []
        cur.append(r)
        last = r["start"]
    if cur:
        out.append(cur)
    return out


def summary(path):
    rs = load(path)
    d = [r for r in rs if is_destructive(r)]
    print(f"\n== {path}: {len(rs)} requests, {len(d)} destructive")
    if not d:
        return
    t0 = min(r["start"] for r in d)
    print(f"   spread {max(r['start'] for r in d) - t0:.2f} s, at most {concurrency(d)} destructive in flight, "
          f"{len({r['conn'] for r in d})} connections, hosts {dict(Counter(r['host'] if 'host' in r else r['service'] for r in d))}")
    for i, w in enumerate(waves(d)):
        print(f"   wave {i + 1} at +{w[0]['start'] - t0:.2f} s ({len(w)}): " +
              ", ".join(f"{r['op'].split(':')[1]} {name_of(r)}" + (f" [{r.get('status')}]" if r.get("status") != 200 and r.get("status") != 204 else "")
                        for r in w))
    inv = Counter(r["invocation_id"] for r in rs if r.get("invocation_id"))
    dup = {k: v for k, v in inv.items() if v > 1}
    if dup:
        print(f"   invocation ids sent more than once: {len(dup)} ({sum(dup.values())} requests)")
    ident = Counter(r["identity"] for r in d)
    dupi = {k: v for k, v in ident.items() if v > 1}
    if dupi:
        print(f"   identical destructive requests (method+path+body): {len(dupi)} groups, {sum(dupi.values())} requests")


def simulate(path, answer_after=8.0, window=3.0):
    """Upper bound: arrivals as without holds, one user answering one ask at a time."""
    d = sorted((r for r in load(path) if is_destructive(r)), key=lambda r: r["start"])
    if not d:
        return
    keys = {
        "one per request": lambda r: r["id"],
        "per (computer, host)": lambda r: r.get("host", r["service"]),
        "per connection": lambda r: r["conn"],
        "per program": lambda r: re.search(r"(Terraform/[\d.]+|aws-cli/[\d.]+)", r.get("ua", "")) and
                                 re.search(r"(Terraform/[\d.]+|aws-cli/[\d.]+)", r["ua"]).group(1),
        "per computer": lambda r: 0,
    }
    print(f"\n== offline replay of {path} ({len(d)} destructive), window {window} s, answer {answer_after} s")
    for label, kf in keys.items():
        # retries join: same invocation id is one line
        lines = {}
        for r in d:
            lines.setdefault(r.get("invocation_id") or r["id"], r)
        groups = defaultdict(list)
        asks = []
        for r in lines.values():
            k = kf(r)
            if groups[k] and r["start"] <= groups[k][-1]["open"] + window:
                groups[k][-1]["n"] += 1
            else:
                a = {"open": r["start"], "n": 1}
                groups[k].append(a)
                asks.append(a)
        asks.sort(key=lambda a: a["open"])
        free, first_wait = 0.0, None
        for a in asks:
            shown = a["open"] + window
            ans = max(shown, free) + answer_after
            free = ans
            if first_wait is None:
                first_wait = ans - a["open"]
        print(f"   {label:22s} {len(asks):4d} asks, first request waits {first_wait:6.1f} s, last answer +{free - d[0]['start']:7.1f} s")


if __name__ == "__main__":
    args = sys.argv[1:]
    if args and args[0] == "--simulate":
        for p in args[1:]:
            simulate(p)
    else:
        for p in args:
            summary(p)
