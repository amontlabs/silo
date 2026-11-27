# Step 6 under B: model traffic, spend and the receipt

Part of option B's specs ([README](README.md)). #373's `specs.md` specifies this step for A, at
#370's level: the boundary for model traffic, conversations, spend and a first receipt. Under B the
boundary is the same program, on the Mac, outside every computer, and it reads the same traffic.
Everything in #373's step 6 holds, apart from the four places below.

## The authority in main

Under A, Silo's authority goes into main's System keychain over SSH if #359's test 21 shows that
works with nobody at the screen, and into each program's own setting in every case. Under B it
goes into Linux's store, which every program on Ubuntu reads, at step 10 of the first run:

```
/usr/local/share/ca-certificates/silo.crt    then update-ca-certificates
/etc/environment                              NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt
                                              NODE_OPTIONS=--use-system-ca
                                              CODEX_CA_CERTIFICATE=/etc/ssl/certs/ca-certificates.crt
                                              REQUESTS_CA_BUNDLE, SSL_CERT_FILE, AWS_CA_BUNDLE likewise
```

This is what #347 ran on Linux, where Claude Code, Codex and eight more programs accepted the
authority, and #369 found three that need a setting of their own (Gemini CLI, Qwen Code, Aider).
Test 21 is not needed, and nothing about it can block the first night.

## Which process holds the boundary

`which-process-holds-the-boundary` answers "the helper of that computer". Under A the helper is
Swift, so #373 runs the boundary as a second process in the helper's launchd job, or builds Zig
inside the Swift build. Under B the helper is Zig, so the boundary is a library linked into the
helper, which is the answer read literally. One process holds main, its network stack's handoff,
the boundary and the way in.

What that costs: a crash of the boundary is a crash of the helper, and main stops with it
(`helper-on-broken-assumption`). The red team's malformed requests (#358) then become a way for an
agent to stop its own computer. The spec keeps the boundary in the helper, because that is the
answer, and adds a test: each of #358's seven requests, and the fuzz corpus of the readers, sent
from inside main, must never end the helper. If polarzero prefers the boundary in its own process
to keep main running through a boundary crash, it is #373's second process, unchanged.

## gvproxy before the boundary

Under A, gvproxy under a macOS computer is #359's test 07, which becomes blocking for A. Under B,
gvproxy with a libkrun Linux computer is what Podman machine runs on a Mac, with the same vfkit
datagram socket. #363's patch and stream 7's component apply unchanged. The throughput #366
measured for gvproxy (1.4 Gbit/s out at MTU 1500, 4.1 at 65520) applies; the helper sets main's MTU
to 65520 through the seed (`network: {version: 2, ethernets: {eth0: {mtu: 65520}}}`), since main's
network is Silo's on both ends.

## Mac test 34 in its Linux form

#373 adds test 34, a whole night of Claude Code on polarzero's Max plan and Codex signed in with
ChatGPT, in main, through the boundary. Under B it runs first on a Linux host with KVM (the kit's
test K14), where every part of it can run before the Mac does, then on the Mac. Its conditions
are #373's.

## What B adds to the receipt

Step 7's probe gives two items of `what-a-receipt-holds` that A marks "not known yet":

| Item | Under B |
|---|---|
| Still running | The processes of the conversation's tool calls that had not exited when its last reply ended, matched by #371's rule (the command text of the tool call against the process's command line). A report |
| Commands and exit codes | Each tool call's exit code as the program sent it to the model (a report read from the traffic), and beside it the exit the probe saw (a report from inside). #371 showed both; a mismatch between the two is shown |

Both are reports, because the probe runs inside main. The receipt says so.
