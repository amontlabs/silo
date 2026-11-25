"""A logging HTTPS proxy: it logs every CONNECT (host, port, time, and the process that opened the
connection, found from /proc), then tunnels it to the upstream proxy given in UPSTREAM
(host:port), or straight to the host when UPSTREAM is empty.

It stands in for what Silo's boundary sees of the names a computer reaches: the host as a fact
(here from CONNECT; at Silo's boundary from the TLS SNI or the Host header), and the process as a
report.

Run: UPSTREAM=<upstream proxy host:port> LOGF=hosts.jsonl PORT=3199 python3 host_log_proxy.py
Then run the install with HTTPS_PROXY=http://127.0.0.1:3199.
"""
import asyncio, json, os, time

UP = os.environ.get("UPSTREAM", "")
LOG = open(os.environ.get("LOGF", "hosts.jsonl"), "a")
TAG = os.environ.get("TAG", "")


def proc_of(peer_port):
    """The process that owns the client end of a local TCP connection (Linux /proc)."""
    inode = None
    for f in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            for line in open(f).readlines()[1:]:
                parts = line.split()
                if int(parts[1].split(":")[1], 16) == peer_port:
                    inode = parts[9]; break
        except OSError:
            pass
        if inode:
            break
    if not inode:
        return None
    target = f"socket:[{inode}]"
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        try:
            for fd in os.listdir(f"/proc/{pid}/fd"):
                if os.readlink(f"/proc/{pid}/fd/{fd}") == target:
                    cmd = open(f"/proc/{pid}/cmdline", "rb").read().split(b"\0")
                    parent = open(f"/proc/{pid}/stat").read().split(") ")[1].split()[1]
                    pcmd = open(f"/proc/{parent}/cmdline", "rb").read().split(b"\0")
                    return {"pid": int(pid), "comm": open(f"/proc/{pid}/comm").read().strip(),
                            "cmd": b" ".join(cmd[:3]).decode(errors="replace")[:160],
                            "parent": b" ".join(pcmd[:3]).decode(errors="replace")[:160]}
        except OSError:
            continue
    return None


async def pipe(r, w):
    try:
        while True:
            b = await r.read(65536)
            if not b:
                break
            w.write(b); await w.drain()
    except Exception:
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass


async def handle(cr, cw):
    head = b""
    while b"\r\n\r\n" not in head:
        b = await cr.read(4096)
        if not b:
            cw.close(); return
        head += b
    first = head.split(b"\r\n")[0].decode()
    method, target, _ = first.split(" ", 2)
    if method != "CONNECT":
        cw.write(b"HTTP/1.1 405 Method Not Allowed\r\ncontent-length: 0\r\n\r\n"); cw.close(); return
    host, port = target.rsplit(":", 1)
    peer = cw.get_extra_info("peername")
    rec = {"t": round(time.time(), 3), "tag": TAG, "host": host, "port": int(port), "proc": proc_of(peer[1])}
    LOG.write(json.dumps(rec) + "\n"); LOG.flush()
    try:
        if UP:
            uh, up = UP.rsplit(":", 1)
            ur, uw = await asyncio.open_connection(uh, int(up))
            uw.write(f"CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n".encode()); await uw.drain()
            resp = b""
            while b"\r\n\r\n" not in resp:
                b = await ur.read(4096)
                if not b:
                    raise OSError("upstream closed")
                resp += b
            cw.write(resp); await cw.drain()
            if b" 200" not in resp.split(b"\r\n")[0]:
                cw.close(); return
        else:
            ur, uw = await asyncio.open_connection(host, int(port))
            cw.write(b"HTTP/1.1 200 Connection established\r\n\r\n"); await cw.drain()
    except Exception as e:
        try:
            cw.write(f"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nx-err: {e}\r\n\r\n".encode()); cw.close()
        except Exception:
            pass
        return
    await asyncio.gather(pipe(cr, uw), pipe(ur, cw))


async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", int(os.environ.get("PORT", "3199")))
    async with srv:
        await srv.serve_forever()

asyncio.run(main())
