"""Silo's phone app, as far as the ask path needs it: it receives each version of an ask from the
relay, checks that the Mac sealed and signed it, and answers after a delay that stands in for the
person, with an answer signed by the phone's own key and bound to the exact list it showed.

Environment: RELAY, PAIRING, LOG (phone.jsonl), POLICY (JSON):
  {"delay": 20, "verdict": "yes"}         answer every ask 20 s after it first arrived
  {"delay": 20, "verdict": "yes", "only": ["ask_1"]}
  {"never": true}                          show, never answer
"""
import json, os, threading, time, urllib.request
import link

RELAY = os.environ.get("RELAY", "http://127.0.0.1:8810")
PAIR = json.load(open(os.environ.get("PAIRING", "pairing.json")))
POLICY = json.loads(os.environ.get("POLICY", '{"delay": 20, "verdict": "yes"}'))
logf = open(os.environ.get("LOG", "phone.jsonl"), "a")
latest = {}      # ask -> newest message received
scheduled = set()


def log(kind, **kv):
    logf.write(json.dumps({"t": round(time.time(), 4), "kind": kind, **kv}) + "\n"); logf.flush()


def answer(ask):
    m = latest[ask]
    msg = {"type": "answer", "ask": ask, "version": m["version"], "list_hash": m["list_hash"],
           "verdict": POLICY.get("verdict", "yes"), "nonce": os.urandom(16).hex(),
           "issued": time.time(), "expires": time.time() + 600}
    blob = link.seal(msg, PAIR["phone"]["sign"], PAIR["mac"]["box_pub"])
    urllib.request.urlopen(urllib.request.Request(f"{RELAY}/t/{PAIR['to_mac']}", data=blob, method="POST"), timeout=10)
    log("answered", ask=ask, version=m["version"], verdict=msg["verdict"], lines=len(m["lines"]))


def main():
    since = 0
    while True:
        try:
            msgs = json.load(urllib.request.urlopen(f"{RELAY}/t/{PAIR['to_phone']}?since={since}&wait=20", timeout=30))
        except Exception as e:
            log("relay.error", err=str(e)); time.sleep(1); continue
        for r in msgs:
            since = max(since, r["seq"])
            try:
                m = link.open_(link.unb64(r["b"]), PAIR["phone"]["box"], PAIR["mac"]["sign_pub"])
            except link.Refused as e:
                log("refused", why=str(e)); continue
            log("shown", ask=m["ask"], version=m["version"], lines=m["lines"], sent=m["sent"],
                in_transit=round(time.time() - m["sent"], 4))
            latest[m["ask"]] = m
            if POLICY.get("never") or m["ask"] in scheduled:
                continue
            if POLICY.get("only") and m["ask"] not in POLICY["only"]:
                continue
            scheduled.add(m["ask"])
            threading.Timer(POLICY.get("delay", 20), answer, (m["ask"],)).start()


if __name__ == "__main__":
    main()
