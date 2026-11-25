# The boundary's seven conflicts, settled

Research of 25 November 2026, by an agent, for polarzero. #395 put six drafts in one boundary and
found seven places where they disagree. This folder settles each one. Where code was wrong, a test
shows it failing first and then passing with the fix. The branch is #395's branch with these
changes merged on top.

Nothing here is a decision and nothing is built for Silo. The plan changed only by agents' notes
and one proposed answer, listed at the end.

## In short

| # | Conflict | Settled by | Before, after |
|---|---|---|---|
| 1 | #378's relay waits 40 ms on every reply | `TCP_NODELAY` on the program's connection in the relay inside the computer | 44 ms, 1.5 ms a kept-alive request ([`relay/`](relay/)) |
| 2 | #380's hand-off drops the metadata address that #379's server answers | In hand-off mode, gvproxy hands every TCP connection to the boundary before its own checks ([`metadata/`](metadata/)) | 35 of 37 cases, 39 of 39, with #380's own flags and with `-sealed` |
| 3 | #374 knows a computer by an address, and neither way in gives one | One socket per computer and way in, as #395 built it. #374 also had to refuse a connection from no known computer ([`computers/`](computers/)) | a key nobody was given reached the provider, refused |
| 4 | #377 gives every secret of every computer the same placeholder | One placeholder per secret and computer, made by Silo in the value's shape. The boundary refuses a shared one, and another computer's on every host ([`placeholders/`](placeholders/)) | 39 of 41 cases, 41 of 41 |
| 5 | #379's swap is in HTTP/2 headers, and the boundary speaks only HTTP/1.1 | HTTP/2 on hosts Silo reads that are marked for it, with nghttp2 on both sides ([`h2/`](h2/)) | every gRPC call failed, 6 of 6 cases pass |
| 6 | #374 writes its own lines, and #386 is the record | Already settled in #395's code: #374's lines stay a trace for debugging, and the boundary sends #386 lines for its decisions | nothing to fix |
| 7 | A request waits for room in the record until the program gives up | polarzero's call, unchanged from #395: a limit after which Silo refuses with its sentence is recommended | |

Two findings came up on the way:

- **#374 let a connection from an unknown address skip both its checks.** With a $0.000001 budget
  on main, a call from `127.0.0.9`, which is no computer, carried a key nobody was given and reached
  the provider. #374 ran its key check and its budget only `if (comp) |k|`, so they were skipped
  when no computer matched.
- **Claude Code does not read a credential's shape.** #395 wrote that Claude Code tells a key from a
  sign-in by its placeholder's shape. Claude Code 2.1.332 sends `x-api-key` for
  `ANTHROPIC_API_KEY` and `Authorization: Bearer` for `CLAUDE_CODE_OAUTH_TOKEN`, whatever the value
  looks like, `placeholder-not-a-key` included ([`results/claude-shapes.txt`](placeholders/results/claude-shapes.txt)).

## 1. The relay's 40 ms

[`relay/test.sh`](relay/test.sh) runs the relay that sits inside the computer (#378's, as #395
copied it) in a network namespace, in front of a stand-in for the boundary that answers each
request in two writes, the head and then the body a millisecond later. A client in the computer
makes 50 requests on one kept-alive connection.

| Relay | Median | Slowest |
|---|---:|---:|
| #378's, `NODELAY=0` | 43.98 ms | 46.43 ms |
| with `TCP_NODELAY` on the program's connection | 1.45 ms | 2.55 ms |

The 1.45 ms includes the stand-in's 1 ms gap. Without `TCP_NODELAY`, the relay holds the body
until the program acknowledges the head, and the program delays that acknowledgement by 40 ms. The
relay runs in the computer's Linux on both systems, so the fix is one line in one place. #395's
copy of the relay already sets it; #378's draft needs the same line. Without the stand-in's gap
both relays pass, since the two writes then reach the relay as one: the stall shows only when the
body comes after the head, as it does when the boundary streams a reply.

## 2. The metadata server behind the hand-off

