"""The same pattern for Redis, in a few lines: the computer's REDIS_URL holds a placeholder.
AUTH <placeholder> (or HELLO ... AUTH user <placeholder>) is checked here and replaced with the
real password; FLUSHALL, FLUSHDB, CONFIG, SCRIPT, EVAL, MODULE, DEBUG, SHUTDOWN, KEYS * are
refused. Plain TCP to keep it short; the Postgres prototype shows the TLS legs.

Run: python redis_boundary.py <listen_port> <redis_port> <placeholder> <real_password_file>
"""
import asyncio
import hmac
import sys

LISTEN, UPSTREAM, PLACEHOLDER, PWFILE = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4]
REAL = open(PWFILE).read().strip()
REFUSED = {b"FLUSHALL", b"FLUSHDB", b"CONFIG", b"SCRIPT", b"EVAL", b"EVALSHA", b"FCALL",
           b"FUNCTION", b"MODULE", b"DEBUG", b"SHUTDOWN", b"REPLICAOF", b"SLAVEOF", b"MIGRATE",
           b"ACL", b"SWAPDB"}


async def read_command(r):
    """One RESP array of bulk strings, as redis-cli and client libraries send."""
    line = await r.readline()
    if not line:
        return None
    if not line.startswith(b"*"):  # inline command
        return line.strip().split()
    parts = []
    for _ in range(int(line[1:])):
        ln = int((await r.readline())[1:])
        parts.append((await r.readexactly(ln + 2))[:-2])
    return parts


def encode(parts):
    return b"*%d\r\n" % len(parts) + b"".join(b"$%d\r\n%s\r\n" % (len(p), p) for p in parts)


async def handle(cr, cw):
    sr, sw = await asyncio.open_connection("127.0.0.1", UPSTREAM)

    async def pump():
        while data := await sr.read(65536):
            cw.write(data)
            await cw.drain()
        cw.close()

    task = asyncio.create_task(pump())
    try:
        while (cmd := await read_command(cr)) is not None:
            name = cmd[0].upper() if cmd else b""
            if name == b"AUTH" or (name == b"HELLO" and b"AUTH" in [c.upper() for c in cmd]):
                i = len(cmd) - 1 if name == b"AUTH" else [c.upper() for c in cmd].index(b"AUTH") + 2
                if not hmac.compare_digest(cmd[i], PLACEHOLDER.encode()):
                    cw.write(b"-WRONGPASS Silo: not the placeholder of this computer\r\n")
                    continue
                cmd[i] = REAL.encode()
                if name == b"AUTH" and len(cmd) == 3 and cmd[1] == b"":
                    del cmd[1]  # redis-cli sends an empty user for redis://:pw@host
            elif name in REFUSED or (name == b"KEYS" and cmd[1:] == [b"*"]):
                cw.write(b"-ERR Silo denied " + name + b" on this Redis: a rule refuses it\r\n")
                print("denied", cmd[:3], flush=True)
                continue
            sw.write(encode(cmd))
            await sw.drain()
    finally:
        sw.close()
        task.cancel()


async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", LISTEN)
    print(f"redis boundary on {LISTEN} -> {UPSTREAM}", flush=True)
    await srv.serve_forever()

asyncio.run(main())
