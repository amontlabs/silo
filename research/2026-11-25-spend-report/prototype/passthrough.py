"""The boundary for traffic it passes on unread (#393's computer set for a Claude plan): every byte
is forwarded as it came, encryption included, and only what can be seen from outside is written
down, as facts. One line per exchange in <OUT>/exchanges.jsonl, written as soon as the exchange
ends (or its connection closes, reply or not), so a program killed hard still leaves its exchanges behind.

An exchange is what can be told apart in encrypted HTTP/1.1 kept alive: a burst of bytes up (the
request), then bytes down (the reply), until bytes go up again or the connection closes. HTTP/2
multiplexes several calls in one connection and would need another cut (not verified here).

Each line: computer (the address the connection came from), connection id, exchange number, time
the request started, bytes up, time the reply started and ended, bytes down, and the number of
reads down (one per TLS record or so), all as seen at the boundary.

Run: OUT=out LISTEN=8500 UPSTREAM=127.0.0.1:8411 python3 passthrough.py
"""
import asyncio, itertools, json, os, time

OUT = os.environ.get("OUT", "out"); os.makedirs(OUT, exist_ok=True)
F = open(f"{OUT}/exchanges.jsonl", "a")
UP_HOST, UP_PORT = os.environ["UPSTREAM"].rsplit(":", 1)
ids = itertools.count(1)
# Fewer bytes down than this do not end a request: TLS 1.3 sends session tickets (about 255 bytes)
# while the first request is still going up, which would otherwise cut it in two.
MIN_REPLY = 512


def write(rec):
    F.write(json.dumps(rec) + "\n"); F.flush()


async def handle(cr, cw):
    conn = next(ids)
    peer = cw.get_extra_info("peername")[0]
    ur, uw = await asyncio.open_connection(UP_HOST, int(UP_PORT))
    ex = {"n": 0, "cur": None}

    def close_exchange():
        e = ex["cur"]
        if e:
            write(dict(e, computer=peer, conn=conn))
        ex["cur"] = None

    async def up():
        try:
            while d := await cr.read(65536):
                now = time.time()
                e = ex["cur"]
                if e is None or e["down_bytes"] >= MIN_REPLY:   # bytes up after a reply: a new request
                    close_exchange()
                    ex["n"] += 1
                    e = ex["cur"] = {"exchange": ex["n"], "t_up": round(now, 4), "up_bytes": 0,
                                     "t_down": None, "t_end": None, "down_bytes": 0, "down_reads": 0}
                e["up_bytes"] += len(d)
                uw.write(d); await uw.drain()
        finally:
            uw.close()

    async def down():
        try:
            while d := await ur.read(65536):
                now = time.time()
                e = ex["cur"]
                if e is not None:
                    if e["t_down"] is None:
                        e["t_down"] = round(now, 4)
                    e["t_end"] = round(now, 4); e["down_bytes"] += len(d); e["down_reads"] += 1
                cw.write(d); await cw.drain()
        finally:
            close_exchange()
            cw.close()

    await asyncio.gather(up(), down(), return_exceptions=True)


async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", int(os.environ.get("LISTEN", "8500")))
    async with srv:
        await srv.serve_forever()

asyncio.run(main())
