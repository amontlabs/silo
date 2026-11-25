# The parsers of the drafts, fuzzed and compared

Research of 25 November 2026, by an agent, for polarzero. `fuzz-untrusted-parsers` says every
parser of bytes a computer controls is fuzzed for crashes, hangs and broken assertions. Eight
drafts of 24 and 25 November added such parsers as prototypes. This folder lists them
([`inventory.md`](inventory.md)), fuzzes the ones that run, coverage-guided, and compares each
reader of a format another program also reads against that program, as #376 proposes in
`parsers-fuzzed-and-compared`.

Nothing here is built, and no entry is taken by it. The drafts' code is tested at the commits
`fetch.sh` names. Two patches show fixes; they are not pushed to the drafts' branches.

## What was found

Every finding below was shown failing first, by the test named beside it, on the draft's own
code. The output is in [`results/`](results/).

### The spend boundary (#374, #385)

1. **Six spellings of Anthropic's model route carry a model call past a budget, uncounted.** The
   boundary counts a call when its path is exactly `/v1/messages`. Anthropic's host answered
   `https://api.anthropic.com/v1/messages`, `//v1/messages`, `/v1//messages`, `/v1/./messages`,
   `/v1/x/../messages` and `/%761/messages` as its model route (an authentication error, where an
   unknown path gets 404). Google's host does the same with a trailing slash after
   `:generateContent`. OpenAI's spellings were counted, because the boundary matches the end of the
   path. Against a stand-in host that routes the way the real ones answered, a computer with a $50
   budget had 10 calls served, $240 at the host's counts, and the boundary counted 3. Test:
   `boundary/e2e.py paths`. Real hosts: `boundary/live.sh`.
2. **A model call hidden in the body of another request leaves uncounted.** The boundary reads a
   header line up to CRLF; Anthropic's and OpenAI's hosts also end a header at a bare LF, and fold
   a line that starts with a space into the header before it. So `x-pad: a\ntransfer-encoding:
   chunked` is one header to the boundary and two to the host, and ` content-length: 98` on its
   own line is a length to the boundary and part of `x-pad` to the host. Either way the host reads a
   second request, a model call, where the boundary read a body. The call's answer reaches the
   program on its next request. Neither the call nor its key was seen by the boundary. Test:
   `boundary/e2e.py smuggle`.
3. **A request of 64 KiB took 5.4 GB of the boundary's memory.** The boundary unpacks a request
   body sent with `content-encoding: zstd` and keeps all of it before it counts the call. 64 KiB of
   zstd unpacks to 2 GiB of spaces; the boundary's peak went from 7 MiB to 5,407 MiB on one request.
   A few at once end the process, and every computer's model traffic with it. Test:
   `boundary/e2e.py memory`.
4. **A WebSocket message that ends in a final DEFLATE block stops the connection for good.**
   RFC 7692 lets a sender set BFINAL on a message's last block. The reader's inflate loop then gets
   `Z_STREAM_END` with input left and never ends: the relay thread spins at full speed and nothing
   more passes either way. Either side can send it, the program or the host. The fuzzer found it in
   15 seconds from nothing. Test: [`repro/ws_final_block.zig`](repro/ws_final_block.zig).

[`repro/boundary-strict-requests.patch`](repro/boundary-strict-requests.patch) reads a program's
request strictly (CRLF only, no folded line, a token before each colon, one length in digits,
`chunked` alone, an origin-form path with no empty, `.` or `..` segment, no `%` and no trailing
slash) and holds at most 64 MiB of a body, packed or unpacked.
[`repro/ws_final_block.patch`](repro/ws_final_block.patch) ends a message at its final block.
With both, the same tests give: every spelling and framing refused with a 400, 3 calls served and
counted, the boundary's peak at 134 MiB, and the WebSocket test passing
([`results/boundary-patched.txt`](results/boundary-patched.txt)). The unit tests of #385 still
pass. The patches were not tried with Claude Code, Codex and Gemini CLI, which #374 ran.

### The credential swap (#379)

5. **The swap of a placeholder for a real token follows the Host header, not the TLS name.** A
   connection whose TLS name is `collector.example` and whose Host header says
   `storage.googleapis.com` got the real Google token swapped in. The prototype forwards every
   request to one stand-in, so nothing leaked here. The spend boundary connects to the host the TLS
   name gives. A boundary that swaps by one name and connects by the other sends the real token to
   whoever the program names. Test: `python/cloud_swap_host.py`.
