"""The network between each Mac and the ledger: a TCP relay per Mac that delays every byte by half
a round trip each way, and that can cut a Mac off.

python3 netem.py --ctl ctl.json --map LISTEN:TARGET:RTT_MS [--map ...]

ctl.json is read every 50 ms: {"down": [LISTEN, ...]} lists the relays that are cut off. A cut-off
relay forwards nothing and opens nothing new; what was sent waits, as TCP retransmits, and goes
through once the cut ends, unless the Mac gave up and closed first. A ledger that is killed is
another thing: the relay then cannot connect, and the Mac sees its connection closed at once.
"""
import argparse, asyncio, json, os, time

DOWN = set()


async def pump(reader, writer, delay, port):
    q = asyncio.Queue()

    async def later():
        while True:
            at, data = await q.get()
            if data is None:
                break
            while True:
                now = time.monotonic()
                if now < at:
                    await asyncio.sleep(at - now)
                if port not in DOWN:
                    break
                await asyncio.sleep(0.05)
            try:
                writer.write(data)
                await writer.drain()
            except (ConnectionError, OSError):
                break
        try:
            writer.close()
        except Exception:
            pass
    t = asyncio.create_task(later())
    try:
        while True:
            data = await reader.read(65536)
            if not data:
                break
            await q.put((time.monotonic() + delay, data))
    except (ConnectionError, OSError):
        pass
    await q.put((time.monotonic() + delay, None))
    await t


def serve(listen, target, rtt):
    async def on(cr, cw):
        while listen in DOWN:
            await asyncio.sleep(0.05)
        try:
            ur, uw = await asyncio.open_connection("127.0.0.1", target)
        except OSError:
            cw.close()
            return
        await asyncio.gather(pump(cr, uw, rtt / 2, listen), pump(ur, cw, rtt / 2, listen))
    return asyncio.start_server(on, "127.0.0.1", listen)


async def ctl(path):
    while True:
        try:
            with open(path) as f:
                d = set(json.load(f).get("down", []))
            DOWN.clear()
            DOWN.update(d)
        except (OSError, ValueError):
            pass
        await asyncio.sleep(0.05)


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ctl", required=True)
    ap.add_argument("--map", action="append", required=True)
    a = ap.parse_args()
    servers = []
    for m in a.map:
        listen, target, rtt = m.split(":")
        servers.append(await serve(int(listen), int(target), float(rtt) / 1000))
    print("netem up", flush=True)
    await ctl(a.ctl)


if __name__ == "__main__":
    asyncio.run(main())