#380's gvproxy checks the cloud metadata address and its deny list before it hands a connection
to the boundary. In hand-off mode gvproxy dials nothing itself, so those checks protect nothing:
they only hide connections from the boundary, which answers the metadata server itself (#379)
and refuses every address it was not given a name for.

[`gvproxy-handoff-decides.patch`](metadata/gvproxy-handoff-decides.patch), on top of #380's patch,
moves both checks behind the hand-off. Its Go test opens a guest connection to `169.254.169.254`,
to `10.0.0.7` and to a public address under `-outbound-deny private,host`, and expects all three at
the hand-off socket. Without the change, the first two were refused
([`results/go-test.txt`](metadata/results/go-test.txt)).

[`metadata/cases.sh`](metadata/cases.sh) runs #395's cases on the second way in:

| gvproxy | Flags | Cases | Metadata |
|---|---|---|---|
| #380's patch | `-outbound-deny private,host`, as #380 runs it | 35 of 37 | refused by gvproxy |
| with this patch | the same | 39 of 39 | Google's and Azure's tokens from the boundary |
| with this patch | `-sealed` | 39 of 39 | the same |

A private address on the local network still reached nothing: the boundary took the connection
and closed it. #395's other fix, cutting the deny list to `host`, also works, but `-sealed` puts
`private` back, and a computer started sealed with a hand-off would lose its metadata again.

gvproxy's `-ec2-metadata-access` flag is a separate bug, in upstream since commit `c58b9368`: it
sets a field the forwarders never read. [`gvproxy-ec2-flag-with-test.patch`](metadata/gvproxy-ec2-flag-with-test.patch)
is #395's one-line fix with a test that fails on `197eea05`
([`results/go-test-ec2-flag.txt`](metadata/results/go-test-ec2-flag.txt)). It is ready for the
upstream proposal (backlog 14); sending it is public, so it is polarzero's call.

## 3. Telling computers apart

#395's boundary knows a computer by the socket a connection arrives on: its own listener for
`CONNECT`, its own hand-off socket, its own port for lookups. On the Mac each virtual machine has
its own vsock device, so a listener per computer comes with it. This needs one gvproxy per
computer, as #380 runs it.

#374 knows a computer by the local address a connection arrives on, and runs its two checks only
when an address matches. [`computers/test.sh`](computers/test.sh) builds #374's boundary as it is,
and then with [`unknown-computer.patch`](computers/unknown-computer.patch), and makes three calls:

| Call | #374 | Patched |
|---|---|---|
| main, a key it was not given | refused | refused |
| main, its own key, a second call over its budget | refused | refused |
| `127.0.0.9`, which is no computer, a key nobody was given | **reached the provider** | refused: "this connection comes from no computer Silo knows, so nothing it sends can leave" |

## 4. One placeholder per secret and per computer

Say main holds two Anthropic keys, one for work and one of your own. Both go to
`api.anthropic.com`. With #377's one placeholder for every secret, a request cannot say which key
it means, and #395's swap sent the first key while the record named both as used.

#395's own test settings had the same fault in a smaller form: both computers got the same Azure
placeholder. A case added to #395's [`run/cases.py`](../2026-11-25-boundary-end-to-end/run/cases.py)
sends one computer's Azure placeholder from the other. It reached Azure with the real token on both
computers ([`cases-before.txt`](placeholders/results/cases-before.txt)): #374's check refuses
another computer's placeholder only on model hosts.

Changed in #395's boundary, on this branch:

- `conf.checkPlaceholders`: the boundary refuses to start when two placeholders are equal, when one
  contains another (the swap replaces substrings), or when one overlaps a value. #395's old settings
  now stop it with `PlaceholderShared` ([`load-395-config.txt`](placeholders/results/load-395-config.txt)).
- `swap.fromOther`: a placeholder Silo gave another computer is refused on every host Silo reads,
  and the record names whose it was.
- [`placeholder.zig`](../2026-11-25-boundary-end-to-end/boundary/src/placeholder.zig) and
  `boundary placeholder`, which reads a value on stdin and prints a placeholder for it. It keeps a
  known prefix (`sk-ant-api03-`, `ghp_`, `AIza`, 28 in all), the length and the kind of each
  character, and draws the rest at random. A JWT keeps its header and claims and gets a new
  signature. A value that would leave fewer than 96 random bits is refused.
- `run/config.py` gives each computer its own Azure placeholder.

After: 41 of 41 cases ([`cases-after.txt`](placeholders/results/cases-after.txt)).

A GitHub token's last six characters are a checksum. A placeholder made this way fails it, so
GitHub's secret scanning would not report a placeholder pushed to a public repository as a leaked
token. `gh` does not check it.

## 5. HTTP/2 at the boundary

gRPC needs HTTP/2 and cannot fall back. [`h2/test.sh`](h2/test.sh) runs a gRPC client in each
computer against a stand-in Google API that speaks gRPC ([`h2/grpc/`](h2/grpc/), grpc-go 1.72): one
call and one stream of three messages with trailers, with the computer's placeholder as its token.
Before, every call failed in the handshake: "missing selected ALPN property"
([`before.txt`](h2/results/before.txt)).

[`h2.zig`](../2026-11-25-boundary-end-to-end/boundary/src/h2.zig) ends HTTP/2 on both sides with
nghttp2 1.59. A host in the boundary's settings marked `"h2": true` gets HTTP/2 when the program
offers it. Each request's headers go to the same checks and swap as an HTTP/1.1 head, written as
`name\0value` lines, so `swap.zig` runs unchanged. Bodies, responses and trailers are carried stream
by stream. A refused gRPC call gets `PERMISSION_DENIED` with Silo's sentence, which the client
prints.

| Case, on c1 (`CONNECT`) and c2 (hand-off) | Result |
|---|---|
| gRPC call and stream with the computer's placeholder | the host saw the real token, 3 messages and the trailer came back |
| the Anthropic placeholder sent to the gRPC host | `PermissionDenied desc = Silo: this request carries a placeholder that may not go to this host.`, nothing reached the host |
| the other computer's placeholder | `PermissionDenied`, nothing reached the host |
| the record of each computer | `silorec verify`: the chain holds |

Model hosts stay on HTTP/1.1, where #374 counts calls, and the existing case "a client asking for
HTTP/2 gets HTTP/1.1" still passes on them.

## What it changes in the plan

| Entry | Change |
|---|---|
| `secret-placeholder` | The answer, which has no verdict, now says "swapped in by the boundary, whichever way the computer's requests reach it" in place of "the network stack" and "No proxy is configured inside", and adds "Each secret has its own placeholder in each computer". The notes give the cases, and correct #395 on Claude Code |
| `network-stack` | Notes: the relay's fix, the hand-off before gvproxy's checks, the socket per computer and #374's unknown computer |
| `ending-the-encrypted-connection` | Notes: HTTP/2 built for marked hosts, measured with gRPC on both ways in |

`plan check` passes.

## For polarzero to rule

- `enforcing-and-recording`, from #395, unchanged: a limit after which a request waiting for room in
  the record is refused with Silo's sentence (recommended), or no limit.
- `secret-placeholder`: the answer's new sentences, above. With #378's proxy inside, a program that
  ignores the proxy settings reaches no host, so it cannot use a placeholder either. With the
  hand-off it can. That is a reason for the hand-off, not for the old sentence.
- The gvproxy bug and the hand-off change go upstream with #380's options, when polarzero sends
  them.

## Not built, or not measured

- Nothing ran on a Mac.
- HTTP/2 is offered only on hosts marked for it. Offering it wherever the host offers it needs the
  boundary to reach the host before it answers the program's handshake. That is the recommended
  next step.
- HTTP/2 flow control: the boundary takes every byte a side sends and holds it until the other side
  reads it. A host that sends faster than the program reads grows the boundary's memory without
  limit. Not measured, and no speed was measured for HTTP/2.
- The gRPC stand-in uses gRPC's health service, not Pub/Sub's API, and the client is grpc-go, not
  Google's Go or Node libraries.
- A placeholder in an HTTP/2 body is neither swapped nor reported.
- The shapes of placeholders were checked only against Claude Code's choice of header.

## Running it

As root on Linux, after #395's setup (its README, "Running it"), with #380's gvproxy in
`/root/e2e/bin` and the same with `gvproxy-handoff-decides.patch` applied in `/root/e2e/bin3`, and
nghttp2's headers (`libnghttp2-dev`):

```
relay/test.sh 0; relay/test.sh 1                   # #378's relay, then with TCP_NODELAY
metadata/cases.sh                                  # #395's cases before and after the gvproxy patch
(cd ../2026-11-24-spend-budgets && run/tls.sh); computers/test.sh before; computers/test.sh after
placeholders/claude-shapes.sh                      # needs claude on PATH
(cd h2/grpc && go build -o /root/e2e/h2/grpc-standin ./standin && go build -o /root/e2e/h2/grpc-client ./client)
h2/test.sh after
```

The research at a glance: the seven conflicts #395 found between the boundary's drafts are settled
on one branch, each code fault shown failing first: #378's relay 44 ms to 1.5 ms, #379's metadata
behind #380's hand-off, one placeholder per secret and computer, and gRPC over HTTP/2 through the
boundary on both ways in. On the way it found that #374 let a connection from an unknown address
skip its key and budget checks.
