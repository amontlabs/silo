"""From #380, with metadata-ping added. Inside the computer: every probe of the suite, as root, from a namespace whose only interface is
gvforwarder's tap0. Prints one line per case: <case> <result>. Results are 'reached' or 'blocked'
(and for scans, the open ports). Addresses come from the environment set by run.sh."""
import os, socket, subprocess, sys, asyncio, random, string
E = os.environ
def tcp(a, p, t=3):
    try:
        c = socket.create_connection((a, p), t); c.settimeout(t); c.sendall(b"hi\n"); d = c.recv(200); c.close()
        return "reached" if d else "blocked"
    except Exception:
        return "blocked"
def udp(a, p, t=2):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(t)
    try:
        s.sendto(b"probe", (a, p)); d, _ = s.recvfrom(100); return "reached" if d.startswith(b"echo:") else "blocked"
    except Exception:
        return "blocked"
def ping(a):
    r = subprocess.run(["ping", "-c", "1", "-W", "2", a], capture_output=True)
    return "reached" if r.returncode == 0 else "blocked"
async def one(a, p, sem, out):
    async with sem:
        try:
            r, w = await asyncio.wait_for(asyncio.open_connection(a, p), 0.5); out.append(p); w.close()
        except Exception:
            pass
def scan(a):
    out = []
    async def go():
        sem = asyncio.Semaphore(800); await asyncio.gather(*(one(a, p, sem, out) for p in range(1, 65536)))
    asyncio.run(go()); return ",".join(map(str, sorted(out))) or "none"
def http(method, url, data=None):
    cmd = ["curl", "-s", "-m", "3", "-o", "/dev/null", "-w", "%{http_code}", "-X", method, url]
    if data: cmd += ["-d", data]
    r = subprocess.run(cmd, capture_output=True, text=True); code = r.stdout.strip()
    return "reached" if code not in ("", "000") else "blocked"
def dns(name):
    try:
        return socket.gethostbyname(name)
    except Exception:
        return "none"
cases = [
    ("gateway-ports", lambda: scan("192.168.127.1")),
    ("host-loopback-ports", lambda: scan("192.168.127.254")),
    ("gateway-unbound-tcp", lambda: tcp("192.168.127.1", 7790)),
    ("api-list", lambda: http("GET", "http://192.168.127.1/services/forwarder/all")),
    ("api-expose", lambda: http("POST", "http://192.168.127.1/services/forwarder/expose", '{"local":"0.0.0.0:2299","remote":"192.168.127.2:8000"}')),
    ("host-loopback-tcp", lambda: tcp("192.168.127.254", int(E["LOOP_PORT"]))),
    ("host-own-address-tcp", lambda: tcp(E["HOST_LAN"], int(E["LAN_PORT"]))),
    ("lan-neighbour-tcp", lambda: tcp(E["NEIGHBOUR"], int(E["LAN_PORT"]))),
    ("lan-neighbour-udp", lambda: udp(E["NEIGHBOUR"], int(E["UDP_PORT"]))),
    ("lan-neighbour-ping", lambda: ping(E["NEIGHBOUR"])),
    ("host-own-address-ping", lambda: ping(E["HOST_LAN"])),
    ("public-lan-neighbour-tcp", lambda: tcp(E["PUB_NEIGHBOUR"], int(E["LAN_PORT"]))),
    ("public-lan-neighbour-ping", lambda: ping(E["PUB_NEIGHBOUR"])),
    ("metadata-tcp", lambda: tcp("169.254.169.254", 80)),
    ("metadata-ping", lambda: ping("169.254.169.254")),
    ("zero-net-tcp", lambda: tcp("0.0.0.1", int(E["LAN_PORT"]))),
    ("internet-tcp", lambda: tcp(E["INTERNET"], int(E["NET_PORT"]))),
    ("internet-udp", lambda: udp(E["INTERNET"], int(E["UDP_PORT"]))),
    ("internet-ping", lambda: ping(E["INTERNET"])),
    ("dns-name", lambda: dns("allowed.example")),
    ("dns-exfil", lambda: dns(E["EXFIL"])),
]
only = sys.argv[1:]
for name, f in cases:
    if not only or name in only:
        print(name, f(), flush=True)
