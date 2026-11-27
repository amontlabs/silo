# The record's format

Research of 25 November 2026, by agents. One format for the record, made from four drafts that
differed: the record section of #370's `formats.md`, the files of #371's morning review prototype,
#362's hash chain and person, and what #351 found on size and keys. The prototype in
[`proto/`](proto/) writes and verifies exactly this format. Where this page and #370's
`formats.md` differ, this page is the newer of the two.

## Where the drafts differed, and what this format keeps

| Question | #370 `formats.md` | #371 prototype | Kept | Why |
|---|---|---|---|---|
| Chains per computer | One for facts and reports, one for opinions | Three files: facts, reports, opinions | #370 | One order across facts and reports is what the recorder sees, and a report points to a fact by its number in the same chain. A reader that wants only facts filters on `is` |
| Writers | The recorder alone | The engine and `silo putback`, each taking a lock | #370 | #371's chain broke with two writers (its finding 5). With one writer the break cannot happen. A put-back is an engine operation, so its control fact reaches the recorder as an engine event, like every other |
| Line number | `seq`, given by the recorder | `id` like `main:f79`, counted per file | `seq` | One number per chain. Another chain refers to a line as `c1:1042` |
| Time | UTC, nanoseconds | Local time with its offset, milliseconds | UTC, nanoseconds | The views show local time; the record has one form |
| File | Binary frames: length, CRC-32C, line | One JSON line per text line | Frames | The CRC tells a write cut short from a changed line, which the writer needs at start (below). `silo record --json` still prints the lines byte for byte |
| Who did it | `author` with `by` and `person` | `by` as a string such as `rule:x@1` | `author` object | `every-control-fact-has-its-author` asks it of every control fact, and the verifier checks it |
| Report flag | `is: report` | `kind: report` and `unverified: true` | `is: report` | `unverified` says again what `is` says |
| Bulky content | Not specified | `blobs/<sha256>`, one folder for every computer | #371, in sub-folders | A fork's conversation sends the same messages as main's, so one store for every computer stores them once |
| What a request sent | Not specified | The full list of message hashes on every model call | The messages added to the request it extends | Measured below: the full list grows with the square of a conversation, as raw traffic did |

## Files

```
<data>/record/
  blobs/
    8b/8b454ed1…5fcd           bulky content, by the SHA-256 of its bytes
  c1/
    000001.silorec             segments of facts and reports, append only
    000001.idx                 its index, rebuilt when missing, never trusted over the segment
    000002.silorec
    000002.cut-4096            bytes the recorder cut at start, kept (below)
    head                       the last line's number and hash
    opinions/
      000001.silorec  head     opinions, in a chain of their own
  engine/                      what concerns no computer, as #370 says
```

## A segment

#370's layout, unchanged. A 128-byte header, then frames. A segment is closed at 64 MiB or at local
midnight and never written again.

| Offset | Field | Type | Meaning |
|---|---|---|---|
| 0 | `magic` | `[8]u8` | `SILOREC` and a zero byte |
| 8 | `format` | `u16` | 1 |
| 10 | `compression` | `u8` | 0, none |
| 11 | reserved | `[5]u8` | zero |
| 16 | `computer` | `u64` | The computer's id: 1 for `c1`, 0 for `engine` |
| 24 | `segment` | `u32` | One more than the segment before |
| 28 | reserved | `[4]u8` | zero |
| 32 | `first_seq` | `u64` | The `seq` of its first line |
| 40 | `prev` | `[32]u8` | The hash of the last line of the segment before; zeros for the first |
| 72 | reserved | `[24]u8` | zero |
| 96 | `checksum` | `[32]u8` | SHA-256 of the first 96 bytes |

A frame is a `u32` length, a `u32` CRC-32C of the payload, and the payload: one line, UTF-8 JSON on
one line, at most 256 KiB. Anything larger goes to a blob.

## A line

