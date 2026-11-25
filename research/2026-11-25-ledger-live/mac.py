"""One Mac: its agents, and its boundary admitting their model calls against the team's budget.

The agents are the simulator's (`../2026-11-25-team-ledger/sim/sim.py`, `Team`): Claude Code with a
main conversation of 40 steps that starts 8 subagents of 14 steps at step 2 ("heavy"), 2 subagents
("medium") or none ("light"), a tool call of 0.2 to 1 s between steps. Each step is a real streamed
Anthropic Messages call to #385's stand-in provider, which bills it to its own ledger.jsonl, cut
calls for what was generated. Each call asks the provider for 2,000 to 12,000 tokens of reasoning
(`THINK=`), as the simulator draws.

The boundary prices a call before it leaves from its request alone (input estimated at 3.6 bytes
a token, the part sent before as a cache read), holds it with #374's rule through the way the team
shares the budget (`../2026-11-25-team-ledger/ledger/client.py`), counts its output by time while it
runs, settles it on the usage the provider reports, and cuts calls under way when the way says so.
A cut call is settled on its input and its output counted by time, which is all a boundary has.

Writes one JSON line per call, then a summary line, to --out.
"""
import argparse, http.client, json, os, random, socket, ssl, sys, threading, time, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-25-team-ledger", "ledger"))
from book import Book  # noqa: E402
import client as ways  # noqa: E402

# Opus 5.5 at #374's prices, dollars per token.
P_IN, P_OUT, P_CR, P_CW = 4e-6, 20e-6, 0.2e-6, 5e-6
B_IN, B_OUT = 2e-6, 10e-6  # its batch tier
THINK = (2000, 12000)
TYPICAL_OUT = THINK[0] + 0.9 * (THINK[1] - THINK[0]) + 120  # the simulator's typical output
KINDS = {"heavy": 8, "medium": 2, "light": 0}
MODEL = "claude-opus-5-5"
TOOLS = [{"name": "Bash", "description": "Run a shell command.", "input_schema": {"type": "object", "properties": {"command": {"type": "string"}}}}]


def filler(tokens, ratio=3.8, seed=0):
    """Text that the stand-in's tokenizer counts as about `tokens` tokens."""
    words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu".split()
    r = random.Random(seed)
    out, n = [], 0
    while n < tokens * ratio:
        w = r.choice(words)
        out.append(w)
        n += len(w) + 1
    return " ".join(out)


class Provider:
    def __init__(self, port, cafile):
        self.port = port
        self.ctx = ssl.create_default_context(cafile=cafile)
        self.ctx.check_hostname = False

    def conn(self, timeout=120):
        return http.client.HTTPSConnection("127.0.0.1", self.port, context=self.ctx, timeout=timeout)


