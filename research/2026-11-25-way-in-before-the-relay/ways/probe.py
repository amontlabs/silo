"""Inside the computer, during the window before the relay: what the way in leaves open. Runs
#380's guest.py cases (except its gvproxy-specific gateway scans) plus a scan of every port of the
address the way in hands the computer as its router, and of the Mac end's link-local address.
usage: probe.py <guest.py> <addresses to scan, comma-separated, or 'none'>"""
import os, subprocess, sys, asyncio
guest, router = sys.argv[1], sys.argv[2]
skip = {"gateway-ports", "host-loopback-ports", "api-list", "api-expose"}
names = ["host-loopback-tcp", "host-own-address-tcp", "lan-neighbour-tcp", "lan-neighbour-udp", "lan-neighbour-ping",
         "host-own-address-ping", "metadata-tcp", "zero-net-tcp", "internet-tcp", "internet-udp", "internet-ping", "dns-name", "dns-exfil"]
print(subprocess.run([sys.executable, "-I", guest] + names, capture_output=True, text=True).stdout, end="", flush=True)
async def one(a, p, sem, out):
    async with sem:
        try:
            r, w = await asyncio.wait_for(asyncio.open_connection(a, p), 0.5); out.append(p); w.close()
        except Exception: pass
def scan(a):
    out = []
    async def go():
        sem = asyncio.Semaphore(800); await asyncio.gather(*(one(a, p, sem, out) for p in range(1, 65536)))
    asyncio.run(go()); return ",".join(map(str, sorted(out))) or "none"
for a in router.split(","):
    print("ports-on " + a, scan(a) if a != "none" else "no router", flush=True)
