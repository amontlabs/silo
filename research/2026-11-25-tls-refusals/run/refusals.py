"""Turns the boundary's connection lines into what a person sees about programs that do not trust
Silo's certificate: an alert when a refusal starts, and one line per program and host in the
morning. A reader of the record, as a rule or the app is: it reads facts and writes nothing back.

usage: refusals.py <record lines (JSON, one a line; `silorec print` gives them)> [--now RFC3339]
       refusals.py --test

How lines are read:
- `why: certificate-refused` (the program sent a certificate alert) and `closed-after-certificate`
  (it hung up once it had Silo's certificate, as Node and Bun do) are refusals.
- `why: no-request` (the handshake finished, then the program closed without a request) is what
  GnuTLS (git) and a pinned key look like; any program that opens a connection and gives up looks
  the same. It counts only from 3 on, for a program that made no request to that host meanwhile.
- Anything else that failed (no TLS, no common version, a different name) is not a refusal.

A program is named by its TLS client (`tls_client`): from the User-Agent that the same client sent
on any connection that worked, in this record, on any earlier night; otherwise it is "a program"
with its client's name, since a program that never trusted Silo never sent a request.

A refusal lasts from its first line until the same program makes a request to that host, or until
30 minutes pass with no refusal: then it has ended, and a new refusal is a new alert
(`alert-only-on-a-change`). Refusals of one program on one host are one alert, with a count.
"""
import json, sys
from datetime import datetime, timedelta, timezone

STRONG = {"certificate-refused", "closed-after-certificate"}
WEAK = {"no-request"}
QUIET = timedelta(minutes=30)
WEAK_MIN = 3
STILL = timedelta(minutes=5)  # a last refusal this close to the end: the program was still at it

PRODUCTS = [  # User-Agent product -> how the program is named, and where its store is set
    ("claude-cli", "Claude Code", "NODE_EXTRA_CA_CERTS"), ("claude-code", "Claude Code", "NODE_EXTRA_CA_CERTS"),
    ("codex_", "Codex", "SSL_CERT_FILE or the system's store"), ("curl", "curl", "CURL_CA_BUNDLE"),
    ("git", "git", "GIT_SSL_CAINFO"), ("Python-urllib", "Python", "SSL_CERT_FILE"),
    ("python-requests", "Python requests", "REQUESTS_CA_BUNDLE"), ("Go-http-client", "a Go program", "SSL_CERT_FILE"),
    ("node", "Node", "NODE_EXTRA_CA_CERTS"), ("Bun", "Bun", "NODE_EXTRA_CA_CERTS"),
]

def program_of(agent):
    for prefix, name, fix in PRODUCTS:
        if agent.startswith(prefix):
            return name, fix
    return (agent.split("/")[0] or "a program"), None

def t(line):
    s = line["time"].rstrip("Z")
    if "." in s:
        a, b = s.split(".")
        s = a + "." + b[:6]
    return datetime.fromisoformat(s).replace(tzinfo=timezone.utc)

def hm(d):
    return d.strftime("%H:%M")

