#!/usr/bin/env python3
"""Silo v1 phase 0 VM check against the Dev engine (~/.silo-dev/engine.sock).

Run: python3 docs/v1/checks/phase0_check.py 2>&1 | tee docs/v1/evidence/phase-0/vm-check.txt

Creates only computers named e2e-p0-*; every mutation goes through the engine API
(protocol 7). Never touches ~/.silo or ~/.local/bin/msb. Stdlib only.
"""
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time
import uuid

HOME = os.path.expanduser("~")
DEV_ROOT = os.path.realpath(os.path.join(HOME, ".silo-dev"))
SOCK = os.path.join(DEV_ROOT, "engine.sock")
MSB_HOME = os.path.join(DEV_ROOT, "c19a79dc92d3")
REPO = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
MSB = os.path.join(REPO, "app/SiloUI/target/debug/bundle/macos/Silo Dev.app/Contents/MacOS/msb")
MAIN, FORK = "e2e-p0-main", "e2e-p0-fork"
MB = 1024 * 1024
T0 = time.time()
RESULTS = {}


def log(msg=""):
    stamp = time.strftime("%H:%M:%S", time.localtime()) + f" +{time.time() - T0:7.1f}s"
    print(f"[{stamp}] {msg}", flush=True)


# ---------------------------------------------------------------- engine client


def _frame(sock, value):
    data = json.dumps(value).encode()
    sock.sendall(struct.pack(">I", len(data)) + data)


def _recv(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("engine closed the connection")
        buf += chunk
    return buf


def _read(sock):
    (n,) = struct.unpack(">I", _recv(sock, 4))
    return json.loads(_recv(sock, n))


class EngineError(Exception):
    pass


class Engine:
    def __init__(self, path=SOCK, name="phase0-check", timeout=1800):
        real = os.path.realpath(path)
        if os.path.commonpath([real, DEV_ROOT]) != DEV_ROOT:
            raise SystemExit(f"refusing socket outside {DEV_ROOT}: {real}")
        self.path, self.name, self.timeout = path, name, timeout
        self.lock = threading.Lock()
        self.connect()

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.path)
        _frame(self.sock, {"id": "hs", "method": "engine.handshake", "params": {
            "protocol": {"min": 8, "max": 8}, "client": {"name": self.name, "version": "0"}}})
        reply = _read(self.sock)
        if "error" in reply:
            raise EngineError(json.dumps(reply["error"]))
        self.handshake = reply["result"]

    def call(self, method, params=None):
        with self.lock:
            rid = str(uuid.uuid4())
            _frame(self.sock, {"id": rid, "method": method, "params": params or {}})
            while True:
                msg = _read(self.sock)
                if msg.get("id") == rid:
                    if "error" in msg:
                        raise EngineError(f"{method}: {json.dumps(msg['error'])}")
                    return msg.get("result")

    # jobs: poll operations.job.get (events are not needed)
    def job_ids(self):
        q = self.call("operations.list")
        ids = list(q.get("unacknowledgedJobIds", []))
        ids += [e["jobId"] for e in q.get("running", []) + q.get("waiting", []) if e.get("jobId")]
        return set(ids)

    def wait_job(self, job_id, timeout=1800):
        end = time.time() + timeout
        while time.time() < end:
            res = self.call("operations.job.get", {"jobId": job_id})
            if res["state"] == "finished":
                outcome = res.get("outcome") or {}
                if outcome.get("error"):
                    raise EngineError(f"job {job_id} failed: {json.dumps(outcome['error'])}")
                try:
                    self.call("operations.job.acknowledge", {"jobId": job_id})
                except EngineError:
                    pass
                return outcome.get("result")
            time.sleep(0.5)
        raise EngineError(f"job {job_id} did not finish in {timeout}s")

    def run_job(self, method, params):
        res = self.call(method, params)
        return self.wait_job(res["jobId"])

    def wait_follow_up(self, before_ids, started_ids, timeout=600):
        """Wait for the checkpoints.reclaim job allocated after a capture: the one new job id."""
        end = time.time() + timeout
        while time.time() < end:
            new = self.job_ids() - before_ids - started_ids
            if new:
                job = sorted(new, key=lambda j: int(j.split(":")[1]) if j.startswith("job:") else 0)[0]
                started_ids.add(job)
                return job, self.wait_job(job)
            time.sleep(0.5)
        raise EngineError("no follow-up reclaim job appeared")

    def computers(self):
        return self.call("computers.list")["linux"]["computers"]

    def computer(self, name):
        for c in self.computers():
            if c["configuration"]["name"] == name:
                return c
        return None

    def exec(self, name, argv, timeout_s=170, check=True):
        cid = self.computer(name)["configuration"]["id"]
        res = self.call("guest.exec", {"computerId": cid, "argv": argv, "timeoutMs": timeout_s * 1000})
        out = bytes(res["stdout"]).decode(errors="replace")
        err = bytes(res["stderr"]).decode(errors="replace")
        if check and res["exitCode"] != 0:
            raise EngineError(f"exec {argv} exit {res['exitCode']}: {err.strip()[:300]}")
        return res["exitCode"], out, err


