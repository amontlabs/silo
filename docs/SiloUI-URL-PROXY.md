# A URL per computer

Each computer's published websites get their own local name, for example
`http://fork-a.silo.localhost:3000`, so several computers that all serve guest
`:3000` can be open side by side. The code is `url_proxy.rs` in the engine; `silod`
feeds it and exposes `network.urls`.

## How it works

- Published ports already exist: the runtime forwards a guest port to a loopback
  host port (see the [network plan](SiloUI-NETWORK-PLAN.md)). The proxy sits in
  front of those forwards and never touches the guest, so it works the same for
  Linux and macOS computers.
- A route is added for each website port (scheme `http`) that the network state
  reports as reachable. The table is replaced whole on every network state read
  and by the 30-second port-repair timer, so a computer that stops, loses its
  port or is deleted leaves the table. Deleting a computer also removes its routes
  at once.
- One loopback listener serves each guest port that some computer publishes and
  dispatches by `Host` (`<computer>.silo.localhost[:port]`). Unknown names and
  unpublished ports answer 404; a computer whose forward is not answering
  answers 502.
- Names are the lower-cased computer name as one DNS label. Two names that
  reduce to the same label get the first eight hex digits of the computer id on
  the later one.
- The listener tries the guest port number itself, so the common case is
  `:3000`. If anything already answers on that port on 127.0.0.1 or ::1 (the
  user's own dev server, a published forward) or the bind fails, it takes a free
  port instead. `network.urls` reports the URL as bound, so a client never has to
  guess.
- Requests are forwarded by hyper with the `Host` header unchanged, so dev
  servers see the name the browser used. Upgrades (WebSocket, for hot reload)
  are passed through in both directions. Each request opens its own upstream
  connection; there is no pooling. A connection that has already upgraded keeps
  running after its route is removed.
- `*.localhost` resolves to loopback in browsers (RFC 6761) and in curl on macOS
  (checked: `curl http://x.silo.localhost:PORT/` connects to 127.0.0.1). No
  `/etc/hosts` edit or elevated privilege is needed. Tools that do not resolve
  `*.localhost` can use `curl --resolve <name>:<port>:127.0.0.1`.
- Only HTTP websites are routed. `https` ports and plain TCP ports keep their
  existing host-port URLs.

## Alternatives considered

| Option | Why not |
| --- | --- |
| A single fixed proxy port | `Host` cannot tell two ports of one computer apart, and the `:3000` URLs would not hold. |
| Caddy, Traefik or nginx as a sidecar | A new binary to bundle, sign, update and configure for a routing table of a few entries. |
| `hyper-reverse-proxy` and similar crates | Small projects with less maintenance than hyper itself, which the dependency tree already carries (through reqwest); the forwarding is about sixty lines on hyper's client connection. |
| Editing `/etc/hosts` or running a local DNS server | Needs elevated rights or changes system settings; unnecessary because `*.localhost` already resolves. |
| Binding `0.0.0.0` | Would expose dev servers on the network; the listeners bind 127.0.0.1 only. |

Limits: the names do not give web pages separate origins from other loopback
services; cookies are still per host name, which is the reason for a name per
computer. Computers on another device are served by that device, and
`network.urls` refuses a `deviceId` for another device.
