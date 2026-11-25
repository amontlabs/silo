# Telling who drives an SSH session through the boundary, measured

Research of 25 November 2026, by an agent, for polarzero. #379 showed that the agent the boundary
holds for `ssh-keys` signs for an outsider's session when a program in the computer carries it: the
outsider reaches that program (a WebSocket to an allowed host is enough), the program opens the
TCP connection to `github.com`, and the outsider's own SSH client runs through it, asking the
computer's agent for the signature the same way. The boundary sees a real connection from the
computer, to an allowed host, with the right host key.

This folder asks whether the boundary can still tell that login from one a program in the computer
makes itself. Four ways were built on Linux, on top of #379's prototype, and measured with OpenSSH
9.6, Paramiko 5.1, asyncssh 2.24 and Go's `x/crypto/ssh`, 30 logins per case, idle and with twice as
many busy processes as CPUs. The outsider ran OpenSSH, or a Paramiko client built to get past the
check, at round trips from 0 to 50 ms from the computer. The real host was 20 ms from the boundary.

## What was found

**1. A check made of messages tells nothing.** The computer's OpenSSH was made to ask the boundary
to check each host key (`KnownHostsCommand`), and the agent signed only for a connection whose
host key had been checked that way. It worked for the local client, and refused one that skipped
the check. The outsider passed its own check request through the tunnel, as it passes its agent
requests, and logged in. Any check the computer takes part in fails the same way: whatever the
computer can receive or send, the program that carries the outsider can forward.

**2. The shape of a session after login tells nothing.** During a 4 MB download, a local OpenSSH
answered the server's data after 7.1 ms at the median, and an outsider 2 ms away after 0.5 ms.
Window adjustments are not answers to one packet, so the time they take says little.

**3. The time of the key exchange tells little.** The client's first encrypted packet is
encrypted with keys derived from the server's reply, so it cannot reach the boundary sooner than
one round trip to wherever the session keys are made. But clients take long to make them:

| Client, idle | Reply to first encrypted packet, median |
|---|---|
| OpenSSH 9.6, default exchange (`sntrup761x25519-sha512`) | 19.7 ms |
| OpenSSH 9.6, `curve25519-sha256` only | 2.6 ms |
| Paramiko | 42.6 ms |
| asyncssh | 1.3 ms |
| Go | 0.6 ms |
| outsider's OpenSSH 10 ms away | 30.3 ms |

A local OpenSSH looks like an outsider 20 ms away, and Paramiko like one 40 ms away. An outsider
with a fast exchange gets that slack for free.

**4. The time from the signature to the client's next packet tells.** The signed login request
has to reach whoever holds the session keys, and come back encrypted. Every local client answered
in under 0.7 ms at the 95th percentile when idle; under load, the slowest answer was 10.7 ms. An
outsider was never faster than its round trip: 2.5 ms at a set 2 ms, 6.6 ms at 5 ms, 20.7 ms at
20 ms.

**5. A deadline after the signature stops a plain outsider.** `boundary.py` closes a connection,
without forwarding the packet, when a client packet arrives more than 5 ms after the agent signed
for it and the server has said nothing since. The signature is for that connection's session id
only, so it dies with it. Every login of an outsider 5 ms away or more, using OpenSSH through the
tunnel, was cut: 120 of 120. Local logins were cut 0 of 150 times idle, and 7 of 150 times under
load (Paramiko 1, asyncssh 3, Go 3), each one late by at most 11.4 ms. A 12 ms deadline would have
cut none of the local logins measured, and still stopped the outsiders at 20 ms and 50 ms.

**6. An outsider who knows of the deadline gets past it from far enough away.** `attacker_decoy.py`
sends the server a packet of an unknown type at the moment it asks for the signature. The server
answers it with `SSH_MSG_UNIMPLEMENTED`. When that answer reaches the boundary before the signed
request, the deadline is lifted. With the server 20 ms from the boundary, this outsider was cut
every time at 5 and 10 ms, got in 28 of 30 times at 20 ms, and every time at 50 ms. A stricter
rule (every client packet during login must come within the deadline of the last thing the
boundary handed over) was not built: an outsider can keep up a stream of such packets from before
the signature, timed to the server's answers. Telling those packets apart needs the sizes of the
server's encrypted answers, which depend on the server's software.

**7. Not covered by any of these: a program in the computer that logs in itself and hands its
session keys out.** Every check above sees a local client, because one is there. That program is
code in the computer, and only what sees processes can name it (stream "attribution without
processes").

## What it changes in the plan

| Entry | Change |
|---|---|
| `ssh-keys` | Notes added as an agent's part, "Who drives a session": what was measured, and three options (keep the agent and state its limit; add the deadline, recommended; end SSH at the boundary). The entry's answer is unchanged. |

No verdict was written and no list changed.

## For polarzero to rule

- `ssh-keys`: add the deadline after each signature to the agent. It closes the case of an outsider
  using a stock SSH client through a tunnel from farther than the deadline, and each signature's
  fact records how soon the client answered. It does not close the case of an outsider who built a
  client against it from farther than the server, and the entry should keep saying the agent does
  not know who drives a session.

## How to run it

As root on Linux with OpenSSH, socat, stress-ng, Go and Python's `cryptography`, `paramiko` and
`asyncssh`:

```
python3 run.py [workdir] [logins per case]
```

It writes [`results.txt`](results.txt) (the run of this report, 30 logins per case, about 45
minutes on 4 CPUs).

| File | What |
|---|---|
| `boundary.py` | #379's `silo_ssh.py` with timing added: the key-exchange gap, the deadline after each signature, the host-key check socket, and a trace of every chunk's time and size |
| `delay.py` | A delay line that stands for a network path. The container had no `netem` |
| `attacker_decoy.py` | The outsider that sends a packet the server must answer when it asks for the signature |
| `client_*.py`, `client_go.go` | The local clients, from #379 |

## Not verified

- No Mac. The computer here is a set of processes next to the boundary, not a virtual machine
  behind vsock or gvproxy. The deadline has to be set from the same measurements on a Mac, with
  the virtual machine's CPU busy.
- The delay line rounds short waits up to about 1 ms each way, so a set 2 ms round trip measured
  about 2.5 ms. The measured gaps are given as measured.
- The real host was OpenSSH's `sshd`. GitHub's SSH server was not used: whether it answers a
  packet of an unknown type before login, as finding 6 needs, was not checked.
- OpenSSH 10's default exchange (`mlkem768x25519-sha256`) was not run; it should be as fast as
  `curve25519-sha256`. JGit and go-git were not run.
- How near an outsider can get to a person's home on a rented server was not measured. The
  deadline stops only those farther away than it.