# ---------------------------------------------------------------- status poller


class Poller(threading.Thread):
    """Calls computers.list every 250 ms on its own connection; records every failure."""

    def __init__(self):
        super().__init__(daemon=True)
        self.stop_flag = threading.Event()
        self.calls = 0
        self.failures = []
        self.recovered = 0
        self.failed_ops = []

    def run(self):
        eng = None
        while not self.stop_flag.is_set():
            started = time.time()
            try:
                if eng is None:
                    eng = Engine(name="phase0-poller", timeout=30)
                comps = eng.computers()
                self.calls += 1
                for c in comps:
                    op = c.get("checkpointOperation")
                    if c["configuration"]["name"].startswith("e2e-p0-") and op and op.get("status") == "failed":
                        item = (c["configuration"]["name"], op)
                        if item not in self.failed_ops:
                            self.failed_ops.append(item)
                            log(f"POLL: checkpoint operation failed on {item[0]}: {json.dumps(op)}")
            except Exception as error:  # every failure is recorded
                self.calls += 1
                self.failures.append((time.time() - T0, repr(error)))
                log(f"POLL FAILURE #{len(self.failures)}: {error!r}")
                if not isinstance(error, EngineError):  # connection-level: reconnect
                    try:
                        eng = Engine(name="phase0-poller", timeout=30)
                        self.recovered += 1
                    except Exception as again:
                        eng = None
                        log(f"POLL reconnect failed: {again!r}")
            self.stop_flag.wait(max(0, 0.25 - (time.time() - started)))


# ---------------------------------------------------------------- helpers


def host_free_kb():
    st = os.statvfs("/")
    return st.f_bavail * st.f_frsize // 1024


def df_line():
    out = subprocess.run(["df", "-k", "/"], capture_output=True, text=True).stdout.strip().splitlines()[-1]
    return out


def dir_kb(path):
    if not os.path.exists(path):
        return 0
    out = subprocess.run(["du", "-sk", path], capture_output=True, text=True).stdout.split()
    return int(out[0]) if out else 0


def settle(seconds=5):
    time.sleep(seconds)


def snap(label):
    settle()
    free = host_free_kb()
    cache = dir_kb(os.path.join(MSB_HOME, "cache/memory/snapshots"))
    log(f"HOST FREE [{label}]: {free} KiB avail ({free / 1024:.0f} MiB)  | memory cache {cache // 1024} MiB | {df_line()}")
    return free


def guest_df(eng, name):
    _, out, _ = eng.exec(name, ["df", "-k", "/"])
    line = out.strip().splitlines()[-1]
    log(f"GUEST df -k / ({name}): {line}")
    return line


def msb_env():
    env = dict(os.environ)
    env["MSB_HOME"] = MSB_HOME
    return env


def msb_snapshot_list():
    if not os.path.exists(MSB):
        return "(msb binary missing)"
    r = subprocess.run([MSB, "snapshot", "list"], capture_output=True, text=True, env=msb_env(), timeout=60)
    return (r.stdout + r.stderr).strip()


