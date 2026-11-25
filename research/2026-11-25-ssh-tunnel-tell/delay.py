"""A delay line: carries bytes both ways and holds each chunk for a fixed time before passing it on,
without slowing the stream down (chunks in flight overlap, order is kept). It stands for a network
path with a round trip of 2 x one_way_ms. The container this ran in has no netem, so the delay is
made here, in user space.

    delay.py stdio  <one_way_ms> <host> <port>             a ProxyCommand: stdin/stdout <-> TCP
    delay.py tcp    <one_way_ms> <listen_port> <host> <port>
    delay.py unix   <one_way_ms> <listen_path> <target_path>
"""
import asyncio, os, sys


async def line(r, w, d):
    loop, q = asyncio.get_running_loop(), asyncio.Queue()

    async def sender():
        while True:
            due, b = await q.get()
            if b is None: break
            if (wait := due - loop.time()) > 0: await asyncio.sleep(wait)
            w.write(b); await w.drain()
        try: w.close()
        except Exception: pass

    s = asyncio.create_task(sender())
    try:
        while (b := await r.read(65536)):
            q.put_nowait((loop.time() + d, b))
    except (ConnectionError, OSError):
        pass
    q.put_nowait((loop.time() + d, None))
    await s


async def both(r1, w1, r2, w2, d):
    await asyncio.gather(line(r1, w2, d), line(r2, w1, d), return_exceptions=True)


async def main():
    mode, d = sys.argv[1], float(sys.argv[2]) / 1000
    loop = asyncio.get_running_loop()
    if mode == "stdio":
        r2, w2 = await asyncio.open_connection(sys.argv[3], int(sys.argv[4]))
        r1 = asyncio.StreamReader(); await loop.connect_read_pipe(lambda: asyncio.StreamReaderProtocol(r1), sys.stdin)
        t, p = await loop.connect_write_pipe(asyncio.streams.FlowControlMixin, os.fdopen(1, "wb"))
        w1 = asyncio.StreamWriter(t, p, None, loop)
        await both(r1, w1, r2, w2, d)
        return
    if mode == "tcp":
        lport, host, port = int(sys.argv[3]), sys.argv[4], int(sys.argv[5])
        async def h(r1, w1):
            r2, w2 = await asyncio.open_connection(host, port); await both(r1, w1, r2, w2, d)
        srv = await asyncio.start_server(h, "127.0.0.1", lport)
    else:
        path, target = sys.argv[3], sys.argv[4]
        if os.path.exists(path): os.unlink(path)
        async def h(r1, w1):
            r2, w2 = await asyncio.open_unix_connection(target); await both(r1, w1, r2, w2, d)
        srv = await asyncio.start_unix_server(h, path)
    print("ready", flush=True)
    await srv.serve_forever()


asyncio.run(main())
