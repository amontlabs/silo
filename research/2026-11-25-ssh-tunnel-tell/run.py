"""Can the boundary tell an SSH session the computer drives from one it only carries for an outsider?

Sets up one sshd (the real host, behind a delay line standing for the internet), the boundary
(boundary.py: relay, agent, timing), and runs:

  E1  two gaps the boundary can time: the key exchange, and the signature to the next packet, for local
      clients idle and loaded, and for an outsider carried through a tunnel at several round trips;
  E2  the shape of a session after login: how fast the client answers the server's data;
  E3  the deadline after each signature, against a plain outsider and one that sends decoys;
  E4  a host-key check the boundary answers, required before signing: does the tunnel forward it?

Run as root on Linux with OpenSSH, socat, stress-ng and Python's cryptography, paramiko and asyncssh:
    python3 run.py [workdir] [runs]
"""
import json, os, pwd, shutil, statistics, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
W = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "/tmp/silo-tell")
N = int(sys.argv[2]) if len(sys.argv) > 2 else 20
shutil.rmtree(W, ignore_errors=True); os.makedirs(W)
MAC, GUEST, SRV = f"{W}/mac", f"{W}/guest", f"{W}/servers"
for d in (MAC, GUEST, SRV): os.makedirs(d)
ENV = {k: v for k, v in os.environ.items() if not k.startswith("SSH_")}
PY = sys.executable
UPSTREAM_ONE_WAY_MS = 10  # the real host is 20 ms away, like GitHub from a home in Europe
OUT = open(f"{HERE}/results.txt", "w")
procs = []


def sh(cmd, **kw):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, **kw)


def say(*a):
    line = " ".join(str(x) for x in a)
    print(line, flush=True); OUT.write(line + "\n"); OUT.flush()


def bg(cmd, ready=True):
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    if ready: assert p.stdout.readline().strip() == "ready", cmd
    procs.append(p); return p


# the real host ----------------------------------------------------------------------------------
try: pwd.getpwnam("deploy")
except KeyError: sh("useradd -m -s /bin/sh deploy")
sh("usermod -p '*' deploy")
os.makedirs("/run/sshd", exist_ok=True)
sh(f"ssh-keygen -q -t ed25519 -N '' -C 'polarzero@mac' -f {MAC}/id_ed25519")
sh(f"ssh-keygen -q -t ed25519 -N '' -f {SRV}/host_a")
home = pwd.getpwnam("deploy").pw_dir
os.makedirs(f"{home}/.ssh", exist_ok=True)
shutil.copy(f"{MAC}/id_ed25519.pub", f"{home}/.ssh/authorized_keys")
sh(f"chown -R deploy {home}; chmod 700 {home}/.ssh; chmod 600 {home}/.ssh/authorized_keys")
open(f"{SRV}/sshd.conf", "w").write(f"""Port 2222
ListenAddress 127.0.0.1
HostKey {SRV}/host_a
PidFile {SRV}/sshd.pid
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
UsePAM no
StrictModes no
MaxStartups 100
""")
procs.append(subprocess.Popen(["/usr/sbin/sshd", "-D", "-e", "-f", f"{SRV}/sshd.conf"],
                              stderr=open(f"{SRV}/sshd.log", "w")))
bg([PY, f"{HERE}/delay.py", "tcp", str(UPSTREAM_ONE_WAY_MS), "2230", "127.0.0.1", "2222"])
ha = open(f"{SRV}/host_a.pub").read().split()[:2]
open(f"{MAC}/known_hosts", "w").write(f"git.example {ha[0]} {ha[1]}\n")
open(f"{GUEST}/known_hosts", "w").write(f"[127.0.0.1]:2201 {ha[0]} {ha[1]}\n")