def snapshot_dirs():
    base = os.path.join(MSB_HOME, "snapshots")
    return sorted(os.listdir(base)) if os.path.isdir(base) else []


def verdict(step, ok, detail):
    RESULTS[step] = (ok, detail)
    log(f"=== {step}: {'PASS' if ok else 'FAIL'} - {detail}")


def checkpoint_rows(comp):
    return [(c["name"], c["kind"], c["scope"], c["reason"], c["id"][:8]) for c in comp.get("checkpoints", [])]


def usage(eng, cid):
    u = eng.call("checkpoints.usage", {"computerId": cid})
    rows = {e["id"]: e.get("sizeBytes") for e in u["checkpoints"]}
    return u.get("totalBytes"), rows


class GuestProbe(threading.Thread):
    """guest.exec `true` every 2 s on its own connection; records latency and failures."""

    def __init__(self, cid):
        super().__init__(daemon=True)
        self.cid, self.stop_flag = cid, threading.Event()
        self.samples, self.failures = [], []

    def run(self):
        try:
            eng = Engine(name="phase0-probe", timeout=120)
        except Exception as error:
            self.failures.append(repr(error))
            return
        while not self.stop_flag.is_set():
            t = time.time()
            try:
                eng.call("guest.exec", {"computerId": self.cid, "argv": ["true"], "timeoutMs": 60000})
                self.samples.append(time.time() - t)
            except Exception as error:
                self.failures.append(f"+{t - T0:.1f}s {error!r} after {time.time() - t:.1f}s")
            self.stop_flag.wait(2)

    def finish(self):
        self.stop_flag.set()
        self.join(130)
        lat = sorted(self.samples)
        if lat:
            return (f"{len(lat)} ok probes, latency median {lat[len(lat) // 2] * 1000:.0f} ms, "
                    f"max {lat[-1] * 1000:.0f} ms, {len(self.failures)} failures {self.failures[:3]}")
        return f"0 ok probes, {len(self.failures)} failures {self.failures[:3]}"


def cache_mb():
    return dir_kb(os.path.join(MSB_HOME, "cache/memory/snapshots")) // 1024


def dump_logs(eng, cid):
    for source in ("runtime", "system"):
        try:
            page = eng.call("logs.query", {"computerId": cid, "source": source, "limit": 60})
            log(f"logs.query {source}: {page.get('totalMatches')} matches; last entries:")
            for entry in page.get("entries", [])[-40:]:
                log("   " + json.dumps(entry)[:400])
        except Exception as error:
            log(f"logs.query {source} failed: {error!r}")
    log(f"cache/memory/snapshots du: {dir_kb(os.path.join(MSB_HOME, 'cache/memory/snapshots'))} KiB; "
        f"sandboxes du: {dir_kb(os.path.join(MSB_HOME, 'sandboxes'))} KiB")


def write_data(eng, name, home, fname, mb):
    d = f"{home}/p0"
    eng.exec(name, ["mkdir", "-p", d])
    t = time.time()
    cid = eng.computer(name)["configuration"]["id"]
    eng.run_job("guest.exec.start", {
        "computerId": cid, "stdinOpen": False, "retainOutputBytes": 65536,
        "argv": ["sh", "-c", f"set -e; dd if=/dev/urandom of={d}/{fname} bs=1M count={mb} conv=fsync 2>/dev/null; "
                             f"cd {d}; sha256sum {fname} > {fname}.sha; sync"]})
    eng.exec(name, ["sh", "-c", f"cd {d} && sha256sum -c {fname}.sha && ls -l {fname}"], timeout_s=170)
    log(f"wrote {mb} MB to {d}/{fname} in {name} ({time.time() - t:.1f}s)")


def start_and_wait(eng, name):
    eng.run_job("computers.start", {"computerId": eng.computer(name)["configuration"]["id"]})
    for _ in range(120):
        if eng.computer(name)["state"] == "running":
            return
        time.sleep(1)
    raise EngineError(f"{name} did not reach running")