6. The SSH agent held under 6.5 million fuzzed messages in modes `silo` and `silo+infer`: it signed
   nothing but SSHSIG in the `git` namespace without a bind. A message shorter than its fields
   raises instead of answering FAILURE, which drops that agent connection, and a message's length
   is not bounded (OpenSSH's agent stops at 256 KiB). Harness: `python/fz_ssh_agent.py`.

### The ext4 reader (#381)

7. **A file the computer sees can be hidden from Silo.** A directory entry whose type byte is 0xDE
   is skipped by the reader, which takes 0xDE for the checksum tail. Linux lists the file, with an
   unknown type, on a file system without `metadata_csum`. Root in the computer can write that byte.
8. **Silo can see files Linux does not.** Entries past a directory's size (9 files here), and
   entries of a directory block whose checksum fails (30 files; Linux refuses the block), are read
   by the reader and not by the kernel. A name with a NUL byte is `file\0with_…` to the reader and
   `file` to Linux.
   Test for 7 and 8: `python/ext4_vs_kernel.py`, which mounts each image read-only through a loop
   device and compares with the reader's walk. Three other cases read the same.
9. Coverage-guided fuzzing found six ways the reader crashes instead of refusing: `struct.error`
   on a short buffer, `IndexError` on a group or a directory entry past the end, and
   `ZeroDivisionError` on zero inodes or blocks per group. Each stops the whole comparison rather
   than writing a gap. The reader is a Python trial for a Zig reader; these are the checks the Zig
   reader needs. Harness: `python/fz_ext4.py`, 141,000 runs over two images, one with a journal.

### The previews (#384)

10. **An app in the computer can take its preview out of Silo's Content-Security-Policy.** The
    forwarder reads the app's answer up to the first CRLF pair and adds Silo's headers after it.
    An app that ends its head with bare LFs and puts a CRLF pair in its body gets Silo's headers
    written into its body: Chromium reads the head at the bare LFs, so the page runs with no CSP,
    no permissions policy and no `frame-ancestors`. Its script reached a host `connect-src 'self'`
    forbids. The browser's rule proxy still decides by name, so this removes a second layer, not
    the first. Test: `python/preview/run.sh`.
11. The local-model gate reads requests strictly, but a bare LF inside a header's value passes, and
    Go's reader (Ollama's) splits it into a header the gate strips, such as `Origin`. Nothing more
    came of it: the gate writes its own length and its own JSON. Test: `python/gate_header_lf.py`.

### The closed network (#378) and gvproxy (#380)

12. The closed network's proxy checks the first request's host only, then joins the two ends. A
    second request on the same connection goes to whatever name the host behind the allowed
    address serves, and the log shows one request. Test: `python/closed_network_host.py`.
13. The deny list of `gvproxy-silo.patch` lets through an address that carries a private IPv4
    address in a NAT64 or 6to4 form (`64:ff9b::192.168.1.1`, `2002:c0a8:0101::`). gvproxy gives
    the computer IPv4 only today, so this matters only once it carries IPv6. Test:
    `go/deny_fuzz_test.go`, a Go fuzz test to add to the patch.

### The record (#386)

14. The record's verify held. The harness writes each segment header, each line's chain prefix and
    each frame's CRC itself, so that fuzzed bytes reach the JSON reader and every check after it.
    1.16 million runs, no crash, no hang, and no record verify called whole with other lines than
    it holds. #386 had already found and fixed the one crash its own fuzzer found.

## How much was fuzzed

| Parser | Draft | Engine | Runs | Found |
|---|---|---|---|---|
| Chunked framing, SSE, tool-call watcher, tool results (`stream.zig`) | #374/#385 | libFuzzer | 6.1 M | nothing |
| Usage meter, four formats (`usage.zig`) | #374/#385 | libFuzzer | 17.4 M | nothing |
| Model request reader, credential, plan windows (`spend.zig`) | #374/#385 | libFuzzer | 7.6 M | nothing |
| WebSocket reader (`ws.zig`) | #374/#385 | libFuzzer | 14.4 M after the fix | the hang (4) |
| Record verify (`verify.zig`) | #386 | libFuzzer | 1.16 M | nothing |
| SSH agent messages (`silo_ssh.py`) | #379 | atheris | 6.5 M | robustness (6) |
| ext4 reader (`ext4read.py`), with and without a journal | #381 | atheris | 141 k | six crashes (9) |
| Deny list (`outbound.go`) | #380 | Go fuzzing | seeds | NAT64, 6to4 (13) |

The four HTTP findings (1, 2, 10, 12) and the ext4 differences (7, 8) came from comparing two
readers, not from a fuzzer: nothing crashed in any of them.

