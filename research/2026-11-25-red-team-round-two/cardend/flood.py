"""Main scans from inside: SYNs to N distinct addresses, sent to a running cardend the way the card
hands frames over (one per datagram). Prints cardend's memory and the size of its counts file.

    python3 flood.py <cardend binary> [N]
"""
import os, socket, struct, subprocess, sys, tempfile, time

BIN, N = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 1_000_000
d = tempfile.mkdtemp()
p = subprocess.Popen([BIN, "-frames", f"{d}/frames", "-counts", f"{d}/counts.json"], stderr=subprocess.DEVNULL)
while not os.path.exists(f"{d}/frames"):
    time.sleep(0.02)
s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
s.bind(f"{d}/card")
s.connect(f"{d}/frames")


def csum(b):
    t = sum(struct.unpack("!10H", b))
    t = (t >> 16) + (t & 0xFFFF)
    return ~(t + (t >> 16)) & 0xFFFF


def rss():
    return int(next(l for l in open(f"/proc/{p.pid}/status") if l.startswith("VmRSS")).split()[1]) // 1024


eth = bytes.fromhex("5a94ef000001" "5a94ef000002" "0800")
tcp = struct.pack("!HHIIBBHHH", 40000, 443, 1, 0, 0x50, 0x02, 65535, 0, 0)
t0 = time.time()
for i in range(N):
    ip = bytearray(struct.pack("!BBHHHBBH4s4s", 0x45, 0, 40, 0, 0, 64, 6, 0, bytes([169, 254, 77, 7]), struct.pack("!I", 0x0A000000 + i)))
    struct.pack_into("!H", ip, 10, csum(bytes(ip)))
    while True:
        try:
            s.send(eth + bytes(ip) + tcp)
            break
        except BlockingIOError:
            time.sleep(0.001)
        except OSError:
            time.sleep(0.001)
time.sleep(1.5)
size = os.path.getsize(f"{d}/counts.json")
print(f"{os.path.basename(BIN)}: {N:,} SYNs to {N:,} addresses in {time.time() - t0:.0f} s; "
      f"cardend holds {rss()} MiB, counts file {size / 1e6:.1f} MB")
p.terminate()
p.wait()