# the boundary -----------------------------------------------------------------------------------
def boundary(tag, deadline_ms=None, hostcheck=False):
    cfg = {"mode": "silo+infer", "record": f"{W}/record-{tag}.jsonl", "trace": f"{W}/trace-{tag}.jsonl",
           "agent_socket": f"{W}/agent-{tag}.sock", "known_hosts": f"{MAC}/known_hosts", "deadline_ms": deadline_ms,
           "relays": [{"name": "git.example", "listen_port": 2201, "upstream_host": "127.0.0.1",
                       "upstream_port": 2230}],
           "keys": [{"private_key_file": f"{MAC}/id_ed25519", "comment": "polarzero@mac",
                     "hosts": ["git.example"]}]}
    if hostcheck: cfg["hostcheck_socket"] = f"{W}/hostcheck-{tag}.sock"
    json.dump(cfg, open(f"{W}/boundary-{tag}.json", "w"))
    p = bg([PY, f"{HERE}/boundary.py", f"{W}/boundary-{tag}.json"])
    return p, cfg


def traces(cfg, since=0):
    time.sleep(0.3)  # a connection's trace is written when it closes
    if not os.path.exists(cfg["trace"]): return []
    return [json.loads(l) for l in open(cfg["trace"])][since:]


def ntrace(cfg): return len(traces(cfg))


def refusals(cfg, since):
    return [json.loads(l) for l in open(cfg["record"])][since:]


def nrec(cfg): return sum(1 for _ in open(cfg["record"]))


# clients ----------------------------------------------------------------------------------------
def local_cmd(client, sock, cmd="echo ok"):
    S = (f"ssh -o BatchMode=yes -o IdentityFile=none -o IdentityAgent={sock} -o StrictHostKeyChecking=yes "
         f"-o UserKnownHostsFile={GUEST}/known_hosts -p 2201 deploy@127.0.0.1")
    return {"openssh": f"{S} '{cmd}'",
            "openssh-x25519": f"{S} -o KexAlgorithms=curve25519-sha256 '{cmd}'",
            "paramiko": f"{PY} {HERE}/client_paramiko.py {sock} 2201",
            "asyncssh": f"{PY} {HERE}/client_asyncssh.py {sock} 2201",
            "go": f"{W}/goclient {sock} 127.0.0.1:2201"}[client]


def outsider_cmd(one_way_ms, agent_path, cmd="echo outsider-in", extra=""):
    """An outsider's own OpenSSH: its TCP connection is carried by a program in the computer (the
    tunnel), and its agent requests are passed to the computer's agent the same way. Both cross the
    outsider's link, one_way_ms each way."""
    return (f"ssh -o BatchMode=yes -o IdentityFile=none -o IdentityAgent={agent_path} "
            f"-o UserKnownHostsFile={GUEST}/known_hosts -o HostKeyAlias=[127.0.0.1]:2201 {extra} "
            f"-o ProxyCommand='{PY} {HERE}/delay.py stdio {one_way_ms} 127.0.0.1 2201' deploy@git.example '{cmd}'")


def outsider_agent(cfg, one_way_ms, tag):
    path = f"{W}/outsider-{tag}-{one_way_ms}.sock"
    bg([PY, f"{HERE}/delay.py", "unix", str(one_way_ms), path, cfg["agent_socket"]])
    return path


def auth_gap(t):
    """Time from the agent's signature to the next packet the client sends: the signed request
    has to reach whoever holds the session keys, and come back encrypted."""
    signed = [a[0] for a in t.get("agent", []) if a[1] == "signed"]
    if not signed: return None
    nxt = [c[0] for c in t["trace"] if c[1] == "c>s" and c[0] > signed[-1]]
    return round(nxt[0] - signed[-1], 3) if nxt else None


def stats(xs):
    xs = sorted(x for x in xs if x is not None)
    if not xs: return "none"
    q = lambda p: xs[min(len(xs) - 1, int(p * len(xs)))]
    return f"n={len(xs):2} min={xs[0]:7.2f} p50={q(.5):7.2f} p95={q(.95):7.2f} max={xs[-1]:7.2f}"


def go_build():
    if not shutil.which("go"): return False
    d = f"{W}/go"; os.makedirs(d); shutil.copy(f"{HERE}/client_go.go", f"{d}/main.go")
    r = sh(f"cd {d} && go mod init goclient >/dev/null 2>&1 && go get golang.org/x/crypto/ssh@v0.45.0 >/dev/null 2>&1 "
           f"&& go mod tidy >/dev/null 2>&1 && go build -o {W}/goclient .")
    if r.returncode: say("go build failed:", r.stderr[-300:])
    return r.returncode == 0


