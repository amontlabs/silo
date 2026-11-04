# A URL per computer

Each computer's published websites get their own local name, for example
`http://3000.fork-1.silo.localhost:47383`, so several computers that all serve
guest `:3000` can be open side by side. The code is `url_proxy.rs` in the engine;
`silod` feeds it and exposes `network.urls`.

## How it works

- Published ports already exist: the runtime forwards a guest port to a loopback
  host port (see the [network plan](SiloUI-NETWORK-PLAN.md)). The proxy sits in
  front of those forwards and never touches the guest, so it works the same for
  Linux and macOS computers.
- One listener on 127.0.0.1 serves every name. Its port is 47383, or the number
  in `SILO_URL_PROXY_PORT`; when that is busy at start it takes a free port.
  `network.urls` always reports the port actually bound. No guest port number is
  bound on the host, so a development server the user starts later on `:3000`
  (any address family) is never in the way.
- The host name is `<guest port>.<computer>.silo.localhost:<proxy port>`. A
  request whose port is not the proxy's, or that has none, is answered 404.
- A route is added for each website port (scheme `http`) that the network state
  reports as reachable. Each observation carries a ticket taken before it was made;
  an older ticket never replaces a newer table, and removing a computer counts as
  the newest change. The table is refreshed on every local network state read and
  by the 30-second port-repair timer, which observes only while a website port is
  saved or a route is open. Deleting a computer (Linux or macOS) removes its routes
  at once and closes its open connections and WebSocket tunnels.
- A route whose upstream is the proxy's own port is dropped.
- Names are the computer name as one DNS label. The assignment is saved by computer
  id (`url-names.json` beside the network settings) and kept while the computer
  exists, even when it is stopped. A taken label is never given to another computer:
  a later computer with the same label gets its id's first eight hex digits, and a
  counter if that is taken too.
- Unknown names and unpublished ports answer 404; a computer whose forward is not
  answering answers 502, and one that takes more than 60 seconds to connect or send
  response headers answers 504. A client has 10 seconds to send its request head, and
  at most 256 connections are served at once.
- Requests are forwarded by hyper, one upstream connection each. Connection-scoped
  headers (`Connection` and the headers it names, `Keep-Alive`, `Proxy-*`, `TE`,
  `Trailer`, `Transfer-Encoding`, `Upgrade`) are removed both ways, except the
  WebSocket handshake headers. `X-Forwarded-Host`, `-Proto` and `-For` are set by
  the proxy and any sent by the client are dropped. The `Host` header is passed on
  unchanged. The accept loop retries transient errors with a backoff and is
  reopened by the next sync if it ever stops.
- `*.localhost` resolves to loopback in browsers (RFC 6761) and in curl on macOS
  (checked with `curl http://x.silo.localhost:PORT/`). No `/etc/hosts` edit or
  elevated privilege is needed. Tools that do not resolve `*.localhost` can use
  `curl --resolve <name>:<port>:127.0.0.1`.
- Only HTTP websites are routed. `https` and plain TCP ports keep their
  host-port URLs.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Listeners on the guest port numbers (`:3000`) | A later `0.0.0.0:3000` server of the user fails with EADDRINUSE, and `localhost` splits by address family. |
| Caddy, Traefik or nginx as a sidecar | A new binary to bundle, sign, update and configure for a routing table of a few entries. |
| `hyper-reverse-proxy` and similar crates | Less maintained than hyper itself, which the dependency tree already carries through reqwest; the forwarding is a short function on hyper's client connection. |
| Editing `/etc/hosts` or running a local DNS server | Needs elevated rights or system changes; `*.localhost` already resolves. |
| Binding `0.0.0.0` | Would expose dev servers on the network; the listener binds 127.0.0.1 only. |

Limits: cookies are per host name, so each computer and port is its own cookie
jar; the names do not separate pages from other loopback services. Computers on
another device are served by that device, and `network.urls` refuses a `deviceId`
for another device.
