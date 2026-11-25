"""Sets up two sshd servers, the boundary (silo_ssh.py) and a computer's view, then runs every case
twice: once with Silo's policy, once with plain agent forwarding. Prints one line per case.

Run as root on Linux with OpenSSH, git and Python's cryptography and paramiko installed:
    python3 run.py [workdir]
"""
import json, os, pwd, shutil, signal, subprocess, sys, time, base64, socket, threading

HERE = os.path.dirname(os.path.abspath(__file__))
W = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "/tmp/silo-ssh")
shutil.rmtree(W, ignore_errors=True); os.makedirs(W)
MAC, GUEST, SRV = f"{W}/mac", f"{W}/guest", f"{W}/servers"
for d in (MAC, GUEST, SRV): os.makedirs(d)
ENV = {k: v for k, v in os.environ.items() if not k.startswith("SSH_")}


def sh(cmd, **kw):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, **kw)


# servers ----------------------------------------------------------------------------------------
# One user "deploy" whose authorized key is the Mac's key. sshd A is the real host; sshd B has
# another host key, standing for a host that changed key, or a server the computer runs itself.
try: pwd.getpwnam("deploy")
except KeyError: sh("useradd -m -s /bin/sh deploy")
sh("usermod -p '*' deploy")  # no password, but not locked
os.makedirs("/run/sshd", exist_ok=True)
sh(f"ssh-keygen -q -t ed25519 -N '' -C 'polarzero@mac' -f {MAC}/id_ed25519")
for n in ("a", "b"): sh(f"ssh-keygen -q -t ed25519 -N '' -f {SRV}/host_{n}")
home = pwd.getpwnam("deploy").pw_dir
os.makedirs(f"{home}/.ssh", exist_ok=True)
shutil.copy(f"{MAC}/id_ed25519.pub", f"{home}/.ssh/authorized_keys")
sh(f"rm -rf {home}/repo.git && git init -q --bare -b main {home}/repo.git && chown -R deploy {home}")
sh(f"chmod 700 {home}/.ssh; chmod 600 {home}/.ssh/authorized_keys")
procs = []
for n, port in (("a", 2222), ("b", 2223)):
    conf = f"{SRV}/sshd_{n}.conf"
    open(conf, "w").write(f"""Port {port}
ListenAddress 127.0.0.1
HostKey {SRV}/host_{n}
PidFile {SRV}/sshd_{n}.pid
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
AllowAgentForwarding yes
UsePAM no
StrictModes no
LogLevel VERBOSE
""")
    procs.append(subprocess.Popen(["/usr/sbin/sshd", "-D", "-e", "-f", conf], stderr=open(f"{SRV}/sshd_{n}.log", "w")))


def pub(path): return open(path).read().split()[:2]


ha, hb = pub(f"{SRV}/host_a.pub"), pub(f"{SRV}/host_b.pub")

# the boundary -----------------------------------------------------------------------------------
# Its known_hosts is its own. rotated.example is known with host key A but is now served by B.
open(f"{MAC}/known_hosts", "w").write(
    f"git.example {ha[0]} {ha[1]}\nother.example {ha[0]} {ha[1]}\nrotated.example {ha[0]} {ha[1]}\n")
RELAYS = [{"name": "git.example", "listen_port": 2201, "upstream_host": "127.0.0.1", "upstream_port": 2222},
          {"name": "other.example", "listen_port": 2202, "upstream_host": "127.0.0.1", "upstream_port": 2222},
          {"name": "rotated.example", "listen_port": 2203, "upstream_host": "127.0.0.1", "upstream_port": 2223}]