CLIENTS = ["openssh", "openssh-x25519", "paramiko", "asyncssh"] + (["go"] if go_build() else [])
OUTSIDER_ONE_WAY = [0, 1, 2.5, 5, 10, 25]


def stress(on):
    if on:
        return subprocess.Popen(["stress-ng", "--cpu", str(2 * os.cpu_count()), "--cpu-method", "matrixprod",
                                 "--quiet"])
    return None



def decoy_cmd(one_way_ms, agent_path, decoys=1):
    return (f"{PY} {HERE}/attacker_decoy.py {agent_path} "
            f"'{PY} {HERE}/delay.py stdio {one_way_ms} 127.0.0.1 2201' {decoys}")


def cuts(cfg, since):
    return [x for x in refusals(cfg, since) if x["kind"] == "ssh.cut"]


try:
    # E1: two gaps, measured only -------------------------------------------------------------
    p, cfg = boundary("measure")
    env = dict(ENV, SSH_AUTH_SOCK=cfg["agent_socket"])
    say(f"# E1: milliseconds, {N} logins each. kex = the boundary hands over the server's KEX_ECDH_REPLY -> "
        f"first encrypted client packet. sign = the agent hands over the signature -> next client packet. "
        f"Real host {2 * UPSTREAM_ONE_WAY_MS} ms round trip from the boundary. {os.cpu_count()} CPUs; "
        f"'loaded' runs stress-ng with {2 * os.cpu_count()} CPU workers.")
    rows = {}
    for load in ("idle", "loaded"):
        s = stress(load == "loaded"); time.sleep(1 if s else 0)
        for client in CLIENTS:
            n0, ok = ntrace(cfg), 0
            for _ in range(N): ok += "ok" in sh(local_cmd(client, cfg["agent_socket"]), env=env).stdout
            ts = traces(cfg, n0); rows[(client, load)] = ts
            say(f"local {client:14} {load:6} logins={ok:2}/{N}  kex  {stats([t.get('kex_gap_ms') for t in ts])}")
            say(f"{'':41}  sign {stats([auth_gap(t) for t in ts])}")
        if s: s.terminate(); s.wait()
    for d in OUTSIDER_ONE_WAY:
        ag = outsider_agent(cfg, d, "measure")
        n0, ok = ntrace(cfg), 0
        for _ in range(N): ok += "outsider-in" in sh(outsider_cmd(d, ag), env=ENV).stdout
        ts = traces(cfg, n0); rows[("outsider", d)] = ts
        say(f"outsider set rtt={2 * d:5.1f} ms        logins={ok:2}/{N}  kex  {stats([t.get('kex_gap_ms') for t in ts])}")
        say(f"{'':41}  sign {stats([auth_gap(t) for t in ts])}")

    # E2: after login, how fast the client answers the server's data ----------------------------
    say("\n# E2: after login, during `head -c 4000000 /dev/zero`: for each client packet, the time since the "
        "last server packet. Milliseconds.")
    for who, d in (("local openssh", None), ("outsider rtt=2", 1), ("outsider rtt=10", 5)):
        n0 = ntrace(cfg)
        if d is None:
            sh(local_cmd("openssh", cfg["agent_socket"], "head -c 4000000 /dev/zero"), env=env)
        else:
            ag = outsider_agent(cfg, d, "shape")
            sh(outsider_cmd(d, ag, "head -c 4000000 /dev/zero"), env=ENV)
        t = traces(cfg, n0)[0]
        start = [c[0] for c in t.get("agent", [])][-1]
        last_s, gaps = None, []
        for tt, dr, nb in t["trace"]:
            if tt <= start: continue
            if dr == "s>c": last_s = tt
            elif last_s is not None: gaps.append(tt - last_s)
        say(f"{who:16} client packets={len(gaps):4}  {stats(gaps)}")
    p.terminate(); p.wait()

    # E3: the deadline after each signature ----------------------------------------------------
    dl = float(os.environ.get("DEADLINE_MS", "5"))
    say(f"\n# E3: after the agent signs, a client packet arriving later than {dl} ms, before the server "
        f"has said anything since, closes the connection unforwarded. {N} logins each.")
    p, cfg = boundary("deadline", deadline_ms=dl)
    env = dict(ENV, SSH_AUTH_SOCK=cfg["agent_socket"])
    for load in ("idle", "loaded"):
        s = stress(load == "loaded"); time.sleep(1 if s else 0)
        for client in CLIENTS:
            n0 = nrec(cfg)
            ok = sum("ok" in sh(local_cmd(client, cfg["agent_socket"]), env=env).stdout for _ in range(N))
            c = cuts(cfg, n0)
            say(f"local {client:14} {load:6} logged in {ok:2}/{N}  cut {len(c):2}"
                + (f" (late by {max(x['after_signature_ms'] for x in c):.2f} ms at most)" if c else ""))
        if s: s.terminate(); s.wait()
    for d in OUTSIDER_ONE_WAY:
        ag = outsider_agent(cfg, d, "deadline")
        n0 = nrec(cfg)
        ok = sum("outsider-in" in sh(outsider_cmd(d, ag), env=ENV).stdout for _ in range(N))
        say(f"outsider OpenSSH set rtt={2 * d:5.1f} ms  logged in {ok:2}/{N}  cut {len(cuts(cfg, n0)):2}")
    for d in OUTSIDER_ONE_WAY:
        ag = outsider_agent(cfg, d, "decoy")
        n0 = nrec(cfg)
        ok = sum("outsider-in" in sh(decoy_cmd(d, ag), env=ENV).stdout for _ in range(N))
        say(f"outsider decoy   set rtt={2 * d:5.1f} ms  logged in {ok:2}/{N}  cut {len(cuts(cfg, n0)):2}")
    n0 = nrec(cfg)
    sh(outsider_cmd(5, outsider_agent(cfg, 5, "why")), env=ENV)
    c = cuts(cfg, n0)
    say("the fact written:", json.dumps(c[0]) if c else "none")
    p.terminate(); p.wait()

    # E4: a host-key check the boundary answers, required before it signs ----------------------
    say("\n# E4: the computer's OpenSSH checks the host key by asking the boundary (KnownHostsCommand); "
        "the agent signs only for a connection whose host key was checked that way.")
    p, cfg = boundary("hostcheck", hostcheck=True)
    env = dict(ENV, SSH_AUTH_SOCK=cfg["agent_socket"])
    hc = cfg["hostcheck_socket"]
    khc = f"-o UserKnownHostsFile=/dev/null -o KnownHostsCommand=\"/bin/sh -c 'echo %H %t %K | socat - UNIX-CONNECT:{hc}'\""
    r = sh(f"ssh -o BatchMode=yes -o IdentityFile=none -o IdentityAgent={cfg['agent_socket']} {khc} "
           f"-p 2201 deploy@127.0.0.1 'echo ok'", env=env)
    say(f"local openssh with the boundary's check: {'works' if 'ok' in r.stdout else 'fails ' + r.stderr[-200:]}")
    n0 = nrec(cfg)
    r = sh(local_cmd("openssh", cfg["agent_socket"]), env=env)
    why = [x["why"] for x in refusals(cfg, n0) if x["kind"] == "ssh.agent_refused"]
    say(f"local openssh without it: {'works' if 'ok' in r.stdout else 'refused: ' + (why[0] if why else r.stderr[-100:])}")
    hc_out = f"{W}/outsider-hostcheck.sock"
    bg([PY, f"{HERE}/delay.py", "unix", "5", hc_out, hc])
    ag = outsider_agent(cfg, 5, "hostcheck")
    khc_out = f"-o UserKnownHostsFile=/dev/null -o KnownHostsCommand=\"/bin/sh -c 'echo %H %t %K | socat - UNIX-CONNECT:{hc_out}'\""
    r = sh(outsider_cmd(5, ag, extra=khc_out), env=ENV)
    say(f"outsider rtt=10 ms passing the check through its tunnel: {'logged in' if 'outsider-in' in r.stdout else 'fails ' + r.stderr[-200:]}")
    p.terminate(); p.wait()
finally:
    for x in procs:
        try: x.terminate()
        except Exception: pass
