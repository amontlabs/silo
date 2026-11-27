# TLS refusals, recorded and shown

Research of 25 November 2026, by an agent, for polarzero. #403 found that Codex without Silo's
authority says "Reconnecting... waiting for network" and never stops, that #378's proxy logs each
of its connections as allowed, and that only the side that ends TLS sees why. This folder makes
the boundary of #395 record each refused certificate with its cause and the program, and turns
those lines into one alert and one line in the morning.

Stacked on #395 by merge. The boundary's change is in
`../2026-11-25-boundary-end-to-end/boundary/` (`src/ja4.zig`, and `src/main.zig` around
`readHost`); everything else is here.

## In short

- **#395 already writes a line for a failed handshake, but the same one for ten causes.** A refused
  certificate, a client that sent nothing, plain HTTP, no common TLS version, a client stalled mid
  handshake: all `"why":"handshake-failed"`, with no program
  ([`results/before-395.jsonl`](results/before-395.jsonl)). And git's refusal, or a pinned key, was
  not even that: the handshake finished, and the close looked like a connection that worked.
- **With the change, each cause has its own `why`.** The boundary follows the handshake with
  OpenSSL's info callback: whether its certificate went out, and the alert each side sent
  ([`results/after.jsonl`](results/after.jsonl)).

  | `why` | What happened | Seen from |
  |---|---|---|
  | `certificate-refused` | the client sent a certificate alert | curl, Python, Go (`unknown_ca`, `bad_certificate`), Codex (`unknown_ca`) |
  | `closed-after-certificate` | the client hung up once it had the certificate | Node, Claude Code |
  | `no-request` | the handshake finished, then a close with no request | git (GnuTLS checks after its side of the handshake), a pinned key in curl |
  | `no-common-tls`, `no-tls-hello`, `no-hello-in-time`, `name-differs` | not a refusal | a cipher or version the boundary lacks, plain HTTP, nothing sent, another name |

- **No program that trusts Silo gave a line that reads as a refusal.** Claude Code, Codex, curl,
  Python, Go and git, each run with the authority: every connection closed with its requests.
- **The ClientHello tells the programs apart; the name comes from traffic that worked.** A program
  that refuses never sends a request, so its User-Agent is never seen. The boundary writes a
  fingerprint of the ClientHello in JA4's shape (`tls_client`) on every closed connection, and
  the User-Agent beside it when there was a request. Seven clients gave seven distinct
  fingerprints; Claude Code had two (two HTTP stacks in one program) and Codex two (with and
  without `h2`). The view names a refusing client from the User-Agent its fingerprint sent on an
  earlier connection that worked; Node, never seen working, is shown by its fingerprint.
- **One alert per program and host, and one line in the morning.** On a 5-minute night where
  Codex, Claude Code, Node and git lacked the authority (`run/night.sh`,
  [`results/morning.txt`](results/morning.txt)):

  ```
  Codex on c1 did not trust Silo's certificate for api.openai.com: 47 refusals from 18:53 to 18:58, still refusing at the end. Give it Silo's authority (SSL_CERT_FILE or the system's store).
  Claude Code on c1 did not trust Silo's certificate for api.anthropic.com: 18 refusals at 18:54, still refusing at the end. Give it Silo's authority (NODE_EXTRA_CA_CERTS).
  a program (TLS client t13d5212h1_b262b3658495_8e6e362c5eac) on c1 did not trust Silo's certificate for api.anthropic.com: 1 refusal at 18:54, still refusing at the end. If it pins the host's own key, it cannot work through a host Silo reads.
  git on c1 did not trust Silo's certificate for api.anthropic.com: 3 connections closed before any request at 18:54, still refusing at the end. Give it Silo's authority (GIT_SSL_CAINFO).
  ```

  The night was shorter than the 5 minutes after which a program counts as having stopped
  trying, so every line says "still refusing"; `run/refusals.py --test` checks the other endings.
- **A stuck program costs the record about 800 bytes a refusal.** The refusal line is 437 bytes
  and the `opened` line #395 already writes is 372. Codex refused about once every 6 s, so a whole
  night of it (8 hours) is about 4,500 refusals and 3.6 MB. The lines are kept one per connection here; if
  that is too much, the boundary can merge one client's refusals on one host per minute.

## How the view reads the record

[`run/refusals.py`](run/refusals.py) reads the record's lines (`silorec print`) and writes nothing
back, as a rule would.

