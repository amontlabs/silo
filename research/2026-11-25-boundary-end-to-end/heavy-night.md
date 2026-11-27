# The boundary on a heavy night

Research of 25 and 26 November 2026, by an agent, for polarzero. The boundary of this folder
(#395) now also takes #385's counting, #393's "key or plan" setting, #383's holds of destructive
calls, #394's writer and #401's two fixes. This page is what that whole boundary did under a
heavy night: many agents and subagents on two computers, streamed model calls, tool traffic, holds,
hostile requests, and the recorder stopped in the middle.

Nothing here is a decision and nothing is built for Silo.

## In short

- **Everything still composes.** #395's 39 cases pass on the combined boundary
  ([`results/cases-combined.txt`](results/cases-combined.txt)). On every night below, the
  boundary counted every model call and every token the stand-in billed: 4,136 calls at the
  largest size, input, cache reads, cache writes and output equal to the token.
- **Load costs memory and threads, not latency.** From 16 to 256 agents and subagents at once
  (92 operations a second), the first byte of a streamed reply stayed at 57 to 60 ms (p50) and 108
  to 114 ms (p99), and a cloud request at 1.8 to 2.1 ms (p50). The boundary used 16% of one core
  at the largest size. Its memory grew with the connections open: 37 MB at 16 loops, 375 MB at
  256, about 1.4 MB and 5.8 threads per loop, because the boundary holds a thread for each
  connection, kept-alive ones included, and closes none for being idle.
- **A heavy night fills the record's room for decisions in about two minutes.** At 128 loops
  (64 a computer) a computer sends about 32 decisions a second. With the default room of 4,096,
  a stopped recorder was noticed by nobody for 127 s; after that every new decision waited. What
  happened next is the open choice of `enforcing-and-recording`, measured three ways below.
- **The precaution costs a tenth of a millisecond, and a hold costs a thread.** #383's rules in
  #353's detector, run as their own process as `synchronous-precaution-path` places a classifier,
  decided in 0.1 ms (p50) and 0.5 ms (p99) for every request to a host Silo reads other than a
  model's, 6,188 times in two minutes at the largest size, using 2.7% of a core; the round trip
  is inside the cloud request's 1.9 ms. 205 destructive calls left unanswered
  were all refused at Silo's 90 s limit (90.0 to 92.1 s), with the sentence that says nobody said
  no; the boundary's threads did not grow beyond the same night without holds.
- **#401's requests are refused on the combined boundary, and slip through #395 as drafted.**
  Through #395 as drafted, a model call spelt `//v1/messages` reached the provider and was not
  counted, and a 64 KiB zstd body took the boundary to 5,049 MB. On the combined boundary, every
  spelling and framing got a 400 with Silo's sentence and a line in the record, and the boundary
  peaked at 134 MB.
- **Two refusals leave no sentence and no line.** A request whose body unpacks past 64 MiB, and
  a head with an empty folded `content-length`, close the connection with nothing sent back and
  nothing recorded. Both should refuse in the program's terms and record the refusal, as the other
  refusals do.

## What was folded in

| From | What | Where |
|---|---|---|
| #385 | Work out of sight (batches, background responses, caches), chained Responses calls, a cut stream, cache lifetimes, forks of a conversation, searches billed per call | #385's modules, copied at build time as #374's were, and its changes to #374's `main.zig` carried into this folder's `main.zig` |
| #393 | "Claude with an API key or a plan", per computer. "plan": api.anthropic.com is passed through unread. "key": a plan's sign-in is refused with #393's sentence | `boundary/src/keyplan.zig`, the computer's `anthropic` setting in `conf.zig`. #393 read the host from the ClientHello; here the way in already names it |
| #383, #353 | Destructive calls held for a person, before any secret is swapped in | `boundary/src/hold.zig` (the boundary's side and both limits), `precaution/precaution.py` (the detector, unchanged, and a stand-in person). The detector's folders are taken from #383's branch unchanged |
| #394 | The writer that asks for `F_FULLFSYNC` on a Mac | #386's writer, as the recorder already copies it |
| #401 | Strict request heads, a 64 MiB limit on a body packed or unpacked, a WebSocket message ended at its final DEFLATE block | #401's two patches, applied to this folder's `main.zig` and to #374's `ws.zig` |

### How a hold goes

1. A request to a host Silo reads, other than a model's, is sent to the precaution before its
   placeholders are swapped, so the precaution never sees a secret's value.
2. The precaution answers with the detector's opinion within `classify_limit_ms` (2 s). No answer
   in time: refused, or left to the rules, as `on_no_opinion` says (`classifier-wait-limit`).
3. "hold": the boundary records the hold with the detector's sentence, then waits for the person
   for `hold_limit_ms` (90 s, `hold-limit-length`). The boundary keeps both timers; nothing the
   precaution does lengthens them.
4. A yes: the request leaves, and the record says who allowed it. A no, or no answer: 403, which
   programs do not retry by themselves (`how-a-hold-ends`), with a sentence that says which
   (`hold-sentence`).

A line of the record from a hold, as `silorec print` shows it:

```json
{"v":1,"seq":266,"prev":"…","time":"2026-11-25T14:10:24.063259903Z","computer":"c1","source":{"id":"boundary:c1","count":"k:346c8a8e","n":266},"is":"fact","kind":"connection","what":"held","decision":"held","decided_by":"precaution","precaution":"destructive-calls","host":"api.github.com","line":"DELETE /repos/polar/scratch-3-5 HTTP/1.1","verdict":"destructive","says":["deletes the GitHub repository polar/scratch-3-5"],"classify_ms":0.64}
```

## The night

[`run/night.py`](run/night.py), run by [`run/night.sh`](run/night.sh) in both computers at once.
Each loop is an agent or one of its three subagents. A turn is a streamed model call (Anthropic
with a cached system prompt and tools for two loops in three, OpenAI's Responses for the third),
its conversation growing, then one to three tool calls after a pause of 0.2 to 1.5 s:

| Share | Tool call | What it exercises |
|---|---|---|
| 45% | Cloud Storage with the computer's placeholder | a host read, a secret swapped |
| 20% | A read of GitHub's API | the precaution, a secret swapped |
| 15% | A write to Vercel's API that is not destructive | the precaution, a body read |
| 20% | A fetch from github.com | a host passed through unread |
| 1% | `DELETE` of a GitHub repository | a hold; the stand-in person says yes after 2 s |
| 2% | One of #401's requests | a refusal |

The stand-in model thinks 200 to 1,500 tokens and writes at 150 tokens a second, so a call takes
1.5 to 10 s, and a loop takes a turn about every 6 s, three times as often as #370's morning (a
turn every 20 s). The 4-core container ran the load, the stand-ins (Python) and the boundary
together.

### Size

[`results/night/scale.jsonl`](results/night/scale.jsonl), 120 s each, loops per computer × 2.

| Loops | Ops/s | Model calls | First byte p50 / p99, ms | Cloud p50 / p99, ms | Vercel write p50 / p99, ms | Boundary peak MB | Threads | CPU % of a core | Record KB/s |
|---:|---:|---:|---|---|---|---:|---:|---:|---:|
| 16 | 6.0 | 259 | 56.8 / 108.1 | 2.1 / 12.8 | 9.8 / 61.1 | 37 | 103 | 1.3 | 6.2 |
| 64 | 23.2 | 1,048 | 57.5 / 108.5 | 1.9 / 16.2 | 11.7 / 53.8 | 107 | 382 | 4.5 | 24.2 |
| 128 | 46.6 | 2,067 | 57.8 / 109.1 | 1.8 / 16.6 | 11.6 / 52.2 | 195 | 751 | 8.7 | 48.5 |
| 256 | 92.6 | 4,136 | 59.6 / 113.5 | 1.9 / 20.4 | 12.7 / 50.4 | 375 | 1,483 | 16.2 | 96.3 |

- The recorder stayed at 4.9 MB and 6.5% of a core at 240 lines a second. `silo verify` held on
  both computers' records of the largest night (12,963 and 18,236 lines).
- The record grows by about 400 bytes a line and 2.6 lines an operation: a connection opened and
  closed, each secret swapped, each model call admitted and counted. At 128 loops, eight hours at
  this pace would be 1.4 GB before compression; #386 measured about 5 to 1 on closed segments.
  #386's modelled morning (3 agents, a turn every 20 s) was 35 MB. Most of the difference is one
  `secret-used` line for every request on a kept-alive connection; one line per connection and
  secret would cut it.
- Memory and threads track open connections, not requests. A loop holds up to four kept-alive
  connections, and a host passed through holds two threads. An idle limit on kept-alive
  connections, or an event loop in place of a thread per connection, is what would bound it.

### The recorder stopped

The recorder was stopped (`SIGSTOP`) 30 s into a night of 128 loops, with the default room of
4,096 decisions. [`results/night/stall.jsonl`](results/night/stall.jsonl).

For 127 s nothing changed: decisions went into the room. Then the room was full, and each new
decision waited. Three limits on that wait (`decision_wait_ms`) were run, with a stall of 150 s
(23 s past full) and of 200 s (73 s past full):

| Wait for room | Stall | Refused after room was full | Operations started while full | Their wait, p50 / max | Model calls served = counted |
|---|---|---:|---:|---|---|
| 3 s | 150 s | 491 (each with Silo's sentence) | 706 | 3.0 s / 12.6 s | 3,794 = 3,794 |
| 30 s | 150 s | 0 | 209 | 16.4 s / 30.6 s | 3,794 = 3,794 |
| none (600 s) | 150 s | 0 | 208 | 16.7 s / 31.3 s | 3,805 = 3,805 |
| 30 s | 200 s | 226 | 388 | 30.0 s / 60.0 s | 3,290 = 3,290 |
| none (600 s) | 200 s | 0 | 209 | 65.4 s / 81.0 s | 3,275 = 3,275 |

- With 3 s, the agents kept working: each refusal was a sentence the program could act on, and
  three times as many operations were tried. With no limit, every agent sat waiting with nothing
  to tell it why; at 73 s past full, half the waits were over a minute. An agent program that
  gives up sooner (Claude Code retried for about three minutes in #378's test) would see its own
  timeout instead of Silo's sentence.
- When the recorder came back, every waiting line was written once, the chain held, and nothing
  was dropped.
- A refusal on c2 at the TLS step closes the connection (an `EOF` to the program): the hand-off
  has no way to carry a sentence before TLS. On c1 the refusal is a 403 at the `CONNECT`.
- The room's size sets how long a stall goes unnoticed: 4,096 decisions is about two minutes at
  this pace. It is a separate setting from the wait, and only the wait changes what programs see.

### Holds left unanswered

[`results/night/holds-unanswered.jsonl`](results/night/holds-unanswered.jsonl): 128 loops for
240 s, 5% of tool calls destructive, nobody answering. All 205 were refused at 90.0 to 92.1 s with
"It was held for a person and not answered in 90 s, so it did not leave. Nobody said no: it can
be tried again later." The boundary peaked at 182 MB and 726 threads, against 195 MB and 751
threads for the same size without holds: a held request costs its connection's thread for 90 s,
and nothing more. Every model call was still counted.

### #401's requests through the whole boundary

[`run/hostile.sh`](run/hostile.sh) sends #401's cases from inside c1, through the relay and the
boundary, once with #395 as drafted and once with the combined boundary.

| | #395 as drafted | Combined |
|---|---|---|
| Six spellings of `/v1/messages`, two of `/v1/responses` | `//v1/messages` served by the provider and not counted; the stand-in answered the others 404 or counted them | all refused, 400, with a line in the record |
| Bare LF before `transfer-encoding`, a folded `content-length`, a space before the colon, `1_0` as a length | passed to the host | all refused, 400, with a line in the record |
| 64 KiB of zstd unpacking to 2 GiB | boundary at 5,049 MB | boundary at 134 MB; the connection closed with nothing sent back and no line in the record |
| Host header naming another host than TLS | the real key went to the connection's host | the same: the swap and the dial follow the name the connection was opened for, not the Host header |

([`results/night/hostile-395-as-drafted.txt`](results/night/hostile-395-as-drafted.txt),
[`results/night/hostile-combined.txt`](results/night/hostile-combined.txt).) The stand-in hosts
are Python's HTTP server, which does not read a chunked body, so #401's smuggled second request
could not be served here; #401 showed it against a stand-in in Go.

## Not built, or not measured

- Real agent programs on this night. The loops are a script, so that a night of 256 runs
  unattended; #395 ran Claude Code through the same boundary, and #374 and #385 ran it with
  subagents.
- A Mac. Everything ran on one Linux container, so the timings include the load and the Python
  stand-ins on the same four cores.
- The precaution's other parts: SQL, MCP, git pushes and AWS go through the same detector call,
  but the night sends none of them.
- What a person sees when a hold asks; here a stand-in answers after a fixed delay.
- An idle limit on kept-alive connections, which the memory figures call for.

## How to run it

After [the setup in README](README.md#running-it), and `pip install brotli zstandard graphql-core
sqlglot` for the detector:

```
run/night.sh /tmp/e2e/n 64 120                         # 128 loops for 120 s
python3 run/summary.py /tmp/e2e/n                      # one JSON object
STALL_AT=30 STALL_FOR=150 run/night.sh /tmp/e2e/s 64 240 '{"decision_wait_ms": 3000}'
ANSWER=none DESTRUCTIVE=0.05 run/night.sh /tmp/e2e/h 64 240
run/hostile.sh /tmp/e2e/x                              # BOUNDARY_BIN=<#395's build> for the other column
```