def stop_and_wait(eng, name):
    eng.run_job("computers.stop", {"computerId": eng.computer(name)["configuration"]["id"]})
    for _ in range(120):
        if eng.computer(name)["state"] == "stopped":
            return
        time.sleep(1)
    raise EngineError(f"{name} did not reach stopped")


def create_linux(eng, name):
    cfg = {"id": str(uuid.uuid4()), "name": name, "cpus": 8, "maxCPUs": 12, "memoryGiB": 32,
           "maxMemoryGiB": 48, "workspaceStorageGiB": 120, "runtimeStorageGiB": 100,
           "desktop": {"startWithComputer": False, "builtIn": True}}
    res = eng.call("computers.create", {"kind": "linux", "configuration": cfg})
    eng.wait_job(res["jobId"])
    return eng.computer(name)


def delete_computer(eng, name):
    comp = eng.computer(name)
    if comp["state"] != "stopped":
        stop_and_wait(eng, name)
        comp = eng.computer(name)
    eng.run_job("computers.delete", {"computerId": comp["configuration"]["id"], "expected": comp["configuration"]})
    for _ in range(60):
        if eng.computer(name) is None:
            return
        time.sleep(1)
    raise EngineError(f"{name} still listed after delete")


# ---------------------------------------------------------------- header


def header():
    def sh(*cmd, **kw):
        return subprocess.run(cmd, capture_output=True, text=True, cwd=REPO, **kw).stdout.strip()

    inputs = json.load(open(os.path.join(REPO, "app/SiloUI/runtime-inputs.json")))
    log("Silo v1 phase 0 VM check")
    log(f"engine commit : {sh('git', 'rev-parse', 'HEAD')} (branch {sh('git', 'rev-parse', '--abbrev-ref', 'HEAD')})")
    log(f"msb version   : {inputs['microsandboxVersion']} with {len(inputs.get('patches', []))} patches "
        f"(runtime-inputs.json; source {inputs['sourceCommit'][:12]})")
    if os.path.exists(MSB):
        log(f"msb --version : {sh(MSB, '--version')} (bundle {MSB})")
    log(f"macOS         : {sh('sw_vers', '-productVersion')} build {sh('sw_vers', '-buildVersion')} ({sh('uname', '-m')})")
    log(f"socket        : {SOCK}; MSB home {MSB_HOME}")


# ---------------------------------------------------------------- the check


