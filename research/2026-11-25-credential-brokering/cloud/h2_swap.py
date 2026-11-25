"""Can a placeholder be swapped in gRPC (HTTP/2) traffic, as Google's Pub/Sub, Spanner and Firestore
libraries send it? Throwaway research code.

A gRPC server (grpcio, plaintext HTTP/2) records the authorization header of each call. A gRPC
client sends `authorization: Bearer <placeholder>` on 3 calls, through one of two boundaries:

  bytes   replaces the placeholder's bytes in the stream, as a swap made on HTTP/1.1 text could;
  hpack   decodes each HEADERS frame (h2 library), swaps the header value, encodes it again.

Clients: Python grpcio (C core), and if given, a Go grpc binary and a Node grpc-js script
(grpc_clients/). Usage: python3 h2_swap.py [go-client-binary] [node-client.js]

Prints, per client and boundary, how often the placeholder's bytes appeared in the stream, and what the
server received.
"""
import asyncio, os, secrets, sys, threading, time, json
from concurrent import futures
import grpc
import h2.connection, h2.config, h2.events

REAL = "ya29.c." + secrets.token_urlsafe(120)
PH = "ya29.c.siloPH_" + secrets.token_urlsafe(120)[: len(REAL) - 14]
SEEN = []


def handler(req, ctx):
    SEEN.append(dict(ctx.invocation_metadata()).get("authorization", ""))
    return b"pong"


def start_grpc(port):
    s = grpc.server(futures.ThreadPoolExecutor(4))
    s.add_generic_rpc_handlers([grpc.method_handlers_generic_handler(
        "demo.Echo", {"Ping": grpc.unary_unary_rpc_method_handler(handler)})])
    s.add_insecure_port(f"127.0.0.1:{port}"); s.start(); return s


STATS = {"placeholder_bytes_seen": 0}


async def byte_proxy(cr, cw, up):
    ur, uw = await asyncio.open_connection("127.0.0.1", up)
    async def c2u():
        while (b := await cr.read(65536)):
            STATS["placeholder_bytes_seen"] += b.count(PH.encode())
            uw.write(b.replace(PH.encode(), REAL.encode())); await uw.drain()
        uw.close()
    async def u2c():
        while (b := await ur.read(65536)): cw.write(b); await cw.drain()
        cw.close()
    await asyncio.gather(c2u(), u2c())


async def hpack_proxy(cr, cw, up):
    """Ends HTTP/2 on both sides and carries streams across, swapping the header value."""
    ur, uw = await asyncio.open_connection("127.0.0.1", up)
    down = h2.connection.H2Connection(h2.config.H2Configuration(client_side=False, header_encoding=None))
    upc = h2.connection.H2Connection(h2.config.H2Configuration(client_side=True, header_encoding=None))
    down.initiate_connection(); upc.initiate_connection()
    cw.write(down.data_to_send()); uw.write(upc.data_to_send())
    smap, rmap = {}, {}

    def flush():
        if (d := down.data_to_send()): cw.write(d)
        if (d := upc.data_to_send()): uw.write(d)

    async def from_client():
        while (b := await cr.read(65536)):
            STATS["placeholder_bytes_seen"] += b.count(PH.encode())
            for ev in down.receive_data(b):
                if isinstance(ev, h2.events.RequestReceived):
                    hs = [(k, (b"Bearer " + REAL.encode()) if k == b"authorization" and v == b"Bearer " + PH.encode() else v)
                          for k, v in ev.headers]
                    sid = upc.get_next_available_stream_id(); smap[ev.stream_id] = sid; rmap[sid] = ev.stream_id
                    upc.send_headers(sid, hs)
                elif isinstance(ev, h2.events.DataReceived):
                    upc.send_data(smap[ev.stream_id], ev.data); down.acknowledge_received_data(ev.flow_controlled_length, ev.stream_id)
                elif isinstance(ev, h2.events.StreamEnded):
                    upc.end_stream(smap[ev.stream_id])
            flush(); await cw.drain(); await uw.drain()

    async def from_up():
        while (b := await ur.read(65536)):
            for ev in upc.receive_data(b):
                if isinstance(ev, h2.events.ResponseReceived):
                    down.send_headers(rmap[ev.stream_id], ev.headers)
                elif isinstance(ev, h2.events.DataReceived):
                    down.send_data(rmap[ev.stream_id], ev.data); upc.acknowledge_received_data(ev.flow_controlled_length, ev.stream_id)
                elif isinstance(ev, h2.events.TrailersReceived):
                    down.send_headers(rmap[ev.stream_id], ev.headers, end_stream=True)
                elif isinstance(ev, h2.events.StreamEnded):
                    try: down.end_stream(rmap[ev.stream_id])
                    except Exception: pass
            flush(); await cw.drain(); await uw.drain()

    await asyncio.gather(from_client(), from_up(), return_exceptions=True)


def run(kind, listen, up):
    loop = asyncio.new_event_loop()
    fn = byte_proxy if kind == "bytes" else hpack_proxy
    async def main():
        srv = await asyncio.start_server(lambda r, w: fn(r, w, up), "127.0.0.1", listen)
        await srv.serve_forever()
    threading.Thread(target=lambda: loop.run_until_complete(main()), daemon=True).start()


import subprocess
CLIENTS = {"python grpcio": None}
if len(sys.argv) > 1 and os.path.exists(sys.argv[1]): CLIENTS["Go grpc"] = [sys.argv[1]]
if len(sys.argv) > 2 and os.path.exists(sys.argv[2]): CLIENTS["Node grpc-js"] = ["node", sys.argv[2]]
out = {}
server = start_grpc(50551)
for i, kind in enumerate(("bytes", "hpack")):
    run(kind, 50560 + i, 50551)
time.sleep(0.3)
for client, cmd in CLIENTS.items():
    for i, kind in enumerate(("bytes", "hpack")):
        SEEN.clear(); STATS["placeholder_bytes_seen"] = 0
        port = 50560 + i
        if cmd is None:
            ch = grpc.insecure_channel(f"127.0.0.1:{port}")
            call = ch.unary_unary("/demo.Echo/Ping")
            for _ in range(3): call(b"ping", metadata=[("authorization", "Bearer " + PH)], timeout=5)
            ch.close()
        else:
            subprocess.run(cmd + [f"127.0.0.1:{port}", PH], capture_output=True, timeout=30)
        time.sleep(0.2)
        out[f"{client} / {kind}"] = r = {"calls": len(SEEN), "placeholder_bytes_seen_in_stream": STATS["placeholder_bytes_seen"],
                                         "server_got_real": sum(1 for s in SEEN if s == "Bearer " + REAL),
                                         "server_got_placeholder": sum(1 for s in SEEN if s == "Bearer " + PH)}
        print(f"{client:14} {kind:6} {json.dumps(r)}", flush=True)
server.stop(0)
