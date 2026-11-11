# Silo's tools in a computer

An agent that lives inside a computer (Claude Code, Codex, OpenClaw, Hermes) uses Silo from
there with the same `silo` command and the same `silo mcp` server as on the device. Every call it
makes is that computer's call: the engine knows which computer is calling from the channel the
call arrives on, and [the policy](SiloUI-POLICY.md) decides what it may do, what asks the owner,
and what is refused.

## How a call reaches the engine

```
computer                                device
--------                                ------
silo  ──TCP──>  host.microsandbox.internal:<port>  ──(runtime maps the gateway to loopback)──>
                                         127.0.0.1:<port>  silod listener for this computer
                                         silod sends a nonce; the handshake answers with
                                         HMAC(SHA-256(token), nonce, port), never the token
                                         every request runs as Caller::Computer { id }
```

- **One port per computer.** For each computer with the tools on, `silod` listens on its own
  loopback port (`127.0.0.1` only) and serves the ordinary engine protocol on it: the same
  framing, handshake and methods as `engine.sock`, with the caller fixed to that computer.
- **One network rule.** The computer's network policy carries `allow@host:tcp:<port>`, which lets
  it reach exactly that port on its gateway address; the runtime maps traffic to the gateway onto
  the device's loopback ([S1](direction/evidence/S1.md), question 1). The rule is present in every
  network mode (open, allowlist, closed) and only in that computer's rules
  (`network_policy::extra_rules`). `host.microsandbox.internal` resolves to the gateway in every
  computer.
- **A token.** The engine mints 32 bytes from the operating system's random source for the
  computer, writes them into the computer, and keeps only their SHA-256
  (`<app data>/guest-client/tokens.json`, `0600`). The token is valid on that computer's port
  alone. The device side always sees `127.0.0.1`, so the token, not the address, is what tells
  computers apart.
- **A challenge, not the token.** The token never crosses the connection. The engine speaks first
  with a fresh random nonce, and the client answers with HMAC-SHA-256 keyed by SHA-256 of the
  token over a fixed context string, the nonce and the port it believes it is connecting to. A
  program that collects what a client sends learns nothing it can use again, and an answer made
  for one port is useless on another.

## Choices

| Need | Choice | Why |
| --- | --- | --- |
| Transport | A loopback TCP port per computer on the device, reached through the gateway address. | The runtime already maps the gateway to the device's loopback and its rules can open one port; no runtime patch, the same for Linux virtual machines, containers and macOS computers. |
| Identity | A per-computer token, proved in the handshake by an HMAC over the engine's nonce and the port, checked against the port's computer. | The peer address is always `127.0.0.1`. The port says which computer the connection claims to be; the proof shows the token without sending it. |
| Client | The ordinary `silo` binary and `silo mcp`. | One CLI, one set of commands; the engine's answer decides what is allowed. |

Alternatives, and why not:

- **The `/.msb` folder** (the previous transport: request files the engine polled). It needed the
  engine to parse files a guest wrote in a share that has had no security review, a hold and a
  cache-dropping workaround around every full checkpoint, and it did not exist for macOS
  computers. Removed.
- **A Unix socket per computer mapped to the gateway port.** Leaves no port open on the device, but
  needs a runtime patch. It is the later hardening step (below).
- **virtio-vsock** (`--vsock`, S1 question 5). Works for Linux virtual machines, not for
  containers or macOS computers, and an export refuses a computer that has one.

## Pieces

| Piece | Where |
| --- | --- |
| `guestClient.enable`, `.revoke`, `.status` and the endpoint path | `app/SiloUI/api/src/methods/guest_client.rs` |
| The challenge, the handshake's `proof` and how it is made, protocol 8 | `app/SiloUI/api/src/transport.rs` (`ChannelChallenge`, `channel_proof`) |
| Ports, token hashes and held ports, the endpoint file, turning the tools on and off | `app/SiloUI/src-tauri/engine/src/guest_client/` |
| The rule in every network mode | `app/SiloUI/src-tauri/engine/src/network_policy/mod.rs` (`extra_rules`) |
| The listeners, the order of checks, the start-time file check | `app/SiloUI/src-tauri/silod/src/channel.rs`, `channel/watch.rs` |
| The session (shared with `engine.sock`) and its caller | `app/SiloUI/src-tauri/silod/src/socket.rs`, `dispatcher.rs` |
| The three methods | `app/SiloUI/src-tauri/silod/src/methods/guest_client.rs` |
| What each call may do, and who made which computer | `app/SiloUI/src-tauri/engine/src/policy/` |
| The transport choice in `silo`, `silo tools`, `silo mcp` inside a computer; reading the file again after a refusal | `app/SiloUI/cli/src/endpoint.rs`, `engine.rs`, `tools.rs`, `mcp.rs`; `app/SiloUI/client/src/session.rs` |

## Turning the tools on

