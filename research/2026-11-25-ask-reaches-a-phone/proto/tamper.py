"""What a hostile relay, or anyone who can post to it, can do to the ask path. Each case holds a
request, sends something through the relay, and checks what the Mac did with it. The relay is
the stand-in in relay.py, turned hostile through its /_evil switch.

    python3 tamper.py <outdir>       prints one line per case and writes tamper.json
"""
import json, os, subprocess, sys, threading, time, urllib.request
import link

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]; os.makedirs(OUT, exist_ok=True)
REC = f"{OUT}/record.jsonl"; open(REC, "w").close()
PAIR = link.pair(f"{OUT}/pairing.json")
RP, BP = 8931, 8932
R = f"http://127.0.0.1:{RP}"
env = dict(os.environ, PYTHONPATH=HERE, RELAY=R, PAIRING=f"{OUT}/pairing.json")
procs = [subprocess.Popen([sys.executable, f"{HERE}/relay.py"], env=dict(env, PORT=str(RP))),
         subprocess.Popen([sys.executable, f"{HERE}/askd.py"], env=dict(env, PORT=str(BP), RECORD=REC, GATHER="0.2",
                          HOLDS=json.dumps({"default": 8})))]
time.sleep(1)
results = []


def post(topic, blob):
    urllib.request.urlopen(urllib.request.Request(f"{R}/t/{topic}", data=blob, method="POST"), timeout=5)


def evil(**kv):
    urllib.request.urlopen(urllib.request.Request(f"{R}/_evil", data=json.dumps(kv).encode(), method="POST"), timeout=5)


def lines():
    return [json.loads(l) for l in open(REC)]


def latest_version(ask):
    v = [l for l in lines() if l["kind"] == "ask.version" and l["ask"] == ask]
    return v[-1]


def held_ask():
    """The ask the last held request went to. Unanswered asks stay open, so with one open ask per
    computer each case adds a line to the same ask until an answer closes it."""
    return [l for l in lines() if l["kind"] in ("request.held", "request.joined")][-1]["ask"]


def curl(path, body="x=1", hold_out=None):
    """Sends a request in the background; returns a dict filled with its exit and output."""
    r = {}
    def go():
        p = subprocess.run(["curl", "-s", "-o", "-", "-w", "\n%{http_code}", "-d", body, f"http://127.0.0.1:{BP}{path}"],
                           capture_output=True, text=True, env={"PATH": os.environ["PATH"]})
        r["code"] = p.stdout.strip().split("\n")[-1]; r["out"] = p.stdout
    t = threading.Thread(target=go); t.start(); r["thread"] = t
    time.sleep(0.6)
    return r


def answer(ask, version=None, verdict="yes", key=None, list_hash=None, issued=None, expires=None, nonce=None):
    v = latest_version(ask) if version is None else [l for l in lines() if l["kind"] == "ask.version" and l["ask"] == ask and l["version"] == version][0]
    m = {"type": "answer", "ask": ask, "version": v["version"], "list_hash": list_hash or v["list_hash"],
         "verdict": verdict, "nonce": nonce or os.urandom(16).hex(),
         "issued": issued or time.time(), "expires": expires or time.time() + 600}
    return link.seal(m, key or PAIR["phone"]["sign"], PAIR["mac"]["box_pub"]), m


def refused_since(n):
    return [l.get("why") for l in lines()[n:] if l["kind"] == "answer.refused"]


def case(name, expect_pass, fn):
    n = len(lines())
    r = fn()
    r["thread"].join()
    passed = r["code"] == "200"
    results.append({"case": name, "request_passed": passed, "expected_pass": expect_pass,
                    "ok": passed == expect_pass, "mac_said": refused_since(n)})
    print(("OK  " if passed == expect_pass else "BAD ") + name, "| passed" if passed else "| refused", "|", refused_since(n))


attacker = link.new_side()


def c_forged():
    r = curl("/v1/charges", "amount=1")
    blob, _ = answer(held_ask(), key=attacker["sign"]); post(PAIR["to_mac"], blob); return r


def c_garbage():
    r = curl("/v1/charges", "amount=2")
    post(PAIR["to_mac"], os.urandom(300)); return r