def start_boundary(mode):
    cfg = {"mode": mode, "record": f"{W}/record-{mode}.jsonl", "agent_socket": f"{W}/agent-{mode}.sock",
           "known_hosts": f"{MAC}/known_hosts", "relays": RELAYS,
           "keys": [{"private_key_file": f"{MAC}/id_ed25519", "comment": "polarzero@mac",
                     "hosts": ["git.example", "rotated.example"], "sshsig_namespaces": ["git"]}]}
    json.dump(cfg, open(f"{W}/boundary-{mode}.json", "w"))
    p = subprocess.Popen([sys.executable, f"{HERE}/silo_ssh.py", f"{W}/boundary-{mode}.json"],
                         stdout=subprocess.PIPE, text=True)
    assert p.stdout.readline().strip() == "ready"
    return p, cfg


# the computer's view: an ssh config that names hosts by the names the boundary knows ---------------
# The computer's own known_hosts holds what it was given (host A for every name). It has no
# private key file: only the agent socket.
open(f"{GUEST}/known_hosts", "w").write(
    "".join(f"[{h}]:{p} {ha[0]} {ha[1]}\n" for h, p in (("127.0.0.1", 2201), ("127.0.0.1", 2202),
                                                          ("127.0.0.1", 2203), ("127.0.0.1", 2222)))
    + f"[127.0.0.1]:2223 {hb[0]} {hb[1]}\n")


def ssh_config(sock):
    return f"""Host git.example
  HostName 127.0.0.1
  Port 2201
Host other.example
  HostName 127.0.0.1
  Port 2202
Host rotated.example
  HostName 127.0.0.1
  Port 2203
Host *
  User deploy
  IdentityAgent {sock}
  IdentityFile none
  UserKnownHostsFile {GUEST}/known_hosts
  StrictHostKeyChecking yes
  BatchMode yes
  ConnectTimeout 5
"""


RESULTS = []


def case(mode, name, ok, detail=""):
    RESULTS.append((mode, name, ok, detail))
    print(f"{mode:5} {'works' if ok else 'fails'}  {name}  {detail}", flush=True)


def last_refusal(cfg, since):
    lines = [json.loads(l) for l in open(cfg["record"])][since:]
    r = [l["why"] for l in lines if l["kind"] == "ssh.agent_refused"]
    return r[0] if r else ""  # the first refusal: later ones follow from it


def nrec(cfg): return sum(1 for _ in open(cfg["record"]))


class AgentRelay(threading.Thread):
    """The relay attack's middle: an outsider's ssh-agent socket whose requests the computer
    passes, unchanged, to the agent it was given."""
    def __init__(self, path, target):
        super().__init__(daemon=True); self.path, self.target = path, target
        if os.path.exists(path): os.unlink(path)
        self.s = socket.socket(socket.AF_UNIX); self.s.bind(path); self.s.listen(8)
    def run(self):
        while True:
            c, _ = self.s.accept()
            u = socket.socket(socket.AF_UNIX); u.connect(self.target)
            for a, b in ((c, u), (u, c)):
                threading.Thread(target=self.pump, args=(a, b), daemon=True).start()
    @staticmethod
    def pump(a, b):
        try:
            while (d := a.recv(65536)): b.sendall(d)
        except OSError: pass
        finally:
            for x in (a, b):
                try: x.shutdown(socket.SHUT_RDWR)
                except OSError: pass