def read(lines, now=None):
    names = {}  # tls_client -> (name, fix)
    for l in lines:
        if l.get("kind") == "connection" and l.get("what") == "closed" and l.get("requests") and l.get("tls_client") and l.get("agent"):
            names.setdefault(l["tls_client"], program_of(l["agent"]))

    def who(fp):
        return names.get(fp, (f"a program (TLS client {fp})", None))

    open_ = {}  # (computer, host, name) -> episode
    done, alerts = [], []
    for l in lines:
        if l.get("kind") != "connection" or l.get("what") != "closed":
            continue
        when, comp, host, fp = t(l), l.get("computer", "?"), l.get("host", ""), l.get("tls_client", "")
        name, fix = who(fp)
        key = (comp, host, name)
        ep = open_.get(key)
        if ep and when - ep["last"] > QUIET:
            done.append(open_.pop(key)); ep = None
        why = l.get("why")
        if l.get("requests"):  # the program made a request to that host: any refusal of it ended
            if ep:
                ep["ended_by"] = when; done.append(open_.pop(key))
            continue
        if why not in STRONG and why not in WEAK:
            continue
        if not ep:
            ep = open_[key] = {"computer": comp, "host": host, "program": name, "fix": fix, "first": when, "last": when,
                               "strong": 0, "weak": 0, "alerts": set(), "alerted": False, "ended_by": None}
        ep["last"] = when
        ep["strong" if why in STRONG else "weak"] += 1
        if l.get("alert"):
            ep["alerts"].add(l["alert"])
        if not ep["alerted"] and (ep["strong"] or ep["weak"] >= WEAK_MIN):
            ep["alerted"] = True
            alerts.append({"time": when.isoformat(), "computer": comp, "program": name, "host": host,
                           "text": f"{name} on {comp} does not trust Silo's certificate for {host}: nothing it sends there gets through."})
    done.extend(open_.values())
    shown = [e for e in done if e["alerted"]]
    end = now or max((t(l) for l in lines if "time" in l), default=None)
    morning = []
    for e in sorted(shown, key=lambda e: e["first"]):
        n = e["strong"] + e["weak"]
        still = e["ended_by"] is None and end is not None and end - e["last"] <= STILL
        span = f"at {hm(e['first'])}" if hm(e["first"]) == hm(e["last"]) else f"from {hm(e['first'])} to {hm(e['last'])}"
        state = "still refusing at the end" if still else (f"it worked again at {hm(e['ended_by'])}" if e["ended_by"] else "then it stopped trying")
        how = ("refusal" if n == 1 else "refusals") if e["strong"] else "connections closed before any request"
        line = f"{e['program']} on {e['computer']} did not trust Silo's certificate for {e['host']}: {n} {how} {span}, {state}."
        if e["fix"] and still:
            line += f" Give it Silo's authority ({e['fix']})."
        elif not e["fix"]:
            line += " If it pins the host's own key, it cannot work through a host Silo reads."
        morning.append(line)
    return alerts, morning

def main():
    if sys.argv[1:] == ["--test"]:
        return test()
    now = None
    if "--now" in sys.argv:
        now = datetime.fromisoformat(sys.argv[sys.argv.index("--now") + 1].replace("Z", "+00:00"))
    lines = [json.loads(x) for x in open(sys.argv[1]) if x.strip()]
    alerts, morning = read(lines, now)
    print("Alerts, as they would have come:")
    for a in alerts:
        print(f"  {a['time'][11:19]}  {a['text']}")
    print("The morning:")
    for m in morning:
        print(f"  {m}")

def test():
    base = datetime(2026, 11, 25, 2, 0, tzinfo=timezone.utc)
    def L(minutes, **k):
        d = {"kind": "connection", "what": "closed", "computer": "main", "host": "api.openai.com", "time": (base + timedelta(minutes=minutes)).isoformat().replace("+00:00", "Z")}
        d.update(k); return d
    ok = dict(requests=1, tls_client="A", agent="codex_exec/0.172.0")
    ref = dict(why="certificate-refused", alert="unknown CA", tls_client="A")
    # Learned name, one alert for many refusals, ended by a request, a second alert after.
    lines = [L(-600, **ok)] + [L(10 + i, **ref) for i in range(50)] + [L(70, **ok), L(80, **ref)]
    a, m = read(lines)
    assert [x["program"] for x in a] == ["Codex", "Codex"], a
    assert "50 refusals from 02:10 to 02:59, it worked again at 03:10" in m[0], m
    _, m2 = read([L(0, **ref), L(1, **ref)], now=base + timedelta(minutes=20))
    assert "then it stopped trying" in m2[0] and "Give it" not in m2[0], m2
    assert "still refusing at the end" in m[1] and "SSL_CERT_FILE" in m[1], m
    # A quiet half hour ends a refusal: two alerts.
    a, _ = read([L(0, **ref), L(31, **ref)])
    assert len(a) == 2, a
    # A client never seen working is named by its TLS client.
    a, _ = read([L(0, why="closed-after-certificate", tls_client="B")])
    assert a[0]["program"] == "a program (TLS client B)", a
    # Closed before any request: no alert under 3, one alert at 3; other failures never alert.
    a, _ = read([L(0, why="no-request", tls_client="C"), L(1, why="no-request", tls_client="C")])
    assert a == [], a
    a, _ = read([L(i, why="no-request", tls_client="C") for i in range(3)])
    assert len(a) == 1, a
    a, _ = read([L(i, why=w, tls_client="D") for i, w in enumerate(["no-common-tls", "no-tls-hello", "name-differs", "no-hello-in-time"])])
    assert a == [], a
    # One program on two hosts, and two computers: separate alerts.
    a, _ = read([L(0, **ref), L(0, host="chatgpt.com", **ref), L(0, computer="fork", **ref)])
    assert len(a) == 3, a
    print("refusals.py: 8 checks passed")

if __name__ == "__main__":
    main()