**Zig fuzzing on Linux.** Zig 0.15's own `--fuzz` runs forever behind a web page. Here each harness
is built with `zig build-obj -ffuzz` and linked to clang's libFuzzer, which then needs one call to
register Zig's coverage counters ([`zig/sancov_init.c`](zig/sancov_init.c)). This gives libFuzzer's
timeouts, memory limit, dictionaries and crash files for Zig code. `zig/run.sh` builds and runs all
five.

## What this changes in the plan

- `fuzz-untrusted-parsers`: findings 1, 2, 7, 8, 10 and 12 are the case #376 gives for
  `parsers-fuzzed-and-compared`: two readers of one format, no crash, a different reading. Two more
  things belong in the answer: memory is bounded per input including what it unpacks to (3), and a
  reader that refuses gives a refusal, never an exception (9). Agent notes added.
- `what-a-budget-promises`: its property `budget-stops-what-it-meters` (checked by a hostile test)
  fails on #385's code for findings 1 and 2. Agent notes added with the patch that makes it hold.
- `program-that-cannot-be-read`: it says nothing goes unread to a host Silo reads. Findings 1 and 2
  are traffic Silo reads and reads wrongly. Agent notes added.

## What was not verified

- The real hosts were asked through this container's HTTPS proxy, which ends TLS and sends the
  request again. A Go server behind it redirected `/./anything`, so the proxy passed that path as
  written; whether it rewrites absolute-form targets, bare LFs or folded lines is not known. The
  check to make on a Mac, with no proxy, is `boundary/live.sh`. No credential was sent, so no call
  was billed: a 401 shows the route, not a billed call.
- The smuggled call (2) was not sent to a real host. It was sent to a Go net/http host, which reads
  bare LFs and folded lines the way the real hosts answered.
- The patches were not run with the agent programs #374 measured.
- The SSH agent was fuzzed with no connection seen; the bind path needs a real host signature and
  was left to #379's own attack cases.
- The ext4 comparison is with Linux. The Mac reads no ext4; option A's main is APFS, whose reader no
  draft has built yet.

## Run it

```
git clone https://github.com/amontlabs/silo && cd silo
research/2026-11-25-untrusted-parsers/fetch.sh /tmp/src             # the drafts' code, pinned
research/2026-11-25-untrusted-parsers/zig/run.sh /tmp/src 600       # five Zig fuzzers, 10 min each
cd /tmp/src/385/research/2026-11-24-spend-budgets && run/tls.sh && (cd boundary && zig build -Doptimize=ReleaseSafe)
SB=/tmp/src/385/research/2026-11-24-spend-budgets python3 research/2026-11-25-untrusted-parsers/boundary/e2e.py paths smuggle memory
```

Linux, as root (namespaces, loop mounts). Zig 0.15.2, clang 18 with `libclang-rt-18-dev`, Go 1.24,
Python 3.13 with `atheris`, `zstandard` and `cryptography`, Node with Playwright, e2fsprogs. Each
script's first lines say what it takes.

| Path | What it is |
|---|---|
| `inventory.md` | Every parser of the drafts that reads bytes a computer or a host controls |
| `fetch.sh` | The drafts' code at the commits tested |
| `zig/` | libFuzzer harnesses for #385 and #386, and `run.sh` |
| `boundary/` | The spend boundary end to end (`e2e.py`, stand-in host `upstream.go`), and the real hosts (`live.sh`) |
| `python/` | atheris harnesses (#379, #381), the ext4 comparison with Linux, the cloud swap, the preview in Chromium, the gate, the closed network |
| `go/` | The deny-list fuzz test for #380, and an echo server the Python tests use |
| `repro/` | The WebSocket test, and the two patches |
| `results/` | What each test printed |

## The research at a glance

Fuzzing found one hang (spend boundary WebSocket) and six reader crashes (ext4); comparing each
reader with the program it sits in front of found more: six path spellings and two header
framings that carry uncounted model calls past a budget, a 64 KiB request that takes 5.4 GB, a
Host header that steers a real cloud token, a file hidden from Silo's disk reader, and a preview
that drops its CSP. Patches for the boundary pass the same tests.

## For polarzero to rule

- `parsers-fuzzed-and-compared` (#376): take it. This folder is its evidence: of the 14 findings,
  the six that matter most are different readings with no crash.
- Whether a boundary refuses what it cannot read exactly (the patch: strict requests, refused with
  a 400) or rewrites it into one form before it leaves. Refusing is simpler and showed no cost here;
  rewriting keeps a lenient program working. Recommended: refuse, and run #374's programs on the
  patch to see whether any of them is refused.
