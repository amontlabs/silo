"""The precaution that holds destructive calls, as a process of its own beside the boundary.

It runs #353's detector with #383's rules (`../../2026-11-25-destructive-calls-platforms/proto/
detector/`), unchanged, and stands in for the person who answers a hold. The boundary keeps every
limit (see `boundary/src/hold.zig`); this process only gives an opinion, then an answer or none.

usage: SOCK=<unix socket> [ANSWER=yes|no|none] [ANSWER_MS=<delay>] [CLASSIFY_DELAY_MS=<delay>]
       [LOG=<jsonl>] python3 precaution.py
ANSWER is the stand-in person's answer to every hold (default none: no answer comes). The log
holds one line per request: the verdict, the detector's time, and nothing of the request's body.
"""
import base64, json, os, socket, sys, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
DET = os.path.normpath(os.path.join(HERE, "..", "..", "2026-11-25-destructive-calls-platforms", "proto", "detector"))
sys.path.insert(0, DET)
import engine  # noqa: E402  (#383's subclass of #353's detector)

D = engine.load(os.path.join(DET, "rules.json"))
ANSWER = os.environ.get("ANSWER", "none")
ANSWER_MS = int(os.environ.get("ANSWER_MS", "0"))
CLASSIFY_DELAY_MS = int(os.environ.get("CLASSIFY_DELAY_MS", "0"))
LOG = open(os.environ["LOG"], "a", buffering=1) if os.environ.get("LOG") else None
lock = threading.Lock()


def serve(conn):
    with conn:
        f = conn.makefile("rb")
        line = f.readline()
        if not line:
            return
        q = json.loads(line)
        t0 = time.perf_counter()
        r = D.check({"host": q["host"], "method": q["method"], "target": q["target"],
                     "headers": q["headers"], "body": base64.b64decode(q["body_b64"]), "facts": {}})
        ms = (time.perf_counter() - t0) * 1000
        if CLASSIFY_DELAY_MS:
            time.sleep(CLASSIFY_DELAY_MS / 1000)
        out = {"decision": r["decision"], "verdict": r["verdict"], "says": r["says"]}
        if LOG:
            with lock:
                LOG.write(json.dumps({"computer": q["computer"], "host": q["host"], "method": q["method"],
                                      "verdict": r["verdict"], "decision": r["decision"], "detector_ms": round(ms, 3)}) + "\n")
        try:
            conn.sendall((json.dumps(out) + "\n").encode())
            if r["decision"] == "hold" and ANSWER in ("yes", "no"):
                time.sleep(ANSWER_MS / 1000)
                conn.sendall((json.dumps({"answer": ANSWER}) + "\n").encode())
            elif r["decision"] == "hold":
                conn.recv(1)  # no answer: wait until the boundary ends the hold
        except OSError:
            pass


def main():
    path = os.environ["SOCK"]
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(path)
    s.listen(512)
    if os.environ.get("READY"):
        open(os.environ["READY"], "w").close()
    while True:
        c, _ = s.accept()
        threading.Thread(target=serve, args=(c,), daemon=True).start()


if __name__ == "__main__":
    main()