New computers have the tools from the start: Linux virtual machines and containers (the rule is
in their create arguments) and macOS computers (the rule is in their gateway's start policy). A
fork of a computer that has them gets them too, on a port and with a token of its own. Existing
computers and imported ones stay off until the owner turns them on:

```sh
silo tools enable <computer> [--binary PATH]
silo tools status <computer>
silo tools disable <computer>
```

`enable` (`guestClient.enable`) opens the computer's port, and:

- for a **running** computer, writes the endpoint file now. If the computer's network does not
  carry the rule yet, the answer says it can reach the tools after a restart (`network:
  nextRestart`); `silo restart <computer>` does it.
- for a **stopped** virtual machine, nothing more: its next start sees that its runtime lacks the
  rule and restores it in place with the rule (as a network policy change does), then the file is
  written once it runs.
- for a stopped macOS or container computer, the gateway takes the rule at its next start.

With `--binary`, the CLI first sends a `silo` built for Linux into a running Linux computer as
`/usr/local/bin/silo`: the file is uploaded under a fresh name to `/workspace`, root refuses a link
or a non-regular file, copies the bytes into a `0700` folder under `/root`, checks their sha256
against the one the device computed, and installs them; the account removes the upload. A macOS
computer uses the device's own `silo`. Until the engine provides the binary itself, build one with
`app/SiloUI/scripts/build-guest-silo.sh` (needs `rustup target add aarch64-unknown-linux-musl` and
`zig`).

`disable` (`guestClient.revoke`) forgets the token hash and closes every open connection of the
computer at once. The rule stays in the computer's network until its next start without it, so
the engine keeps the port bound with a listener that accepts every connection and closes it at
once, reading nothing, until the computer runs with rules that no longer name it or is deleted
(the port is kept in `<app data>/guest-client/held-ports.json` and bound again at engine start).
No other program can take it in the meantime and become reachable from the computer. Turning the
tools on again reuses that port.

No answer carries the token. `status` gives the host name, the port, the endpoint path, when the
tools were turned on, when the computer's network lets it reach them (`applied`, `nextStart`,
`nextRestart`), and `restartNeeded` when the channel had to move to a new port (below) and is not
served until the computer restarts.

## The endpoint file

`/etc/silo/endpoint.json` in the computer:

```json
{"version": 1, "host": "host.microsandbox.internal", "port": 41234, "token": "silo_gc_…", "computerId": "…"}
```

Root-owned, mode `0640` with the `silo` account's group (`0600` when the computer has no such
account), written atomically by the engine as root through the guest agent (SSH for a macOS
computer). On a macOS computer the `silo` account's group is `staff`.