def c_flip():
    r = curl("/v1/charges", "amount=3")
    evil(mode="flip", topic=PAIR["to_mac"])
    blob, _ = answer(held_ask()); post(PAIR["to_mac"], blob)
    time.sleep(0.5); evil(mode="honest"); return r


honest = {}


def c_honest():
    r = curl("/v1/charges", "amount=4")
    blob, m = answer(held_ask()); honest["blob"] = blob; post(PAIR["to_mac"], blob); return r


def c_replay_same_request():
    # The same charge again: a new ask, since ask_4's yes was spent. The relay replays the old yes.
    r = curl("/v1/charges", "amount=4")
    post(PAIR["to_mac"], honest["blob"]); return r


def c_wrong_list():
    r = curl("/v1/charges", "amount=6")
    blob, _ = answer(held_ask(), list_hash="0" * 32); post(PAIR["to_mac"], blob); return r


def c_stale():
    r = curl("/v1/charges", "amount=7")
    blob, _ = answer(held_ask(), issued=time.time() - 3600, expires=time.time() - 3000); post(PAIR["to_mac"], blob); return r


def c_delayed_past_expiry():
    r = curl("/v1/charges", "amount=8")
    evil(mode="delay", topic=PAIR["to_mac"], s=3)
    blob, _ = answer(held_ask(), expires=time.time() + 1.5); post(PAIR["to_mac"], blob)
    time.sleep(0.3); evil(mode="honest"); return r


def c_dropped():
    r = curl("/v1/charges", "amount=9")
    evil(mode="drop", topic=PAIR["to_mac"])
    blob, _ = answer(held_ask()); post(PAIR["to_mac"], blob)
    time.sleep(0.3); evil(mode="honest"); return r


def c_version_seen_only():
    # Two requests: the phone saw version 1 (one line) and says yes to it; the second, held after,
    # must not pass on that yes.
    r1 = curl("/v1/charges", "amount=10")
    time.sleep(0.5)
    blob, _ = answer(held_ask())
    r2 = curl("/v1/charges", "amount=999999")
    post(PAIR["to_mac"], blob)
    r1["thread"].join()
    results.append({"case": "yes to the version seen lets its own line pass", "request_passed": r1["code"] == "200",
                    "expected_pass": True, "ok": r1["code"] == "200", "mac_said": []})
    print(("OK  " if r1["code"] == "200" else "BAD ") + "yes to the version seen lets its own line pass")
    return r2


def c_phone_gets_forged_ask():
    # The relay makes up an ask for the phone. The phone app must refuse it: it is not the Mac's.
    blob = link.seal({"type": "ask", "ask": "ask_99", "version": 1, "lines": ["push main to github.com/you/site"],
                      "list_hash": "x", "sent": time.time(), "ends": time.time() + 60, "computer": "main"},
                     attacker["sign"], PAIR["phone"]["box_pub"])
    try:
        link.open_(blob, PAIR["phone"]["box"], PAIR["mac"]["sign_pub"]); ok = False; why = "accepted"
    except link.Refused as e:
        ok = True; why = str(e)
    results.append({"case": "phone refuses an ask the relay made up", "ok": ok, "phone_said": why})
    print(("OK  " if ok else "BAD ") + "phone refuses an ask the relay made up |", why)


case("answer signed by another key", False, c_forged)
case("bytes of the relay's own making", False, c_garbage)
case("one bit changed on the way", False, c_flip)
case("honest yes (control)", True, c_honest)
case("old yes replayed for the same request", False, c_replay_same_request)
case("yes to a list the Mac never sent", False, c_wrong_list)
case("answer an hour old", False, c_stale)
case("yes held by the relay past its expiry", False, c_delayed_past_expiry)
case("answer dropped by the relay", False, c_dropped)
case("yes to the version seen does not cover a line added after", False, c_version_seen_only)
c_phone_gets_forged_ask()
seen = json.load(urllib.request.urlopen(f"{R}/_seen"))
for p in procs: p.terminate()
json.dump({"results": results, "relay_saw_example": seen[:3], "relay_saw_fields": sorted(seen[0].keys())},
          open(f"{OUT}/tamper.json", "w"), indent=1)
print("relay could log only:", sorted(seen[0].keys()))
print(sum(r["ok"] for r in results), "of", len(results), "as expected")
