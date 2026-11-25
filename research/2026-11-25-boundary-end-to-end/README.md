# The boundary end to end, on Linux

Research of 25 November 2026, by an agent, for polarzero. Six drafts each built one piece of the
boundary: #378 a computer whose only way out is a proxy reached over vsock, #380 a patched gvproxy
that hands every TCP connection to the boundary, #374 spend budgets in the Zig boundary of #347,
#379 placeholders for cloud credentials and a metadata server, #377 a key swapped in on a real
night, and #386 the record's format and its writer. This folder puts them in one program and runs
it with two computers, one on each way in, to see whether the pieces fit and what they cost
together.

Nothing here is a decision and nothing is built for Silo. The plan changed only by agents' notes,
listed at the end.

## In short

- **The pieces compose.** One boundary takes both ways in, as #380 recommends: c1 comes in by
  `CONNECT` through #378's relay, and c2 through gvproxy's hand-off with a PROXY v2 header. On both,
  39 of 39 cases pass: model calls of three providers, Google and Azure tokens from the metadata
  server, a placeholder refused on the way to another host, a real key pasted into the computer
  refused, SSH to `github.com:22`, a host not allowed, TLS that names another host than the
  connection, and no real value in the record, the trace or any file of either computer. With #380's own hand-off flags, the two metadata cases of c2 fail (37 of 39). Claude
  Code 2.1.332 ran a task on each computer holding only its placeholder, and every one of its 24
  model calls reached the host with the real key.
- **The boundary adds 0.3 to 3.5 ms to a request.** A request on a kept-alive connection took
  0.22 ms with no boundary, 0.53 ms through c1 and 0.71 ms through c2. A request on a new
  connection took 6.3, 7.9 and 9.7 ms: the boundary makes its own TLS connection to the host each
  time. The first byte of a streamed reply came 2 ms later through c1 and 4 ms later through c2.