The engine checks the file whenever it sees the computer running after it was not (every start,
including the engine's own start), at most every 30 seconds after the computer changed, every ten
minutes, and as soon as it can for a running computer without a token: it reads the file as root
and compares the host, the port, the computer's id and the SHA-256 of the token with what it
keeps. A file that is missing or does not match is replaced with a new token; the old token stops
working at once and every session opened with it ends. Each check runs on its own thread, at most
four at once and one per computer, and each command in the computer has 8 seconds, so a computer
that stalls its check holds up no other. A computer whose check failed is tried again after five
seconds, then ten, doubling up to five minutes.

`silo` uses the channel when `/etc/silo/endpoint.json` exists, or when `SILO_ENDPOINT` names
another file; `--socket` always means the engine socket. A file it cannot read is an error, never
a silent fall back to a socket. When the engine refuses its handshake (closes without an answer),
`silo` reads the file again and retries with the new one, waiting 1, 2, 4 and 8 seconds while the
file has not changed: after an engine restart or an export the engine writes a new token within
seconds. `silo mcp` remembers the newer file for its later calls.

## The listener

On a new connection, in this order and before anything is answered:

1. The connection takes one of the computer's 4 handshake places, or is closed at once.
   Connections that have not proved anything never take one of the 8 places of sessions.
2. The engine sends `{"nonce": "<64 hex digits>"}`: 32 fresh bytes from the operating system's
   random source.
3. The first frame is read within 2 seconds (sending the nonce included) and at most 16 KiB.
4. It must be `engine.handshake` with a `proof`: lowercase hex of HMAC-SHA-256 keyed by the
   stored SHA-256 of the token **of the computer the port belongs to**, over the context string
   `silo channel handshake v1\0`, the nonce and the port (two bytes, big-endian), compared in
   constant time. The computer's record must name this port. The computer comes from the port;
   no field of the frame (`computerId`, the client's name, an SSH key, a `token`) selects or
   changes it.
5. Anything else (no proof, a proof for another computer, port or nonce, a wrong one, another
   method first, an oversized or malformed frame) gets no answer: the refusal is written to the
   audit log as a refused `engine.handshake` by that computer, at most once a second per
   computer so the rows collapse and cannot fill the audit queue, a line goes to the engine's log
   at most once a minute per computer, and the connection is closed after one second, holding its
   handshake place meanwhile.
6. The session then takes one of the computer's 8 session places, or is closed at once, and is
   served exactly as an `engine.sock` session, with every request made as
   `Caller::Computer { id }`. It ends when the tools are turned off or the token is minted again.

A held port (a computer whose tools were turned off, or whose channel moved) accepts and closes
every connection at once, with no challenge, no audit row and no log line.

Limits besides those: 16 requests running at once per computer over all its connections (2 more
kept for `engine.state`), the socket's 4 MiB frame size, and at most 16 jobs the computer has not
acknowledged (the gate refuses more). A job over half an hour old that the computer never
acknowledged is acknowledged for it once it has finished.

`events.subscribe` is owner-only, so a client on the channel follows jobs by polling
`operations.job.get`, as `silo` already does.

At engine start every token hash is forgotten: a token learned while the engine was not running
is dead, and each running computer gets a new one at its first check. Each computer's port is
bound again, and each held port that is free. A port another program took in the meantime is
replaced by a new one; the computer is marked `restartNeeded`, its new port is held rather than
served, and no file is written for it, because its network still names the old port. Once the
computer runs with rules that name the new port (its next start), the port is served and the file
written.

## What the engine allows

Every call runs through `methods::dispatch`, where [the policy](SiloUI-POLICY.md) decides it: free,
asked of the owner (`approval_required` with an approval id), or refused (`permission_denied`, with
the reason, which `silo` shows as it is). Every refused or held call, and every call that can
change something, is written to the audit log with the computer as caller. `silo list` inside a
computer shows that computer and the computers it made.

Inside a computer, `silo mcp` does not list the tools that only make sense on the device: the
hosted reviewer (`review`), SSH names (`ssh_name`) and reaching a review request's fork
(`request_open`); a `device` argument and a folder as a fork's source are refused there.

## Lifecycle

- **Promote and undo** turn the tools off in both computers involved (`changes.promote`: the
  fork and its target; `changes.undo`: both computers of the promote it takes back). A failure
  stops the operation before anything changes. Turn them on again for the computer that should
  have them.
- **Delete** forgets the computer's port and token hash and lets every port kept for it go. A
  record whose computer has been missing from the engine's list for ten minutes is forgotten too
  (a creation that failed), and so is a held port whose computer is missing for that long.
- **Forks.** A fork's copy of its parent's file is useless by construction: the fork cannot reach
  the parent's port (no rule for it) and the parent's proof is refused on the fork's port. Every
  fork also has the file (and the earlier client's `/etc/silo/guest-client.json`) removed: a Linux
  fork by its fork options, a macOS fork in the offline preparation of its disk. It gets its own
  file at its first start when it has the tools.
- **Exports.** An export of a Linux virtual machine is made from a hidden copy, which has the file
  removed with the known login files (whether or not the logins are kept, the copy is also kept
  without Silo's tools). Every other export (a checkpoint, a macOS or a container computer, or one
  that keeps the logins) holds the computer's file; right after it, the engine ends the
  computer's token, so the copy in the file proves nothing, and the computer gets a new one at its
  next check.
- **Restore.** A memory restore keeps the computer's processes but resets their TCP connections
  through the gateway, so an open session ends. Each `silo` command connects anew, and `silo mcp`
  reconnects on its next call.
- **Imports** start without the tools, whatever the archive carried.
- **Earlier tokens.** A token record of the earlier in-guest client hands the forks and jobs it
  recorded to the policy's record at engine start and is then removed; the tools of that
  computer are off until turned on.

## Container and macOS computers

A container computer's gateway and a macOS computer's gateway take the computer's policy when
they start (`network_policy::gateway_policy`), with the rule in it, and answer
`host.microsandbox.internal` with the gateway address. The listener and the token are the same.
The endpoint file is written through the container's guest agent, or over SSH as root on a macOS
computer.

## The `/.msb` folder

Silo writes nothing request-shaped into a computer's `/.msb` share and reads nothing from it:
there is no bridge folder, no stat override, no hold during captures and no cache dropping before
a full checkpoint. MicroSandbox itself still uses the share for its own control files (scripts,
TLS certificates, the heartbeat, checkpoint state), as described in
[boundary 6](THREAT-MODEL.md#6-the-workspace-disk-and-host-folders). A `silo-bridge` folder left
by an earlier engine is ignored.

## Open items

- **A port is open on the device's loopback** for each computer with the tools. Any program on
  the device can connect to it; without the token it gets a nonce, a closed connection and a
  collapsed audit row, and it can keep a computer's 4 handshake places busy so that computer's
  new sessions wait ([threat model, boundary 9](THREAT-MODEL.md#9-silos-tools-in-a-computer-a-port-and-a-token)).
  On a shared Linux host every account can do this, so a shared host is a weaker place than a
  single-user device. The later hardening is a runtime patch mapping the gateway port to a Unix
  socket in a private folder.
- While the engine is not running, another program can bind a computer's port. A client that
  connects then gives that program only a proof for its nonce on that port, which opens nothing,
  and every token is ended at the next engine start. If the program still holds the port at
  start, the computer can reach that program until it restarts (`restartNeeded`).
- The engine does not yet put the `silo` binary into a computer or register `silo mcp` with the
  agents (item T5b).
- The live check is `docs/direction/checks/t5_channel_check.sh`, with its output in
  `docs/direction/evidence/T5a.txt`.
