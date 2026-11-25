# One record format, its writer and `silo verify`

Research of 25 November 2026, by agents, for polarzero. Four drafts described the record's files
differently: #370's `formats.md`, #371's morning review prototype, #362's hash chain and person,
and #351's findings on keys and size. This folder makes them one format,
[`format.md`](format.md), and a prototype of its writer and of `silo verify` in Zig
([`proto/`](proto/)), with a crash test, a tamper test and a fuzzer.

Nothing here is built and no entry is taken by it. What it changes in the plan is written as agent
notes in the entries listed below, and as one new open decision.

## What was found

1. **The writer survives a crash at every step.** The crash test kills the writer at each of its
   209 steps on the disk in turn: before and in the middle of each write, before each flush, before
   each rename. After every one, the recorder started again, every line acknowledged before the
   crash was there, the chain, head and blobs verified, and once the sources sent their lines again
   each line was there exactly once. Three bugs planted in the writer were each caught:

   | Planted bug | Crash points where the test failed | First failure |
   |---|---|---|
   | A line written before the blob it points to | 26 of 209 | a blob a line points to is missing, and no fact says it was removed |
   | The head rewritten in place instead of renamed | 6 of 203 | the head cannot be read |
   | A line sent again written a second time | 196 of 209 | a line is missing or written twice |
   | Any bad frame cut at start, not only an unfinished last one | the tamper case below | the recorder cut the record |

   This tests a process killed at any point, not a power cut, as #365's catalog test did. What a
   power cut keeps depends on `F_FULLFSYNC`, which is a Mac test.

