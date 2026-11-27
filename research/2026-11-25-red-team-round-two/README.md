# Red team, round two: the components of 25 November

Research of 25 and 26 November 2026, by an agent, for polarzero. Round one (#358, fixes in #363)
attacked the boundary of 24 November; #401 fuzzed the drafts' parsers. This round puts a hostile
main against the components drafted on 25 November: the network card's Mac end `cardend` (#402),
gvproxy as #409 holds it, the vsock relay and proxy (#378, #403), the metadata server and SSH agent
(#379), the key-or-plan switch (#393), the team ledger (#388) and the price table (#396). Nothing
here is built, and no entry is taken by it.

Two components were attacked and fixed with tests that fail on the draft and pass with the patch.
The other five were read for attacks, and what was found is written below as **read, not run**:
the time ran out before their tests were written. Nothing already in #358, #363 or #401 is counted
again.

## Shown failing, with a fix

### 1. A frame shorter than an Ethernet header cuts a computer off, or ends gvproxy (#409, #380)

#409 found that a second `VFKT` silently kills a computer's link and left the cause untraced. It is
any frame under 6 bytes, and frames of 6 to 13 bytes are worse. In gvproxy v0.9.0,
`Switch.rxBuf` reads the source address under `camLock` without checking the length. Under 6
bytes, the slice panics while the lock is held; the deferred `disconnect` then waits for that same
lock for ever, so the panic never reaches the top. The computer's goroutine is stuck, nothing is
logged, the process stays up, and it no longer stops on SIGTERM. A frame of 6 to 13 bytes sent to
the gateway or to everyone panics after the lock is released, and gvproxy ends.

[`gvproxy/short-frames.py`](gvproxy/short-frames.py) on gvproxy v0.9.0 built from its tag
([`results/short-frames.txt`](gvproxy/results/short-frames.txt)):

| Frame | As released | With `gvproxy-runt.patch` |
|---|---|---|
| A second `VFKT` (4 bytes) | link lost, SIGTERM hangs | dropped and counted, link up |
| 5 zero bytes | link lost, SIGTERM hangs | dropped and counted |
| 6 or 12 bytes to everyone, 13 to the gateway | gvproxy ends (panic) | dropped and counted |
| 6 zero bytes, a 14-byte header alone | link up | link up |

[`gvproxy/gvproxy-runt.patch`](gvproxy/gvproxy-runt.patch) (9 lines): `rxBuf` drops any frame under
14 bytes, counts it, and logs the first one only, so a computer cannot fill the log.

Can main send one? Not from user space on Linux: the kernel refuses a packet-socket frame under 14
bytes on an Ethernet card, even for root with `CAP_SYS_RAWIO`
([`gvproxy/linux-refuses-short.py`](gvproxy/linux-refuses-short.py): `EINVAL` for 4, 6 and 12
bytes, 14 bytes delivered). Root that loads a kernel module and writes the virtio queue itself is
not stopped by that check; whether Apple's card or libkrun's passes such a buffer on is not known.
So the patch is needed for any gvproxy facing a computer, and the helper must not count on SIGTERM:
it stops a gvproxy that has not ended within a grace with SIGKILL.

### 2. `cardend` lets main fill the Mac's memory and disk, and its stack sees main's SYNs (#402)

- **The drop count has no bound.** Every SYN, UDP datagram, ICMP or ARP to a new address is a new
  line held in memory and written to the counts file twice a second. A scan from inside of
  200,000 addresses left cardend at 132 MiB with an 8 MB file; a million did not finish in 110 s,
  as the rewrites slowed it. With the fix: a million in 5 s, 13 MiB, a file under 10 KB, and every
  frame still counted ([`cardend/results/flood.txt`](cardend/results/flood.txt)). The fix keeps 256
  lines, then counts new destinations in an "other destinations" line of their kind.
- **#402 says "A SYN from main never reaches [the stack]". It does.** The filter passes any TCP
  segment from port 22 to the Mac end, SYN included, and any IPv4 fragment whose first bytes look
  like such a header, which gVisor then holds for reassembly. Nothing listens, so the stack answers
  with a reset, but the claim that main cannot open anything rests on the filter alone. The fix
  drops a SYN without ACK and every fragment.

Test: [`cardend/attack_test.go`](cardend/attack_test.go), put beside #402's `main.go`; both tests
fail on the draft and pass with [`cardend/cardend-bounded.patch`](cardend/cardend-bounded.patch)
([`results/attack-test.txt`](cardend/results/attack-test.txt)). `flood.py` is the live run.

## Read, not run

Each is an attack read off the draft's code, with the fix it calls for. None was run.

3. **The metadata server's placeholders reach hosts anyone can name (#379).** An Azure access
   placeholder is swapped for the real token on `*.azure.com`, which takes in
   `<name>.<region>.cloudapp.azure.com`, a name anyone with an Azure account gives a virtual
   machine. And a Google ID token's audience is whatever main asks
   (`identity?audience=https://collector.example`): the boundary mints a real, Google-signed token
   for it and swaps it in on that host. Fix: a placeholder holds the exact host of its token's
   audience, never a wildcard over names others register, and ID tokens only for the audiences the
   person listed.
4. **#378's proxy gives main as many threads and sockets on the Mac as it opens.** One thread per
   connection, no limit, no timeout on the request's head; a `CONNECT host:abc` raises in the
   thread and leaves the socket open. #401 covered the second request on a connection, not this.
   Fix: a limit of connections per computer, a deadline for the head, close on every error. The
   Zig boundary of #395 that replaces it should be checked for the same.
5. **The key-or-plan switch decides on a header reader that a host reads differently (#393).** In a
   computer set for a key, `signInOf` splits lines on CRLF only. #401 showed Anthropic's host also
   ends a header at a bare LF, so `x-pad: a\nAuthorization: Bearer <plan>` is one header to Silo and
   a plan sign-in to Anthropic: plan traffic then goes through a connection Silo reads, which is the
   one thing the setting exists to prevent. #401's strict reader closes it; `signInOf` should also
   answer "plan" for anything it cannot prove is a key.
6. **Anyone who can push to the team's repository can rewrite the Git ledger (#388).** The ledger
   is one JSON file on a branch, and a fast-forward push is the only check. Main pushes code to the
   team's repositories, so it can raise the budget or wipe the grants. Signing ledger commits does
   not help if it is Git's own signing: #379's agent signs anything in the `git` namespace for main.
   Fix: each Mac signs its ledger commits with a key no computer reaches, in a namespace of its
   own, and each Mac checks the chain from a first commit it keeps. The ledger server also takes
   any amount from anyone: a negative or `-Infinity` cost on `/settle` raises the budget, and
   `/give` in another Mac's name frees its slice for others while it still spends it. Fix:
   authenticate each Mac, take the Mac from the key and not the body, refuse amounts that are not
   finite and positive.
7. **The price table's `check_by` is never enforced (#396).** A release past its date is used as
   if current, with nothing in the record. Main cannot cause this, but anyone who keeps the Mac on
   an old release (a blocked update, a stopped server) can. Fix: record "prices past their check
   date" and show it; what a budget does then is for polarzero to rule. From main's side the table
   held: the reply's tier and model are what is priced.

The SSH agent's unbounded message length, the Host-header swap and the proxy's second request are
#401's and are not repeated.

## What this changes in the plan

No entry was changed in this PR. The notes it calls for, for the entries' own threads to add:
`network-stack` (the runt patch and SIGKILL, with #409's notes), `way-in-before-the-relay` (the
SYN and fragment filter, the bounded count), `network-place` (the bound), `cloud-credentials`
(exact hosts, listed audiences), `claude-key-or-plan` (decide on the strict reader),
`pooled-budget-ledger` (signed ledger, authenticated Macs), `where-prices-come-from` (stale table).

## Not verified

- Nothing ran on a Mac. Whether Apple's network card or libkrun hands a frame under 14 bytes to
  its socket is open; #409's Mac question 4 stays.
- gvproxy was built from the v0.9.0 tag with Go 1.26, not the released binary #409 used; the
  outcome of a second `VFKT` matches #409's.
- `cardend` ran as a process fed by a Unix datagram socket, with no namespace and no SSH (this
  container has no `ip`), so the way in was not run again with the fix.
- Findings 3 to 7 were not run.

## Run it

```
python3 gvproxy/short-frames.py <gvproxy binary>       # as root; Python 3
python3 gvproxy/linux-refuses-short.py                 # as root, needs /dev/net/tun
cp cardend/attack_test.go <#402's cardend>/ && (cd <#402's cardend> && go test -run Attack -v .)
python3 cardend/flood.py <cardend binary> 1000000
```

Drafts tested: #402 at `fbc9606`, #409 at `a86ae61`, gvisor-tap-vsock at `v0.9.0`.