def main():
    header()
    eng = Engine()
    log(f"handshake: device {eng.handshake['name']} protocol {eng.handshake['protocol']}")
    existing = [c["configuration"]["name"] for c in eng.computers()]
    log(f"existing computers (untouched): {existing}")
    for n in (MAIN, FORK):
        if n in existing:
            raise SystemExit(f"{n} already exists; refusing to continue")
    log(f"pre-existing snapshot dirs: {snapshot_dirs()}")
    pre_dirs = set(snapshot_dirs())
    log(f"cache/memory/snapshots at start: {cache_mb()} MiB, files {sorted(f[:14] for f in os.listdir(os.path.join(MSB_HOME, 'cache/memory/snapshots')) if f.endswith('.ram'))}")

    settings = eng.call("settings.read")["settings"]
    old_bk = settings.get("beforeKeepCheckpointRetentionCount")
    log(f"beforeKeepCheckpointRetentionCount before: {old_bk!r} (absent means default 5)")

    poller = Poller()
    poller.start()
    main_created = False
    base_free = host_free_kb()
    try:
        # ---------------- step 2
        log("##### STEP 2: create, write, checkpoint, fork")
        base_free = snap("before create")
        comp = create_linux(eng, MAIN)
        main_created = True
        mid = comp["configuration"]["id"]
        log(f"created {MAIN} id {mid} state {comp['state']}")
        start_and_wait(eng, MAIN)
        home = json.loads(json.dumps(eng.call("guest.account", {"computerId": mid})))["home"]
        log(f"guest home {home}")
        guest_df(eng, MAIN)
        write_data(eng, MAIN, home, "base300.bin", 300)
        free_written = snap("main running, 300 MB written")
        known = eng.job_ids()
        t = time.time()
        res = eng.call("checkpoints.create", {"computerId": mid, "name": "p0-base"})
        created = eng.wait_job(res["jobId"])
        base_ckpt = created["checkpoint"]["id"]
        log(f"checkpoint p0-base {base_ckpt} created in {time.time() - t:.1f}s")
        free_before_fork = snap("before fork (after p0-base)")
        fork_ok, fork_detail = True, ""
        try:
            t = time.time()
            eng.run_job("checkpoints.fork", {"computerId": mid, "checkpointId": base_ckpt, "newName": FORK})
            log(f"fork created in {time.time() - t:.1f}s")
            fid = eng.computer(FORK)["configuration"]["id"]
            start_and_wait(eng, FORK)
            _, out, _ = eng.exec(FORK, ["sh", "-c", f"cd {home}/p0 && sha256sum -c base300.bin.sha && ls -l base300.bin"])
            log(f"fork data check: {out.strip()}")
            write_data(eng, FORK, home, "fork200.bin", 200)
            free_fork_running = snap("fork running, 200 MB written in fork")
            guest_df(eng, FORK)
            stop_and_wait(eng, FORK)
            delete_computer(eng, FORK)
            free_after_fork = snap("after fork delete")
            diff_mb = (free_after_fork - free_before_fork) / 1024
            fork_ok = abs(diff_mb) <= 20
            fork_detail = (f"free before fork {free_before_fork // 1024} MiB, fork running +200MB "
                           f"{free_fork_running // 1024} MiB, after delete {free_after_fork // 1024} MiB "
                           f"(after-before {diff_mb:+.0f} MiB); fork saw base data")
        except Exception as error:
            fork_ok, fork_detail = False, f"error: {error}"
            log(f"STEP 2 error: {error!r}")
        verdict("step2 fork", fork_ok, fork_detail)
        # drop the base checkpoint; it is a manual one and is not part of the retention test
        try:
            eng.run_job("checkpoints.delete", {"computerId": mid, "checkpointId": base_ckpt})
            log("deleted p0-base checkpoint")
        except Exception as error:
            log(f"could not delete p0-base: {error!r}")

        # ---------------- step 3
        log("##### STEP 3: before-keep retention (count 2)")
        set_setting(eng, 2)
        step3_ok, step3_detail, retained_ids = True, "", []
        try:
            start_free = snap("step 3 start")
            sizes_at_create = []
            known_jobs = set()
            reclaim_failures = []
            reclaim_durations = []
            for i in range(1, 6):
                write_data(eng, MAIN, home, f"bk{i}.bin", 150)
                free_pre = snap(f"bk{i} before create")
                before_ids = eng.job_ids()
                res = eng.call("checkpoints.create", {"computerId": mid, "name": f"p0-bk-{i}", "kind": "before-keep"})
                known_jobs.add(res["jobId"])
                created = eng.wait_job(res["jobId"])
                cid_i = created["checkpoint"]["id"]
                log(f"bk{i} created {cid_i[:8]}; waiting for checkpoints.reclaim follow-up job")
                probe = GuestProbe(mid)
                probe.start()
                t_rec = time.time()
                try:
                    rj, rres = eng.wait_follow_up(before_ids, known_jobs)
                    dur = time.time() - t_rec
                    log(f"reclaim job {rj} finished in {dur:.1f}s: {json.dumps(rres)}")
                    reclaim_durations.append((i, round(dur, 1)))
                except EngineError as error:
                    reclaim_durations.append((i, round(time.time() - t_rec, 1)))
                    reclaim_failures.append((i, str(error)))
                    log(f"RECLAIM FAILURE after bk{i}: {error}")
                    dump_logs(eng, mid)
                log(f"guest probe during reclaim bk{i}: {probe.finish()}")
                free_post = snap(f"bk{i} after create + reclaim")
                comp = eng.computer(MAIN)
                rows = checkpoint_rows(comp)
                total, per = usage(eng, mid)
                log(f"checkpoints after bk{i}: {rows}")
                log(f"usage after bk{i}: total {total} bytes; per-checkpoint {json.dumps({k[:8]: v for k, v in per.items()})}")
                log(f"checkpoint operation: {json.dumps(comp.get('checkpointOperation'))}")
                sizes_at_create.append((i, cid_i, per.get(cid_i), free_pre - free_post))
            comp = eng.computer(MAIN)
            bk = [c for c in comp["checkpoints"] if c["kind"] == "before-keep"]
            names = sorted(c["name"] for c in bk)
            retained_ids = [c["id"] for c in sorted(bk, key=lambda c: c["createdAt"])]
            final_free = host_free_kb()
            total, per = usage(eng, mid)
            cost_sum = sum(s or 0 for _, _, s, _ in sizes_at_create)
            df_cost_sum = sum(d for _, _, _, d in sizes_at_create) * 1024
            log("per-checkpoint (id, usage size at creation bytes, host free drop create+reclaim KiB): "
                + json.dumps([(i, c[:8], s, d) for i, c, s, d in sizes_at_create]))
            written_kb = 5 * 150 * 1024
            expected_untrimmed = start_free - written_kb - cost_sum // 1024
            log(f"start free {start_free // 1024} MiB; wrote 750 MiB; sum of per-checkpoint usage {cost_sum / MB:.0f} MiB; "
                f"final free {final_free // 1024} MiB; retained usage now {(total or 0) / MB:.0f} MiB")
            log(f"estimated free with all 5 kept (start - 750 MiB - sum of sizes): {expected_untrimmed // 1024} MiB; "
                f"actual {final_free // 1024} MiB; difference {(final_free - expected_untrimmed) / 1024:+.0f} MiB")
            ok_names = names == ["p0-bk-4", "p0-bk-5"]
            ok_gain = (final_free - expected_untrimmed) / 1024 > 20
            no_fail = not reclaim_failures and not poller.failed_ops and not (comp.get("checkpointOperation") or {}).get("status") == "failed"
            step3_ok = ok_names and ok_gain and no_fail
            step3_detail = (f"before-keep kept {names}; final free {final_free // 1024} MiB vs ~{expected_untrimmed // 1024} MiB "
                            f"if all 5 kept ({(final_free - expected_untrimmed) / 1024:+.0f} MiB); reclaim durations {reclaim_durations}; reclaim failures: "
                            f"{len(reclaim_failures)} {reclaim_failures}; failed ops seen {len(poller.failed_ops)}; sum of per-checkpoint usage {cost_sum / MB:.0f} MiB vs retained "
                            f"{(total or 0) / MB:.0f} MiB (df-measured create+reclaim drops summed {df_cost_sum / MB:.0f} MiB)")
        except Exception as error:
            step3_ok, step3_detail = False, f"error: {error!r}"
            log(f"STEP 3 error: {error!r}")
        verdict("step3 retention", step3_ok, step3_detail)

        # ---------------- step 4
        log("##### STEP 4: restore newest retained checkpoint")
        step4_ok, step4_detail = False, ""
        try:
            if not retained_ids:
                bk = [c for c in eng.computer(MAIN)["checkpoints"] if c["kind"] == "before-keep"]
                retained_ids = [c["id"] for c in sorted(bk, key=lambda c: c["createdAt"])]
            if not retained_ids:
                raise EngineError("no before-keep checkpoint to restore")
            newest = retained_ids[-1]
            newest_name = next(c["name"] for c in eng.computer(MAIN)["checkpoints"] if c["id"] == newest)
            n = int(newest_name.rsplit("-", 1)[1])
            write_data(eng, MAIN, home, "after.bin", 10)
            stop_and_wait(eng, MAIN)
            t = time.time()
            eng.run_job("checkpoints.restore", {"computerId": mid, "checkpointId": newest})
            log(f"restore finished in {time.time() - t:.1f}s")
            if eng.computer(MAIN)["state"] != "running":
                start_and_wait(eng, MAIN)
            _, out, _ = eng.exec(MAIN, ["sh", "-c", f"cd {home}/p0 && sha256sum -c bk1.bin.sha base300.bin.sha 2>&1; ls -l"], check=False)
            log(f"restored guest listing:\n{out}")
            code, _, _ = eng.exec(MAIN, ["sh", "-c", f"cd {home}/p0 && " + " && ".join(f"sha256sum -c bk{k}.bin.sha" for k in range(1, n + 1)) + " && sha256sum -c base300.bin.sha"], check=False)
            gone, _, _ = eng.exec(MAIN, ["test", "-e", f"{home}/p0/after.bin"], check=False)
            step4_ok = code == 0 and gone != 0
            step4_detail = (f"restore of newest retained {newest_name} ok; earlier data verified (sha256 rc={code}), "
                            f"after.bin {'absent' if gone != 0 else 'STILL PRESENT'}")
        except Exception as error:
            step4_detail = f"error: {error!r}"
            log(f"STEP 4 error: {error!r}")
        verdict("step4 restore", step4_ok, step4_detail)
    finally:
        # ---------------- step 5 (always clean up what this run created)
        log("##### STEP 5: delete main and check leftovers")
        step5_ok, step5_detail = False, ""
        try:
            if main_created and eng.computer(MAIN) is not None:
                free_pre_delete = snap("before delete main")
                if eng.computer(FORK) is not None:
                    delete_computer(eng, FORK)
                delete_computer(eng, MAIN)
                free_post_delete = snap("after delete main")
                log(f"free after delete vs start: {(free_post_delete - base_free) / 1024:+.0f} MiB "
                    f"(start {base_free // 1024} MiB, after {free_post_delete // 1024} MiB)")
            listing = msb_snapshot_list()
            log(f"msb snapshot list:\n{listing}")
            dirs = snapshot_dirs()
            new_dirs = sorted(set(dirs) - pre_dirs)
            mine = [d for d in dirs if d.startswith((MAIN, FORK))]
            log(f"snapshot dirs now: {dirs}; new since start: {new_dirs}")
            sandbox_dirs = [d for d in os.listdir(os.path.join(MSB_HOME, "sandboxes")) if d.startswith("e2e-p0-")]
            log(f"sandbox dirs named e2e-p0-*: {sandbox_dirs}")
            leftovers = [l for l in listing.splitlines() if MAIN in l or FORK in l] + mine + sandbox_dirs + new_dirs
            remaining = [c["configuration"]["name"] for c in eng.computers()]
            log(f"computers now: {remaining}")
            step5_ok = not leftovers and MAIN not in remaining and FORK not in remaining
            step5_detail = f"leftovers: {leftovers or 'none'}; pre-existing e2e-p0-* dirs from earlier runs untouched: " \
                           f"{sorted(d for d in pre_dirs if d.startswith('e2e-p0-'))}"
        except Exception as error:
            step5_detail = f"error: {error!r}"
            log(f"STEP 5 error: {error!r}")
        verdict("step5 delete", step5_ok, step5_detail)

        try:
            set_setting(eng, old_bk if old_bk is not None else 5)
            log(f"restored beforeKeepCheckpointRetentionCount to {old_bk if old_bk is not None else 5} "
                f"(was {'absent/default 5' if old_bk is None else old_bk})")
        except Exception as error:
            log(f"COULD NOT RESTORE SETTING: {error!r}")
        settle(8)
        log(f"cache/memory/snapshots at end: {cache_mb()} MiB, files {sorted(f[:14] for f in os.listdir(os.path.join(MSB_HOME, 'cache/memory/snapshots')) if f.endswith('.ram'))}")
        poller.stop_flag.set()
        poller.join(5)
        pf = len(poller.failures)
        verdict("step1 polls", pf == 0,
                f"{poller.calls} computers.list polls at 250 ms, {pf} failures, {poller.recovered} reconnects; "
                f"failures: {poller.failures[:5]}; failed checkpoint ops seen: {poller.failed_ops}")
        log("##### SUMMARY")
        for k, (ok, detail) in sorted(RESULTS.items()):
            log(f"{k}: {'PASS' if ok else 'FAIL'} - {detail}")


def set_setting(eng, value):
    eng.call("settings.update", {"patch": {"beforeKeepCheckpointRetentionCount": value}})


if __name__ == "__main__":
    main()