2. **`silo verify` finds every change to the record, except the two the format cannot see.**
   [`proto/results/tamper.md`](proto/results/tamper.md):

   | Change | Found | What verify says |
   |---|---|---|
   | One byte of a line changed | yes | a line does not match its CRC |
   | A line changed and its CRC written again | yes | the line before it was changed, removed or moved |
   | The last line changed and its CRC written again | yes | the head's line was changed |
   | A line removed | yes | the line is not the next line of the chain |
   | Two lines swapped | yes | the line is not the next line of the chain |
   | A forged line inserted, with a valid CRC | yes | the line is not the next line of the chain |
   | The last lines cut at a line's end | yes | the record ends before the head's line |
   | The last lines cut at a line's end, no head | **no** | every chain holds |
   | The last segment deleted | yes | the record ends before the head's line |
   | A segment in the middle deleted | yes | segments are missing or out of order |
   | A header byte changed | yes | the segment's header does not match its checksum |
   | A segment of another computer put in its place | yes | the segment belongs to another computer |
   | Garbage appended to the last segment | yes | the last line is unfinished, or it was changed |
   | A line changed and every line after it chained again, old head kept elsewhere | yes | the head's line was changed |
   | A line changed and every line after it chained again, head rewritten too | **no** | every chain holds |
   | A blob's bytes changed | yes | a blob's bytes do not match its hash |
   | A blob deleted, no fact says so | yes | a blob a line points to is missing |
   | A blob deleted by the size limit, a fact names it | no, as it should | every chain holds |
   | A line changed in the middle of the last segment, then the recorder starts | yes | the recorder refuses to start: `record_damaged` |

   The second "no" matters for `record-edits-show` (#362): a head kept next to the record shows
   accidents, not someone with your user's rights. Only a copy of the head kept elsewhere shows a
   rewrite. Agents inside a computer cannot reach the record, which is on the Mac; the case is
   another program running as you on the Mac.

3. **The fuzzer found one crash in the reader and nothing it missed.** It changes a real record at
   random (bits, bytes, cuts, inserted and repeated ranges, segments dropped or swapped, lengths,
   and payloads changed with a CRC that matches so they reach the JSON reader) and asks verify each
   time. Over 400,000 runs before the final one, verify never said a changed record held. It found
   one crash: the frame walk overflowed on a segment shorter than its header. That is fixed, and
   the final run of 300,000 is in [`proto/results/fuzz.txt`](proto/results/fuzz.txt). Verify parses
   each line in a fixed 2 MiB, whatever it reads (`fuzz-untrusted-parsers`).

4. **Storing each request as the full list of its messages' hashes still grows with the square of
   a conversation.** `store-messages-once` says a request is then "a short list of hashes". In a
   conversation of 150 turns, the last request lists 300 hashes of 71 bytes. On the heavy night
   below, model-call lines take 49.6 MB with full lists and 5.5 MB when each request names the one
   it extends and lists only what it added. Claude Code rewrites earlier messages now and then
   (#371 finding 1); the model assumes 5% of requests, which then carry the full list. The blobs
   themselves are 13.1 MB, against 935 MB if each request's messages were kept whole.

5. **Size per night.** Lines only, with `sent` as the messages added:

   | Night | Lines | Segments | gzip -6 | zstd -3 | xz -6 | Blobs |
   |---|---|---|---|---|---|---|
   | #371's night, real Claude Code and Codex, 4 runs, scripted model, converted to this format | 3,570 | 1.55 MB | 316 KB | 268 KB | | 90 KB |
   | Modelled: #370's morning, 3 agents for 8 hours, a turn every 20 s, 7 processes a turn, 7,412 file facts, 96 checkpoints | 94,761 | 34.9 MB | 8.7 MB | 7.0 MB | 5.6 MB | 13.1 MB |
   | Modelled: one agent, one hour | 1,682 | 0.66 MB | 169 KB | 131 KB | 108 KB | 0.38 MB |

   A line takes about 330 bytes for a process or a file, 1.3 KB for a model call. `prev` takes 75
   bytes of every line, about a fifth of the raw size, and compresses to about half. The modelled
   nights use made-up text, so their compression ratios are a guess; #371's real lines compress 5.8
   to 1 with zstd.

6. **Redaction is cheap with a table of starting byte pairs.** Every string of every line and blob
   passes through the redactor before it is hashed. On #371's lines, it runs at 286 MB/s with no
   secret, 225 MB/s with 50 secrets and 142 MB/s with 200 (`proto/results/redact-cost.jsonl`). A
   plain scan for each secret ran at 16 MB/s with 50. With 1,000 secrets it falls to 6 MB/s,
   because their starting pairs cover most pairs; nobody has 1,000 secrets in one computer.

## What it changes in the plan

| Entry | State | What this adds |
|---|---|---|
| `the-chain` | taken | The format keeps a tool call's command text in its model-call line, which #371's challenge (match by command text) needs |
| `what-a-fact-carries` | taken by agents | The envelope spelled out: `seq`, `prev`, time, computer, source with count and number. A correction is a new line with `corrects` |
| `store-messages-once` | taken | A challenge: "a short list of hashes" is not short. The request names the one it extends and lists what it added (finding 4) |
| `who-made-each-change` | open | `device` as a sixth author, as #370 proposes; the verifier checks the list |
| `record-file-format` | taken by agents | Compression measured on #371's lines and on a modelled night; closed segments compress about 5 to 1 |
| `kinds-of-lines` | open | #371's note on two writers: one writer, and put-back's fact goes through the engine |
| `record-damaged-at-start` | **new, open** | What the recorder does at start when a line it did not just write is damaged |
| `record-edits-show` (#362) | open in #362 | Not on main, so the note is here: a head next to the record shows accidents only; a copy elsewhere shows a rewrite |
| `secret-never-written-down` | in force | The writer redacts every string before it hashes it; what it does not cover is in `format.md` |

## For polarzero to rule

- **`store-messages-once`**: change "a request is then a short list of hashes" to "a request names
  the request it extends and lists the messages it added". The case is finding 4.
- **`record-damaged-at-start`**, new: recommended, keep recording in a new segment and write a fact
  naming the damage, rather than stop every computer until someone looks.
- **`who-made-each-change`**: add `device` to the authors, for the Mac going to sleep.
- **Where a copy of the head goes** (`record-edits-show`, #362): without one, someone running as
  you on the Mac can rewrite the record without a trace.

## What was not verified

- A power cut. The crash test kills a process; what reaches the disk on a power cut is a Mac test
  with `F_FULLFSYNC`.
- The Mac. Everything ran on Linux, on ext4 in a container. The write times here say nothing of a
  Mac's.
- A real night's size. #371's night is real programs against a scripted model and takes 90
  seconds; the heavy night is a model whose counts are stated above.
- The rate at which Claude Code rewrites earlier messages; 5% is an assumption.
- In the format, not in the prototype: the opinions chain (same code, `is: opinion`), the index,
  the 1 KiB cut of long tool inputs, secrets taken back while the recorder runs, and reading a
  segment in pieces of fixed size instead of whole.

## Run it

Zig 0.15.2, as #365 used.

```sh
cd proto
zig build test                                  # unit, tamper and a fuzz run of 3,000
zig build --release=safe
./zig-out/bin/silorec tamper /tmp/t             # the tamper table
./zig-out/bin/silorec crash /tmp/c              # the crash test; add a planted bug's name to try it
./zig-out/bin/silorec fuzz /tmp/f 300000 44
./zig-out/bin/silorec night /tmp/n heavy delta  # or: light, full
python3 scripts/from371.py <night>/record delta > lines.jsonl
./zig-out/bin/silorec import /tmp/i lines.jsonl && ./zig-out/bin/silorec verify /tmp/i c1
```

| File | Holds |
|---|---|
| `proto/src/format.zig` | The segment header, frames and their walk |
| `proto/src/line.zig` | The keys of a line, every string through the redactor |
| `proto/src/redact.zig` | The redactor |
| `proto/src/writer.zig` | The writer: append, flush, segments, blobs, head, start |
| `proto/src/verify.zig` | `silo verify` |
| `proto/src/crash.zig`, `crashtest.zig` | The crash points and the crash test |
| `proto/src/tamper.zig`, `fuzz.zig` | The tamper cases and the fuzzer |
| `proto/src/night.zig` | The modelled nights |
| `proto/scripts/from371.py` | #371's record converted to this format |
| `proto/results/` | The outputs quoted above |