```json
{"v":1,"seq":1042,"prev":"9f2c…e1","time":"2026-11-25T02:41:07.123456789Z","computer":"c1","source":{"id":"engine","count":"h:5b1e44c0","n":311},"is":"fact","kind":"control","author":{"by":"you","person":"p1"},"what":"secret-taken-back","secret":"github"}
```

The keys come in this order, and the recorder writes nothing else before the kind's own keys:

| Key | Meaning |
|---|---|
| `v` | 1 |
| `seq` | The line's number in this chain, one more than the line before |
| `prev` | SHA-256 of the exact bytes of the line before, in hex; 64 zeros for the first line |
| `time` | When the source saw it, UTC, in nanoseconds |
| `computer` | `c1`, or `engine` |
| `source` | `id`, and for a source that sends its lines: `count` and `n` (`sources-number-their-lines`). The recorder's own lines have only `"id":"recorder"`: nothing stands between it and the disk to lose one |
| `is` | `fact` or `report` in the record, `opinion` in the opinions |
| `kind` | One of the nine of `kinds-of-lines` |
| `author` | Control facts only. `by` is `you`, `rule`, `precaution`, `extension`, `silo` or `device`. `person` is set when `by` is `you` (#362). A rule, precaution or extension also gives its id and version in `rule` |

Then the kind's keys. These are the ones the prototype writes; the step specs of #370 add the rest.

| Kind | Keys | Example |
|---|---|---|
| control | `what` and its own keys | `"what":"checkpoint-taken","checkpoint":412,"reason":"interval","made_ms":1830` |
| process (report) | `event` (`started`, `exited`), `pid`, `parent`, `uid`, `command` | `"event":"started","pid":8120,"parent":8001,"uid":501,"command":"/usr/bin/git status --porcelain"` |
| file (fact) | #370's file fact: `between`, `change`, `type`, `path`, `object`, `size`, `what` | `"between":[412,413],"change":"deleted","type":"dir","path":"/Users/you/work/shared-fixtures",…` |
| file (report) | `op`, `pid`, `path`, and `about`, the `seq` of the file fact it explains once matched | `"op":"wrote","pid":8120,"path":"/Users/you/work/app/src/auth.ts"` |
| connection | host, port, decision, `decided_by`, method, path, status, bytes | `"host":"registry.npmjs.org","port":443,"decision":"allowed",…` |
| model-call | provider, login, models, `tokens`, `cost_micro_usd`, `conversation`, `turn`, `sent`, `reply`, `tool_calls` | below |
| gap | `of` (the source), `count`, `missing` (first and last number) | `"of":"helper:c1","count":"k:1","missing":[41,44]` |

A line never changes. A correction is a new line with `corrects`, the `seq` of the line it
corrects (`what-a-fact-carries`).

### A model call

```json
…,"kind":"model-call","provider":"anthropic","login":"subscription","model_asked":"claude-opus-5-5","model":"claude-opus-5-5","tokens":{"in":91200,"out":310,"cache_read":88400,"cache_write":1100},"cost_micro_usd":41200,"conversation":"cv-2","turn":57,"sent":{"after":20113,"added":["sha256:6b86…","sha256:d4e7…"]},"reply":"sha256:8b45…","tool_calls":[{"id":"toolu_bef8…","name":"Bash","command":"rm -rf ~/work/shared-fixtures && echo removed"}]}
```

- `sent` names the model-call line whose request this one extends, by its `seq`, and lists the hash
  of each message added since. When the program changed an earlier message, as Claude Code does
  (#371 finding 1), `sent` is the full list of hashes instead.
- A tool call keeps its command text in the line, up to 1 KiB; a longer input goes to a blob, and
  the line keeps its first KiB. Matching a tool call to the processes it started reads that text
  (#371's challenge to `the-chain`), and a rule on a command reads it as a fact.

## Bulky content

`blobs/<first two hex digits>/<hex>`, the SHA-256 of the bytes. The bytes are redacted first, so
the hash is of what is on disk. A blob is written to a temporary name, flushed, renamed and its
folder flushed, before the line that points to it is written. A line points to a blob with
`sha256:<hex>` anywhere in its values.

When the size limit removes bulky content (`record-size-limit`), a control fact names each blob it
removed:

```json
…,"kind":"control","author":{"by":"silo"},"what":"bulky-removed","why":"size-limit","blobs":["sha256:8b45…"]
```

Removing a blob changes no line, so the chain still holds.

## Secret values

`secret-never-written-down` is kept by the writer itself: every string of every line and every
blob goes through a redactor before it is hashed, and each real value is replaced by
`[secret:<name>]`. The writer receives the values from the engine, holds them in memory only, and
drops a value when the secret is taken back. A value shorter than 8 bytes is refused, since it
would cut ordinary words out of the record.

What it covers: the value as it is, wherever it appears in what Silo writes down, including a
command line, a URL or a tool's output. What it does not cover: a value encoded (base64, URL
encoding) or split across two lines. `secret-values` already says that a host sending the value
back in another form is not covered.

## The writer

The recorder is the only writer of the record.

1. **Append.** Lines are added to a batch. The batch is written with one `pwrite` and one flush
   (`fdatasync` on Linux, `F_FULLFSYNC` on a Mac). Only then are its lines acknowledged to their
   sources.
2. **Sent twice.** The recorder keeps each source's last count and number. A line under the same
   count with a number it already holds is not written again. A number past the next one is
   written as a gap first.
3. **Segments.** A new segment is written to `NNNNNN.silorec.tmp` with its header, flushed and
   renamed.
4. **Head.** `head` holds `{"v":1,"computer":"c1","seq":…,"hash":"…","time":"…"}`. It is written to
   `head.tmp`, flushed and renamed, every hour and when the recorder stops.
5. **Start.** The recorder removes every `.tmp` file, reads the last segment and finds its last
   line. A last frame that is cut short, or whose CRC fails and which ends exactly at the end of
   the file, is a write the recorder did not finish: its source never had it acknowledged and sends
   it again. The recorder copies those bytes to `NNNNNN.cut-<offset>`, cuts them off, and writes a
   control fact `record-tail-cut` with the segment, offset and size. Any other bad frame is damage,
   not an unfinished write, and the recorder does not cut it. What it does then is the open
   decision `record-damaged-at-start`.

## `silo verify`

`silo verify` reads every segment of a chain, its head and its blobs, and stops at the first
problem, naming the segment, the byte and the line. It checks:

- each segment's header, its checksum, its computer, its number, its first line and its `prev`;
- each frame's length and CRC;
- each line's envelope byte for byte (`v`, `seq`, `prev`), that it is one JSON object of valid
  UTF-8 with no key twice, its computer, its time, its source, `is`, `kind`, and for a control fact
  its author and person;
- that the head's line is in the chain with the head's hash;
- that each blob a line points to is on disk with the bytes of its hash, or that a later
  `bulky-removed` fact names it.

What it cannot see, by design:

- **Lines cut from the end, with no head.** Without a head, a record cut at a line's end is a whole
  record. The head shows it, back to the last time the head was written.
- **A record rewritten by someone with your user's rights.** Anyone who can write the files can
  change a line and compute every hash after it again, the head included. Only a copy of the head
  kept somewhere they cannot write shows it (`record-edits-show`, #362). Agents in a computer cannot
  reach these files at all: the record is on the Mac, outside every computer. The case is another
  program running as you on the Mac.

Anyone can check the chain with ordinary tools, since `silo record --json` prints the lines as
stored:

```sh
silo record main --json | head -2 | { read -r a; read -r b; \
  [ "$(printf %s "$a" | shasum -a 256 | cut -d' ' -f1)" = "$(printf %s "$b" | jq -r .prev)" ] && echo ok; }
```