- **#378's relay holds every reply for 40 ms.** Without `TCP_NODELAY` on the program's
  connection, a reply the boundary writes in two pieces waits for a delayed acknowledgement: 44 ms
  per request on a kept-alive connection, and 43 ms on the first byte of every stream. One line in
  the relay removes it. The relay on the Mac (Swift, in the tool #378 read) needs the same.
- **#380's hand-off and #379's metadata server do not work together as each specifies.** #380
  runs the hand-off with `-outbound-deny private,host`, which drops `169.254.169.254` before the
  boundary sees it. Cutting the list to `host` is not enough: gvproxy's own
  `-ec2-metadata-access` flag does nothing at upstream commit `197eea05`, because it never reaches
  the network stack's configuration. With the deny list cut and a one-line fix of that flag
  ([`gvproxy-ec2-flag.patch`](gvproxy-ec2-flag.patch)), c2 gets its tokens like c1.
- **Computers are told apart by the socket they come in on, not by an address.** #374 tells
  computers apart by the address a connection arrives on. Neither way in gives one: a vsock
  connection has none, and every computer behind its own gvproxy is `192.168.127.2`. Here each
  computer has its own `CONNECT` socket, its own hand-off socket and its own port for lookups.
- **When the recorder stops, new requests stop after the room for decisions fills, and what was
  already allowed carries on.** With room for 40 decisions, c1's 21st request waited 3 s and was
  refused with Silo's sentence, while a download allowed before the stop finished all 200 MB. When
  the recorder came back, every waiting line was written once and the chain held. Lookups made
  while the recorder was killed overflowed their buffer of 20, and the record shows one gap of 40
  lines, as `enforcing-and-recording` asks.
- **A program that asks for HTTP/2 gets HTTP/1.1.** The boundary of #347 and #374 offers only
  HTTP/1.1. curl falls back. gRPC cannot: Google's Go and Node cloud libraries (#379) would fail
  through this boundary before any placeholder is swapped.
- **Certificates made per host cost under 1.5 ms each, once.** #347 and #374 used one certificate
  made in advance for the providers' names. Reading any host a secret goes to needs one per name.
  Python 3.13 refuses a certificate without an authority key identifier, which OpenSSL's API does
  not add unless asked; curl and Node accept it.

## What was built

| Folder | What it is |
|---|---|
| [`boundary/`](boundary/) | #374's boundary. #374's modules are used unchanged (the build copies them in), and `main.zig` is #374's with the additions below |
| [`recorder/`](recorder/) | A recorder: #386's writer, unchanged, behind #370's protocol for sources |
| [`standin/hosts.py`](standin/hosts.py) | The hosts on the internet: a gate in front of #374's stand-in providers, Google Cloud Storage, Azure Resource Manager, and `github.com` on 443 and 22. Each accepts only the real value of its secret and logs which credential reached it |
| [`run/`](run/) | The two computers, the cases, the bench and the failure tests |

What the boundary adds to #374, file by file:

| File | What it does |
|---|---|
| `entry.zig` | Reads a `CONNECT host:port` (and a plain `GET http://…` for the metadata server), a PROXY v2 header, and the name in a TLS ClientHello without taking it off the socket |
| `mint.zig` | Makes a certificate for a host on first use, signed by Silo's authority, and keeps it. It starts a day back, so a computer whose clock fell behind still takes it (#378 found a guest an hour and a half behind after a night) |
| `swap.zig` | Replaces each placeholder in a request's head with its value, only on the way to that secret's hosts. A placeholder on its way to any other host refuses the request (`secret-placeholder`) |
| `source.zig` | The boundary as a source of the record: lines numbered per computer, kept until the recorder acknowledges them, sent again after a reconnect. Decisions wait for room; other lines are dropped when their buffer is full, and the recorder writes a gap |
| `dns.zig` | Answers c2's lookups. An allowed name gets an address of its own from `198.18.0.0/15`, so a connection handed over to that address names its host before TLS, as a `CONNECT` does. Any other name gets "no such name" |
| `conf.zig` | Which computer comes in on which socket, the hosts each may reach and whether Silo reads them, the secrets and their placeholders |

### How a connection goes through

1. A connection arrives on a computer's socket. From c1 it starts with `CONNECT api.anthropic.com:443`.
   From c2 it starts with a PROXY header whose destination is `198.18.0.1:443`, the address the
   boundary gave `api.anthropic.com`.
2. The boundary checks the host and port against the computer's list. It writes the decision to
   the record before it answers. A host not on the list gets 403 on c1 and a closed connection on
   c2; on c2 a name not on the list got no address in the first place.
3. For a host Silo reads, the boundary ends TLS with a certificate made for that name, and checks
   that the name the program asked for in TLS is the name it connected to. For a host it passes
   through, it reads that name from the ClientHello and checks it the same way, then carries the
   bytes.
4. For each request it reads, it swaps the placeholders, then applies #374: the credential must be
   one of this computer's placeholders, the model must have a price under a budget, and the call
   must fit the budget. It records the decision and the secrets used, by name, then sends the
   request on its own TLS connection to the host, to the address it looked up itself.
5. When the call ends, #374 counts it, and the count goes to the record with the bytes each way.

A line of the record from c2, as `silorec print` shows it:

```json
{"v":1,"seq":8,"prev":"…","time":"2026-11-25T11:31:49.718452119Z","computer":"c2","source":{"id":"boundary:c2","count":"k:5e0f2a91","n":8},"is":"fact","kind":"model-call","what":"admitted","decision":"allowed","decided_by":"rule","call":3,"host":"api.anthropic.com","line":"POST /v1/messages HTTP/1.1","model":"claude-opus-5-5","credential":"key","secrets":["anthropic"],"input_cost_bound_micro_usd":232,"reserve_micro_usd":2232}
```

## The cases

`run/cases.py`, in three settings of c2's gvproxy. Results in [`results/`](results/).

| Case | c1, `CONNECT` | c2, hand-off |
|---|---|---|
| Anthropic, OpenAI (a stream) and Gemini (the key in the query) with the computer's placeholder | the host got the real key | the same |
| Google's metadata token, then Cloud Storage with it | a placeholder, then the real token reached the host | blocked by gvproxy before the boundary, unless the deny list is cut and the flag fixed |
| Azure's metadata token (a JWT placeholder, #379), then Resource Manager | the same as Google | the same as Google |
| The Anthropic placeholder sent to Cloud Storage | refused, nothing reached the host | the same |
| A real key pasted into the computer, another computer's placeholder | refused by #374's check, nothing reached the host | the same |
| A host not on the list | 403 at the `CONNECT` | no address for its name |
| `github.com`, passed through unread | the host's own certificate | the same |
| SSH to `github.com:22` | through `CONNECT github.com:22` | straight to the name |
| TLS that names `storage.googleapis.com` on a connection to `api.anthropic.com` | refused in the handshake | the same |
| A client asking for HTTP/2 | gets HTTP/1.1 | the same |
| Plain HTTP to an allowed host; `CONNECT` to an address; garbage instead of `CONNECT` | 403 | |
| A lookup of `x7f3a.exfil.example` | | no answer; the boundary alone saw the name |
| A connection to an address no lookup gave, naming an allowed host in TLS | | allowed: the boundary dials the name, never that address |
| A private address on the local network | | refused, by gvproxy or by the boundary |
| A broken PROXY header on the hand-off socket | | closed |
| A connection that sends nothing | closed after 10 s | |
| Any real value in the record, the trace or the computers' files | none | none |

## Speed

`run/bench.sh`, 200 requests each (100 for streams), on a 4-core Linux container. "Direct" is the
same client on the host, straight to the stand-in, with no boundary. The boundary is built
`ReleaseSafe`. The stand-ins are Python, so the absolute times are the stand-ins'; the differences
are what the boundary and each way in add.

| Median, ms | Direct | c1, `CONNECT` | c2, hand-off |
|---|---:|---:|---:|
| TCP (and `CONNECT`) | 0.07 | 0.57 | 1.14 |
| TLS handshake with the program | 1.67 | 1.10 | 1.53 |
| A request on a new connection, placeholder swapped | 6.27 | 7.85 | 9.71 |
| A request on a kept-alive connection | 0.22 | 0.53 | 0.71 |
| First byte of a streamed model reply, new connection | 5.79 | 7.76 | 10.01 |
| 32 programs at once, new connections: calls a second | 354 | 243 | 250 |
| Download, host passed through, Gbit/s | 19.5 | 11.2 | 1.1 |
| Download, host read, Gbit/s | 20.4 | 3.9 | 1.0 |

The TLS handshake is faster through the boundary than direct because the boundary is Zig on
OpenSSL and the stand-in is Python. At 99th percentile, a kept-alive request was 0.43, 1.17 and
1.03 ms.

c2 downloads at 1 Gbit/s whatever the boundary does: that is gvproxy's netstack at MTU 1500, as
#380 measured. Reading a host costs c1 two thirds of its throughput (decrypting, reading the
body for #374, encrypting again): 3.9 Gbit/s, far above what an API sends.

With #378's relay as it is, c1 was 44 ms slower on every kept-alive request and on every first byte
of a stream ([`results/bench-relay-without-nodelay.jsonl`](results/bench-relay-without-nodelay.jsonl)).

## When a part fails

`run/failures.py`, results in [`results/failures.txt`](results/failures.txt). The run had room for
40 decisions and 20 other lines, a wait of 3 s for room, and a budget of $0.002 on c1.

| What failed | What the program saw | What the record shows |
|---|---|---|
| c1's budget reached | the second call got 400 with #374's sentence; so did the next four | the refusals, as decisions |
| The recorder stopped (`SIGSTOP`), 30 requests from c1 | 20 passed at full speed. Each of the next 10 waited 3 s and got 403 at the `CONNECT` | when it came back, every line it had not acknowledged, once; the chain holds |
| The same, a 200 MB download allowed before the stop | finished in 9.5 s, unhindered | its close |
| The recorder killed and started again, 5 requests from c2 | all passed | the lines sent again were written once |
| The recorder killed, 30 lookups from c2 (only observations) | all answered | one gap of 40 lines, the 20-line buffer having overflowed |
| The host stopped answering | waited until its own timeout (8 s): the boundary has none | |
| The host gone | 400 "the host cannot be reached", in 30 ms | the connection, closed with `host-unreachable` |
| The boundary killed | c1: connection reset at once. c2: no answer to lookups, at once | nothing: the lines it had not had acknowledged are lost, and the next start opens a new count, which the recorder cannot tell from a clean restart |
| The boundary started again | both worked again | |
| The relay killed (c1) | connection refused at once | |
| gvproxy killed (c2) | lookups time out (8 s) | |

Every failure closes the way out: nothing reached a host around the boundary.

A certificate made per host starts a day back. With Silo's authority made two days earlier, a
computer whose clock is 2 h or 23.9 h behind took it, and one 25 h behind refused it
([`results/clock.txt`](results/clock.txt)). The authority itself must also start in the past: the
one `run/tls.sh` makes starts now, and a computer 2 h behind refused it.

## Where the drafts conflict

Each is written as an agent's note in the entry it concerns, except where the entry is not on
`main` yet.

1. **`secret-placeholder` says "No proxy is configured inside", and #378, which #380 recommends
   for the first build, configures one.** The swap works the same on both ways in (above), so the
   sentence is the only conflict: the placeholder does not need the stack, only the boundary.
   Proposed in the entry's notes: "swapped in by the boundary, whichever way the computer's
   requests reach it".
2. **#380's hand-off drops the metadata address, and gvproxy's flag to allow it is broken.** For
   #379's `cloud-credentials` (on #356 and #379's branches, not on `main`) and `network-stack`.
   In hand-off mode the boundary decides every connection, so the deny list needs only the Mac's
   own addresses; the flag fix is one line and can go upstream with #380's five options.
3. **#374 tells computers apart by address; neither way in gives one.** One socket per computer
   for each way in, and one port for its lookups. For `network-stack`.
4. **#377 gives every computer the same placeholder, `placeholder-not-a-key`.** #374's check that
   a key was given to this computer, and the refusal of a placeholder at another host, need one
   placeholder per secret and per computer. The placeholder must also keep the shape programs
   read: Claude Code tells a key (`sk-ant-api03-…`) from a sign-in (`sk-ant-oat01-…`) by it (#374),
   and Azure's and Google's ID tokens must be JWTs (#379). For `secret-placeholder`.
5. **#379 swaps inside HTTP/2 headers; the boundary speaks only HTTP/1.1.** gRPC clients cannot
   fall back. The boundary needs HTTP/2 on hosts Silo reads that serve gRPC, before #379's HPACK
   swap has anywhere to run. For `ending-the-encrypted-connection`.
6. **#374 writes its own lines; #386 is the record.** Here #374's lines stay as a trace for
   debugging, and the boundary sends #386 lines for its decisions, swaps, counts and connections.
   The keys follow #386's table (`decision`, `decided_by`, `tokens`, `cost_micro_usd`), with
   `what` to tell the boundary's lines apart. #386's `connection` kind has one line per request;
   here a connection has a line when it opens, one per request that needed a decision, and one
   when it closes.
7. **`enforcing-and-recording` lets a request wait for room until the program gives up.** Here it
   waits 3 s and is refused with Silo's sentence. Without a limit, each waiting request holds a
   thread of the boundary and the program sees nothing until its own timeout: Claude Code retried
   for about three minutes in #378's test. For `enforcing-and-recording`.

## What it changes in the plan

| Entry | Note added |
|---|---|
| `secret-placeholder` | Conflicts 1 and 4, measured on both ways in |
| `network-stack` | Conflicts 2 and 3; the relay's 40 ms; what each way in adds |
| `enforcing-and-recording` | What a stalled recorder did, and conflict 7 |
| `ending-the-encrypted-connection` | Conflict 5; certificates per host, Python 3.13's check |

## For polarzero to rule

- `enforcing-and-recording`: a request waiting for room in the record waits until the program
  gives up (the entry's answer today), or a limit after which Silo refuses it with its sentence
  (recommended: the program then says why it stopped).

## Not built, or not measured

- Nothing ran on a Mac. vsock is a Unix socket here, as in #378; gvproxy runs as on Linux, over a
  stream socket, not Apple's datagram socket (#380 measured that one slower).
- #379's SSH agent and its refresh-token swap. Port 22 is passed through, and the agent's checks
  need only the bytes of the key exchange, which both ways in carry. A placeholder in a body is
  reported in the record, not swapped.
- HTTP/2 and WebSocket admission (#374 did not build the latter either).
- Closing the connections already open when a computer's list changes (`network-modes`).
- A time limit on the host's answer; a 502 instead of 400 for a host that is not a model's.
- The boundary as one process for every computer of a device: here it is, which is what
  `which-process-holds-the-boundary` has to weigh.
- Claude Code made one request the boundary refused as "not given": `GET
  /api/claude_code_penguin_mode` carries a credential Claude Code made itself. It went on.

## Running it

Linux, as root, with Zig 0.15.2, OpenSSL, zlib, zstd, Python 3, Go, iproute2 and `dhclient`.

```
run/tls.sh                                                   # test authorities, in run/tls
(cd boundary && zig build -Doptimize=ReleaseSafe && zig build test)
(cd recorder && zig build)
(cd ../2026-11-25-record-format/proto && zig build)          # silorec, to verify the record
# gvproxy with #380's patch (its build.sh), into /root/e2e/bin; with gvproxy-ec2-flag.patch too, into /root/e2e/bin2
run/cases-all.sh                                             # the cases, in three settings
O=/tmp/e2e/bench; BIN=/root/e2e/bin2 GVPROXY_ARGS="-no-gateway-api -no-host-loopback -outbound-deny host -ec2-metadata-access -outbound-handoff $O/c2.handoff.sock -dns-upstream 127.0.0.1:5302" run/up.sh $O
run/bench.sh $O; run/program.sh $O; run/down.sh $O
```

Every key and token in `run/config.py` is a fake.

The research at a glance: one Zig boundary takes both #378's `CONNECT` and #380's hand-off and
composes #374's budgets, #379's placeholders and metadata server and #386's record: 39 of 39 cases
pass on both, it adds 0.3 to 3.5 ms a request, and it found three integration faults (#378's relay
stalls 40 ms, #380's hand-off blocks #379's metadata server with a broken gvproxy flag, #374 cannot
tell computers apart on either way in).