class Mac:
    def __init__(self, a):
        self.a = a
        self.rng = random.Random(a.seed * 1000 + a.mac)
        self.rlock = threading.Lock()
        self.provider = Provider(a.provider_port, a.cafile)
        self.out = open(a.out, "a")
        self.olock = threading.Lock()
        self.running = {}  # call id -> the connection, to cut it
        self.cut_flag = set()
        self.rmu = threading.Lock()
        self.cid = 0
        self.live = 0
        self.lmu = threading.Lock()
        self.all_done = threading.Event()
        self.counted = 0.0  # what this boundary settled
        policy = a.policy
        if a.way == "local":
            self.way = ways.Local(a.budget, policy, a.tps)
        else:
            h = ways.Http("127.0.0.1", a.ledger_port, timeout=a.timeout)
            if a.way == "central":
                self.way = ways.Central(h, "team", f"m{a.mac}", on_down=a.on_down)
            else:
                self.way = ways.Lease(h, "team", f"m{a.mac}", policy, a.tps, rtt=a.rtt, margin=a.margin, keep=a.keep, rearm=not a.no_rearm)
        self.way.on_cut = self.cut_all
        self.book_rule = Book(0, policy, a.tps, P_OUT)  # only for reserve_for

    def rand(self, lo, hi, kind="int"):
        with self.rlock:
            return self.rng.randint(lo, hi) if kind == "int" else self.rng.uniform(lo, hi)

    def log(self, o):
        with self.olock:
            self.out.write(json.dumps(o) + "\n")
            self.out.flush()

    # ---- cutting ----

    def cut_all(self):
        with self.rmu:
            conns = list(self.running.items())
            for cid, _ in conns:
                self.cut_flag.add(cid)
        for _, sock in conns:
            try:
                sock.shutdown(socket.SHUT_RDWR)
            except Exception:
                pass

    def watch(self):
        while not self.all_done.is_set():
            time.sleep(0.5)
            if self.a.cut and self.way.check_cut():
                self.cut_all()

    # ---- one model call ----

    def call(self, conv, who):
        new = self.rand(200, 600)
        think = self.rand(*THINK)
        conv["n"] += 1
        text = filler(new, 3.8, seed=zlib.crc32(f"{who}/{conv['n']}".encode())) + f" THINK={think}"
        conv["msgs"].append({"role": "user", "content": [{"type": "text", "text": text, "cache_control": {"type": "ephemeral"}}]})
        body = {"model": MODEL, "max_tokens": 128000, "stream": True, "system": conv["system"], "tools": TOOLS, "messages": conv["msgs"]}
        raw = json.dumps(body).encode()
        # The boundary's estimate: the part sent before is a cache read, the rest a cache write.
        est = len(raw) / 3.6
        read = min(conv["prev_total"], est)
        inp = read * P_CR + (est - read) * P_CW
        with self.lmu:
            self.cid += 1
            cid = f"m{self.a.mac}-{self.cid}"
        res = self.book_rule.reserve_for(inp, TYPICAL_OUT)
        t0 = time.time()
        ok = self.way.admit(cid, inp, res)
        t1 = time.time()
        if not ok:
            self.log({"call": cid, "who": who, "refused": True, "t": t1 - self.a.t0, "est_input": inp})
            return False
        c = self.provider.conn()
        usage, out_tokens, err = None, None, None
        try:
            c.connect()
        except OSError as e:
            err = type(e).__name__
        with self.rmu:
            # The socket itself: http.client lets go of it once a response says `connection: close`.
            self.running[cid] = c.sock
            cut_early = cid in self.cut_flag
        try:
            if cut_early or err:
                raise OSError("cut before it left")
            c.request("POST", "/v1/messages", raw, {"content-type": "application/json", "x-api-key": "sk-ant-api-standin",
                                                    "anthropic-version": "2023-06-01", "x-silo-call": cid})
            r = c.getresponse()
            while True:
                line = r.fp.readline()
                if not line:
                    break
                if line.startswith(b"data: "):
                    ev = json.loads(line[6:])
                    if ev.get("type") == "message_start":
                        usage = ev["message"]["usage"]
                    elif ev.get("type") == "message_delta":
                        out_tokens = ev["usage"]["output_tokens"]
                    elif ev.get("type") == "message_stop":
                        break
        except (OSError, http.client.HTTPException, ValueError) as e:
            err = type(e).__name__
        finally:
            with self.rmu:
                self.running.pop(cid, None)
                was_cut = cid in self.cut_flag
                self.cut_flag.discard(cid)
            try:
                c.close()
            except Exception:
                pass
        t2 = time.time()
        if usage is not None:
            cin = usage["input_tokens"] * P_IN + usage.get("cache_creation_input_tokens", 0) * P_CW + usage.get("cache_read_input_tokens", 0) * P_CR
            conv["prev_total"] = usage["input_tokens"] + usage.get("cache_creation_input_tokens", 0) + usage.get("cache_read_input_tokens", 0)
        else:
            cin = inp
        done = out_tokens is not None and not was_cut
        if done:
            cost = cin + out_tokens * P_OUT
        else:
            # Cut, or dropped: the output counted by time, all a boundary knows.
            cost = cin + min((t2 - t1) * self.a.tps, 128000) * P_OUT
        self.way.settle(cid, cost)
        with self.lmu:
            self.counted += cost
        conv["msgs"].append({"role": "assistant", "content": [{"type": "text", "text": f"step {conv['n']} done"}]})
        # Drop the cache mark from the earlier user turn, as Claude Code keeps only the last.
        for m in conv["msgs"][:-2]:
            if m["role"] == "user":
                for b in m["content"]:
                    b.pop("cache_control", None)
        self.log({"call": cid, "who": who, "t": t1 - self.a.t0, "wait": t1 - t0, "took": t2 - t1, "est_input": inp, "input": cin,
                  "cost": cost, "done": done, "cut": was_cut, "err": err, "think": think})
        return done

    # ---- the agents ----

    def conv(self, who, sub):
        first = ("SUBAGENT: do your part of the task with the shell. " if sub else "Do the task with the shell. ")
        return {"system": [{"type": "text", "text": filler(15000, 3.8, seed=zlib.crc32(who.encode())), "cache_control": {"type": "ephemeral"}}],
                "msgs": [{"role": "user", "content": [{"type": "text", "text": first}]}, {"role": "assistant", "content": [{"type": "text", "text": "ok"}]}],
                "n": 0, "prev_total": 0}

    def started(self):
        with self.lmu:
            self.live += 1

    def ended(self):
        with self.lmu:
            self.live -= 1
            last = self.live == 0
        if last:
            self.way.done()
            self.all_done.set()

    def sub(self, who, join):
        c = self.conv(who, True)
        for _ in range(15):
            if not self.call(c, who):
                break
            time.sleep(self.rand(0.2, 1.0, "f"))
        self.ended()
        join.release()

    def main(self, member, subs):
        who = f"{member}-main"
        c = self.conv(who, False)
        for step in range(41):
            if not self.call(c, who):
                break
            if step == 2 and subs:
                join = threading.Semaphore(0)
                for k in range(subs):
                    self.started()
                    threading.Thread(target=self.sub, args=(f"{member}-sub{k}", join), daemon=True).start()
                for _ in range(subs):
                    join.acquire()
            time.sleep(self.rand(0.2, 1.0, "f"))
        self.ended()

    def batch(self, at, n, max_tokens, result_wait):
        """Work out of sight (#385): a batch held at its whole bound until its results come back."""
        time.sleep(max(0.0, self.a.t0 + at - time.time()))
        reqs = [{"custom_id": f"r{k}", "params": {"model": MODEL, "max_tokens": max_tokens,
                                                  "messages": [{"role": "user", "content": filler(400, seed=k) + " THINK=1400"}]}} for k in range(n)]
        raw = json.dumps({"requests": reqs}).encode()
        bound = sum(len(json.dumps(r["params"])) / 3.6 * B_IN + max_tokens * B_OUT for r in reqs)
        wid = f"batch-m{self.a.mac}"
        ok = self.way.admit_whole(wid, bound)
        self.log({"batch": wid, "bound": bound, "admitted": ok, "t": time.time() - self.a.t0})
        if ok:
            c = self.provider.conn()
            c.request("POST", "/v1/messages/batches", raw, {"content-type": "application/json", "x-api-key": "sk-ant-api-standin"})
            bid = json.loads(c.getresponse().read())["id"]
            c.close()
            while True:
                time.sleep(2)
                c = self.provider.conn()
                c.request("GET", f"/v1/messages/batches/{bid}")
                st = json.loads(c.getresponse().read())
                c.close()
                if st["processing_status"] == "ended":
                    break
            c = self.provider.conn()
            c.request("GET", f"/v1/messages/batches/{bid}/results")
            cost = 0.0
            for line in c.getresponse().read().decode().splitlines():
                u = json.loads(line)["result"]["message"]["usage"]
                cost += u["input_tokens"] * B_IN + u["output_tokens"] * B_OUT
            c.close()
            self.way.settle_whole(wid, cost)
            self.log({"batch": wid, "cost": cost, "t": time.time() - self.a.t0})
        self.ended()

    def run(self):
        a = self.a
        members = a.members.split(",")
        threading.Thread(target=self.watch, daemon=True).start()
        threads = []
        for k, m in enumerate(members):
            self.started()
            delay = random.Random(a.seed * 7919 + a.mac * 31 + k).uniform(0, a.stagger)
            def go(k=k, m=m, delay=delay):
                time.sleep(max(0.0, a.t0 + delay - time.time()))
                self.main(f"m{a.mac}.{k}", KINDS[m])
            threads.append(threading.Thread(target=go, daemon=True))
        if a.batch:
            at, n, mt, wait = json.loads(a.batch)
            self.started()
            threads.append(threading.Thread(target=self.batch, args=(at, n, mt, wait), daemon=True))
        for t in threads:
            t.start()
        self.all_done.wait()
        time.sleep(0.5)
        st = self.way.stats()
        w = sorted(st.pop("waits")) or [0.0]
        for tr in st.pop("trace", []):
            self.log(dict(tr, trace=True, t=tr["t"] - a.t0))
        self.log({"summary": True, "mac": a.mac, "counted": self.counted, "ended": time.time() - a.t0,
                  "wait_p50": w[len(w) // 2], "wait_p99": w[min(len(w) - 1, int(len(w) * 0.99))], "wait_max": w[-1],
                  "waited_10s": sum(1 for x in w if x > 10), "admitted": len(w), **st})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mac", type=int, required=True)
    ap.add_argument("--members", required=True, help="this Mac's members, comma-separated: heavy, medium, light")
    ap.add_argument("--way", choices=["local", "central", "lease"], required=True)
    ap.add_argument("--policy", default="reserve_typical")
    ap.add_argument("--cut", action="store_true")
    ap.add_argument("--budget", type=float, default=15.0, help="for --way local only")
    ap.add_argument("--tps", type=float, default=500.0)
    ap.add_argument("--rtt", type=float, default=0.03)
    ap.add_argument("--timeout", type=float, default=2.0)
    ap.add_argument("--margin", type=float, default=5.0)
    ap.add_argument("--on-down", default="hold")
    ap.add_argument("--keep", type=float, default=1.0, help="leases: room kept on a recall while calls run")
    ap.add_argument("--no-rearm", action="store_true", help="leases: the simulator's cut rule as it was")
    ap.add_argument("--ledger-port", type=int)
    ap.add_argument("--provider-port", type=int, required=True)
    ap.add_argument("--cafile", required=True)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--t0", type=float, required=True)
    ap.add_argument("--stagger", type=float, default=30.0)
    ap.add_argument("--batch", help="JSON [at, requests, max_tokens, result_wait]")
    ap.add_argument("--out", required=True)
    Mac(ap.parse_args()).run()


if __name__ == "__main__":
    main()
