"""Connect to every TCP port of each address given, from inside the computer. Prints the open ones.
usage: scan.py <address>...   (PORTS="53,80,..." to try only those)"""
import os
import asyncio, sys
async def one(a, p, sem, out):
    async with sem:
        try:
            r, w = await asyncio.wait_for(asyncio.open_connection(a, p), 0.5)
            out.append(p); w.close()
        except Exception:
            pass
async def main():
    for a in sys.argv[1:]:
        out = []; sem = asyncio.Semaphore(800)
        await asyncio.gather(*(one(a, p, sem, out) for p in (map(int, os.environ["PORTS"].split(",")) if os.environ.get("PORTS") else range(1, 65536))))
        print(f"   {a}: open TCP ports {sorted(out) or 'none'}")
asyncio.run(main())
