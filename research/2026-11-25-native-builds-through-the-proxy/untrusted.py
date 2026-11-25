"""Shows, case by case, what the proxy's log and the provider side's log say about a program that
does not trust the certificate it is shown.

The proxy sees a CONNECT and bytes each way: to it, a refused handshake looks like any other
allowed connection. Only the side that ends TLS (here provider.py; in Silo, the boundary) sees the
handshake fail, and how: Codex sends the alert `unknown_ca`, Claude Code closes the connection.
So the boundary, not the proxy, is where "this program does not trust Silo" can be recorded.

Usage: python3 untrusted.py OUT   (the folder run.sh wrote)
"""
import json, sys
from collections import Counter

out = sys.argv[1]


def by_case(path):
    cases, cur = {}, None
    for line in open(path):
        o = json.loads(line)
        if "case" in o:
            cur = o["case"]; cases[cur] = []
        elif cur:
            cases[cur].append(o)
    return cases


proxy, prov = by_case(f"{out}/proxy.jsonl"), by_case(f"{out}/provider.jsonl")
print("case\tproxy: connections allowed, refused\tTLS side: handshakes done, failed (how)")
for case in proxy:
    p = Counter(o["decision"].split(",")[0] for o in proxy[case])
    t = prov.get(case, [])
    ok = sum(1 for o in t if o.get("ev") == "tls")
    bad = Counter(("alert unknown_ca" if "UNKNOWN_CA" in o["error"] else "closed by the client" if "EOF" in o["error"] else o["error"][:40]) for o in t if o.get("ev") == "tls-failed")
    print(f"{case}\t{p.get('allowed', 0)}, {p.get('refused', 0)}\t{ok}, {sum(bad.values())} {dict(bad) if bad else ''}")