- A refusal is `certificate-refused` or `closed-after-certificate`. `no-request` counts only from
  its third, for a client that made no request to that host meanwhile: one program that connects
  and gives up must not raise an alert.
- Refusals of one program on one host of one computer are one condition. It starts at the first
  refusal, which raises the alert, and ends when that program makes a request to that host, or
  after 30 minutes with none. A refusal after that is a new alert, as `alert-only-on-a-change`
  asks.
- The morning says how many, from when to when, and how it ended: still refusing at the end (the
  last refusal within 5 minutes of it), it worked again at a time, or it stopped trying. A program
  still refusing gets the setting that gives it Silo's authority, from #403's table.

## What was built

| File | What it is |
|---|---|
| `../2026-11-25-boundary-end-to-end/boundary/src/ja4.zig` | The ClientHello's fingerprint, in JA4's shape, with unit tests |
| `../2026-11-25-boundary-end-to-end/boundary/src/main.zig` | The info callback, `whyFailed`, the fields on the `closed` lines, the User-Agent of the first request |
| [`run/up.sh`](run/up.sh) | #395's set-up with computer c1 only (its `CONNECT` way in); both ways in end TLS in the same function |
| [`run/cases.py`](run/cases.py) | 21 cases, each run in c1, printing the record lines the boundary wrote for it |
| [`run/clients/goget.go`](run/clients/goget.go) | A Go client |
| [`run/night.sh`](run/night.sh) | The 5-minute night: programs that once worked, then the same without the authority, then the view |
| [`run/refusals.py`](run/refusals.py) | The view: alerts and the morning, with its own checks (`--test`) |
| [`results/`](results/) | Each run's output, and the night's record ([`night-record.jsonl`](results/night-record.jsonl), whose chain `silorec verify` holds) |

## What it changes in the plan

Notes were added as "An agent's part, 25 November 2026". No verdict, answer or list was changed.

| Entry | What the note says |
|---|---|
| `program-that-cannot-be-read` | The three ways a refusal looks, which clients do which, no false alarm, the cost in the record |
| `alert-only-on-a-change` | A refused certificate as a condition that lasts: when it starts and ends, the alert and the morning line |
| `ending-the-encrypted-connection` | Each host Silo reads is where a program can refuse; the ClientHello tells programs apart, traffic that worked names them |

Nothing here needs a ruling: what to show follows `alert-only-on-a-change` and
`program-that-cannot-be-read` as they stand.

## Not verified

- Nothing ran on a Mac or with the hand-off way in. Both ways reach the same `readHost`, so the
  lines should be the same; not run.
- JA4's shape was followed from its description; the fingerprints were not checked against
  FoxIO's own tool. Only their distinctness matters here.
- How stable a fingerprint is across a program's versions was not measured: one version each.
  A new version with a new TLS stack is unnamed until it works once.
- Seven clients, each a few times. A trusting program that connects and gives up after the
  handshake would look like `no-request`; none did here, and three are needed for an alert.
- The guest clock behind Silo's certificate (#397) would be refused as `certificate_expired` by
  the clients that say so; not run here.
- The morning's computer is the record's id (`c1`), not its name (`main`).
- The fake keys of #395's `run/config.py` are the only keys used; no real provider was reached.

## Running it

Linux, as root, with what #395's README lists, Codex 0.172.0 from npm (`CODEX=` its path) and
Go 1.24:

```
../2026-11-25-boundary-end-to-end/run/tls.sh
(cd ../2026-11-25-boundary-end-to-end/boundary && zig build -Doptimize=ReleaseSafe && zig build test)
(cd ../2026-11-25-boundary-end-to-end/recorder && zig build)
(cd ../2026-11-25-record-format/proto && zig build)
(cd run/clients && go build -o /root/e2e/goget .)
run/up.sh /tmp/tr/after && python3 run/cases.py /tmp/tr/after; ../2026-11-25-boundary-end-to-end/run/down.sh /tmp/tr/after
run/night.sh /tmp/tr/night 300
python3 run/refusals.py --test
```

`results/before-395.jsonl` is the same cases on #395's boundary as it was (`run/up.sh <out> <its binary>`).

The research at a glance: #395's boundary now tells a refused certificate from nine other failed
handshakes and names the program by its ClientHello, learned from traffic that worked; seven real
clients refuse in three different ways, none of the trusting ones raised a false alarm, and a
stuck Codex (47 refusals in 5 minutes) becomes one alert and one morning line with the setting
that fixes it.