def run_cases(mode):
    p, cfg = start_boundary(mode)
    sock = cfg["agent_socket"]
    conf = f"{GUEST}/ssh_config-{mode}"; open(conf, "w").write(ssh_config(sock))
    S = f"ssh -F {conf}"
    env = dict(ENV, SSH_AUTH_SOCK=sock)

    def run(c, **kw): return sh(c, env=env, **kw)

    n = nrec(cfg); r = run(f"{S} git.example 'echo hello-from-server'")
    case(mode, "ssh to an allowed host, OpenSSH 9.6", "hello-from-server" in r.stdout, last_refusal(cfg, n))

    n = nrec(cfg); t = time.time()
    for _ in range(10): run(f"{S} git.example true")
    case(mode, "10 logins, mean seconds", True, f"{(time.time() - t) / 10:.3f}")

    n = nrec(cfg); r = run(f"{S} other.example true")
    case(mode, "ssh to a host not on the key's list", r.returncode == 0, last_refusal(cfg, n))

    n = nrec(cfg); r = run(f"{S} rotated.example true")
    case(mode, "host now answers with a key the boundary does not know", r.returncode == 0,
         last_refusal(cfg, n) or r.stderr.strip().splitlines()[-1:])

    # The computer accepts any host key (it is hostile, or careless): the boundary's own list decides.
    n = nrec(cfg); r = run(f"{S} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null rotated.example true")
    case(mode, "same, with the computer accepting any host key", r.returncode == 0, last_refusal(cfg, n))

    # A server the computer reaches without crossing the boundary: here sshd B on loopback, standing
    # for a server the computer runs itself, or a /etc/hosts entry it wrote.
    n = nrec(cfg); r = run(f"{S} -p 2223 127.0.0.1 true")
    case(mode, "login to a server reached without crossing the boundary", r.returncode == 0, last_refusal(cfg, n))

    # The relay attack: an outsider's ssh logs in to the real host directly (port 2222, not through
    # the boundary), with SSH_AUTH_SOCK pointing at a socket the computer relays to its agent.
    relay = AgentRelay(f"{W}/outsider-{mode}.sock", sock); relay.start()
    out_env = dict(ENV, SSH_AUTH_SOCK=relay.path)
    n = nrec(cfg)
    r = sh(f"ssh -o BatchMode=yes -o IdentityFile=none -o UserKnownHostsFile={GUEST}/known_hosts "
           f"-o IdentityAgent={relay.path} -p 2222 deploy@127.0.0.1 'echo outsider-in'", env=out_env)
    case(mode, "relay attack: an outsider logs in with the computer's agent", "outsider-in" in r.stdout,
         last_refusal(cfg, n))

    # The same outsider, whose connection the computer carries through the boundary: a tunnel. The
    # outsider holds the session keys; the boundary sees a real connection to an allowed host.
    n = nrec(cfg)
    r = sh(f"ssh -o BatchMode=yes -o IdentityFile=none -o UserKnownHostsFile={GUEST}/known_hosts "
           f"-o IdentityAgent={relay.path} -o HostKeyAlias=[127.0.0.1]:2201 "
           f"-o ProxyCommand='{sys.executable} {HERE}/tunnel.py 127.0.0.1 2201' deploy@git.example 'echo outsider-in'",
           env=out_env)
    case(mode, "relay attack through a tunnel the computer carries", "outsider-in" in r.stdout,
         last_refusal(cfg, n) or r.stderr.strip()[-100:])

    # A sign request with no session-bind, made by a program that speaks the agent protocol
    # itself: a guessed userauth blob for a session the outsider has.
    n = nrec(cfg)
    r = run(f"{sys.executable} {HERE}/raw_sign.py {sock}")
    case(mode, "raw userauth sign request with no bind", "signed" in r.stdout, last_refusal(cfg, n))

    # Git over SSH: clone, commit, push.
    repo = f"{GUEST}/work-{mode}"; sh(f"rm -rf {repo}")
    genv = dict(env, GIT_SSH_COMMAND=S, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1", GIT_AUTHOR_NAME="a", GIT_AUTHOR_EMAIL="a@x", GIT_COMMITTER_NAME="a",
                GIT_COMMITTER_EMAIL="a@x")
    n = nrec(cfg)
    r = sh(f"git clone -q git.example:repo.git {repo} && cd {repo} && git commit -q --allow-empty -m x-{mode} "
           f"&& git push -q origin HEAD:refs/heads/{mode}", env=genv)
    pushed = sh(f"git --git-dir={home}/repo.git rev-parse --verify -q refs/heads/{mode}").returncode == 0
    case(mode, "git clone and push over SSH", r.returncode == 0 and pushed, last_refusal(cfg, n) or r.stderr.strip()[-120:])

    # Commit signing with the key in the agent (gpg.format ssh), and the same key asked to sign a
    # file in another namespace.
    key_literal = "key::" + " ".join(pub(f"{MAC}/id_ed25519.pub"))
    n = nrec(cfg)
    r = sh(f"cd {repo} && git -c gpg.format=ssh -c user.signingkey='{key_literal}' commit -q -S --allow-empty -m signed "
           f"&& git cat-file commit HEAD | grep -c 'BEGIN SSH SIGNATURE'", env=genv)
    case(mode, "git commit -S with gpg.format=ssh", r.returncode == 0, last_refusal(cfg, n) or r.stderr.strip()[-120:])
    open(f"{GUEST}/note.txt", "w").write("x")
    open(f"{GUEST}/k.pub", "w").write(" ".join(pub(f"{MAC}/id_ed25519.pub")) + "\n")
    n = nrec(cfg)
    r = run(f"rm -f {GUEST}/note.txt.sig; ssh-keygen -q -Y sign -n file -f {GUEST}/k.pub -U {GUEST}/note.txt")
    case(mode, "ssh-keygen -Y sign in namespace 'file'", r.returncode == 0, last_refusal(cfg, n))

    n = nrec(cfg); r = run(f"{S} -A git.example 'echo first-hop; ssh-add -l >/dev/null 2>&1; echo agent=$?'")
    case(mode, "ssh -A: the first login", "first-hop" in r.stdout, r.stdout.split()[-1:])

    # Onward: ssh -A to the server, and from there ssh again (the next hop is outside the boundary).
    n = nrec(cfg)
    r = run(f"{S} -A git.example 'ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "
            f"-p 2222 deploy@127.0.0.1 echo hop2'")
    case(mode, "ssh -A, then ssh from the server onward", "hop2" in r.stdout, last_refusal(cfg, n))

    # Other SSH clients.
    n = nrec(cfg); r = run(f"{sys.executable} {HERE}/client_paramiko.py {sock} 2201")
    case(mode, "Paramiko client with the agent", "ok" in r.stdout, last_refusal(cfg, n) or r.stderr.strip()[-100:])
    n = nrec(cfg); r = run(f"{sys.executable} {HERE}/client_asyncssh.py {sock} 2201")
    case(mode, "asyncssh client with the agent", "ok" in r.stdout, last_refusal(cfg, n) or r.stderr.strip()[-100:])
    if os.path.exists(f"{W}/goclient"):
        n = nrec(cfg); r = run(f"{W}/goclient {sock} 127.0.0.1:2201")
        case(mode, "Go x/crypto/ssh client with the agent", "ok" in r.stdout,
             last_refusal(cfg, n) or r.stderr.strip()[-100:])

    # What the computer can read from the agent.
    r = run("ssh-add -L")
    case(mode, "ssh-add -L lists", True, r.stdout.strip()[:40] + "... " + r.stdout.strip().split()[-1])

    p.terminate(); p.wait()


# a Go client, if Go is there --------------------------------------------------------------------
if shutil.which("go"):
    d = f"{W}/go"; os.makedirs(d); shutil.copy(f"{HERE}/client_go.go", f"{d}/main.go")
    r = sh(f"cd {d} && go mod init goclient >/dev/null 2>&1 && go get golang.org/x/crypto/ssh@v0.45.0 >/dev/null 2>&1 "
           f"&& go mod tidy >/dev/null 2>&1 && go build -o {W}/goclient .")
    if r.returncode: print("go build failed:", r.stderr[-300:])

time.sleep(0.5)
try:
    for mode in ("silo", "silo+infer", "plain"):
        run_cases(mode)
finally:
    for p in procs: p.terminate()
json.dump(RESULTS, open(f"{W}/results.json", "w"), indent=1)
