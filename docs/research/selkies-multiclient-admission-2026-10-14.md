# Selkies 2.0.0: admitting a second page without evicting the first (2026-10-14)

Scope: the pinned streamer only. Silo's engine already opens one proxy, tunnel, cookie and
WebSocket per client; this note records what the pinned Selkies does with the second
WebSocket and the mechanism chosen. No VM, app or network was used: everything below is the
2.0.0 source (tag `2.0.0`, checkout at `~/code/projects/selkies`), checked by hash against
`guest/patch-selkies-display-scaling.py` (`websockets_mode.py` `1d86b009...cebc`).

## What 2.0.0 does

All line numbers are in the pristine `src/selkies/websockets_mode.py`.

- **A second controller evicts the first.** In the `SETTINGS` branch of `ws_handler`, a page
  whose `displayId` is `primary` finds `display_clients['primary']` held by a live socket,
  sends it `KILL a new primary client connected connection killed` and closes it with
  `Superseded by new client` (3805, 3816). The web client reads "superseded" in the close
  reason as final and disables its reconnect (`selkies-ws-core.js`, `websocket.onclose`). The
  upstream protocol test says the same: a bystander "joins as a viewer: a second controller
  would take the display over" (`tests/integration/test_protocol.py`).
- **A per-address debounce refuses a quick second connection.** `RECONNECT_DEBOUNCE_MS = 500`
  (1046), applied per source IP at connect (3418). Every Silo viewer reaches the guest from
  the guest's own loopback (the engine's SSH/relay tunnel), so a second viewer opening within
  half a second of the first closes with code 4029 and only retries after the client's 5 s
  reload timer. The harness reproduced the refusal (152 ms).
- **Shared viewers are the documented multi-page mechanism, and they are read-only.** Legacy
  mode: `?role=viewer` (the client sends it for `#shared`, `selkies-ws-core.js` ~6575), refused
  when `--enable-shared=false` (3399). Secure mode: a `viewer` token from `POST /api/tokens`;
  one `mk_control` token can hold keyboard and mouse. A viewer's first `SETTINGS` takes the
  `joins the 'primary' stream` branch (3761): `_ensure_viewer_capture` if no capture runs (5384),
  `broadcast_stream_resolution`, a `PIPELINE_RESETTING primary` for that socket and an IDR.
  Messages are gated by `VIEWER_ALLOWED_PREFIXES` (`SETTINGS`, `START_VIDEO`, `STOP_VIDEO`,
  `REQUEST_KEYFRAME`, `js,`; `input_handler.py` 157) plus, with `enable_collab` and the active
  `mk` token in secure mode only, `VIEWER_COLLAB_EXTRA_PREFIXES` (keyboard, mouse, clipboard;
  166). The page itself blocks audio and video `pipelineControl` in shared mode.
- **The viewer machinery is the part to reuse.** Video goes to every non-paused, non-secondary
  socket through a per-socket `_VideoRelay` with a byte budget and skip-ahead (552, `do_fanout`
  5555); ACK backpressure throttles the display owner only (5575); a viewer's `STOP_VIDEO`
  pauses its own relay and the capture stops only when `_active_primary_consumers` is empty
  (2044); clipboard replies carry the requesting connection (`conn_id`) and go to it alone;
  a socket's departure removes its relay and, if it is not the display owner, nothing else.
- **Audio is not per socket.** `STOP_AUDIO` stops the pcmflux capture and broadcasts
  `AUDIO_STOPPED` to all clients (4250); the fan-out sends every chunk to every client but
  secondary-display owners (1702).
- **A page that did not request a size follows the server's.** On `stream_resolution` that
  differs from the page's own request, the client marks `streamResolutionDiverged`, resizes
  its canvas to the server size and routes input through the canvas box; it never answers with a
  resize. So a page can render a size another page chose without a resize fight.

## Options

| Option | Result |
|---|---|
| Shared viewer for every later client (`?role=viewer`, or a viewer credential) | No patch. N pages, none evicted. Read-only: a shared-mode page attaches no input, and the server refuses a viewer's keyboard, mouse and clipboard messages, so the engine's `sendFrames`, `sendShortcut` and clipboard writes, and audio control, would not work from the second client. Rejected. |
| Secure mode with tokens (`--master-token`, `POST /api/tokens`) | Documented, per-client tokens. One `mk_control` holder at a time and a token table to keep in step with the engine's connections. A second writer needs the table swapped. Rejected as the primary mechanism. |
| One Selkies process per client (port or path) | Several encoders, clipboard monitors, cursor captures and RandR writers on one X screen; no engine routing problem, but 16 clients means 16 encoders. Rejected. |
| Patch `websockets_mode.py` | Chosen. A later page that asks for a held primary display becomes a peer; the shared-viewer path serves it. |

## Chosen mechanism

`guest/patch-selkies-multiclient.py` (details in `docs/SiloUI-DESKTOP.md`, "Several viewers of
one computer"). It does not touch the page, so no minified JavaScript is patched, and it adds no
second path or port: each engine entry connects to the same `127.0.0.1:6901`.

## Reproduce

```sh
# Hash chain and the real patched handler (needs the 2.0.0 checkout and the pure-Python
# Selkies dependencies; skips the handler tests without them):
python3 -I app/SiloUI/scripts/test_selkies_multiclient_patch.py
python3 -I app/SiloUI/scripts/test_selkies_display_patch.py

# The baseline eviction and the debounce refusal are the handler run on the display-patched
# module without this patch: two fake sockets, SETTINGS for `primary` on each.
```

The tests stub `prometheus_client` and `watchdog` (the only missing imports), replace the
sound server, input handler and capture start with recorders, and run the unmodified
`ws_handler` of the patched module. They do not run pixelflux, pcmflux, a browser or a guest.
