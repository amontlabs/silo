# What the record keeps of secrets

Research of 25 November 2026, by agents. It measures how much of a secret's value the record
format's redactor (#386, `proto/src/redact.zig`) leaves in the record, on keys shaped like the real
ones and in the places they appear on a night. It then measures two other redactors: one that knows
the same values and also looks for their other forms, and one that knows no value and looks for the
shape of common keys.

No value in this folder is real. The corpus makes every value at random when it runs, and nothing
it makes is written to the repository.

## What was found

1. **#386's redactor catches a value only as it is: 444 of 816 samples.** It held in every prompt
   and every plain tool output. It missed whole every value in `Authorization: Basic`, in a base64
   file such as a Kubernetes secret, and in hex. It missed every private key inside a JSON request
   body, because each line break is written `\n` there. It missed every password whose special
   characters a URL, JSON or a shell trace escapes. `secret-never-written-down` says "never", so
   these are breaks of a property in force, not edge cases.

2. **Looking for the forms of a value takes 733 of 816 whole, and 30 more in part.** The forms are
   the value JSON escaped once and twice, with Go's escapes, percent-encoded, base64 and base64url
   at the three alignments a value can have inside other bytes, hex, and shell-quoted. A match is
   also followed across line breaks, real or escaped, so a key rewrapped at 76 columns, with CRLF
   ends, or cut by an 80-column screen is taken whole. `redactors.py`, `Forms`.

3. **Two places no redactor of single strings can cover.** A model's reply kept raw as it streams
   puts a few characters of the value in each event: 0 of 51 caught by any method. An output read
   in two pieces and written as two lines leaves the short end of the value on each side: 19 of 51
   whole, 30 partial. The record format already stores the reply as a message (`store-messages-once`),
   so the rule is to redact the assembled message, never the raw stream, and to redact an output
   before it is cut into lines.

4. **#386's table of starting pairs does not scale to the forms.** Every form is a pattern, about
   ten per secret. With the forms of 50 secrets it ran at 115 MB/s, and with 200 at 6 MB/s, on
   8.5 MB of code. A lookup of windows of 16 bytes through a hashed bit table ran at 740 MB/s for 50
   secrets and 254 MB/s for 200, against 595 MB/s with no secret at all (`bench/bench.c`,
   `results/cost.jsonl`). At 1,000 secrets it ran at 65 MB/s, because the bit table is fixed at
   4 Mbit; sizing it to the number of windows is left to the build. One run, best of five; the row
   for 10 secrets (370 MB/s) is lower than the row for 50 and was not run again.

5. **Values Silo was never given are the record's larger exposure.** By `secret-entry`, a key in a
   file inside a computer is not a secret, and #386's redactor never sees it. An agent that reads
   `.env` sends it to the model, and the record keeps that request. Rules for the formats of 17
   common keys took 536 of the 816 samples whole with no value given. They miss keys with no prefix
   (an AWS secret key, an Azure key, a database password) unless a name such as `aws_secret` or
   `password` sits beside them, and every value in hex. This is a new choice,
   `secret-shaped-values`.

6. **The shape rules are cheap and rarely wrong, once the name rules are checked.** On 40 MB of
   this repository's text and code (main and `legacy-2026-11-16`), the format rules fired 25 times,
   all on test keys in test or vendored code. The rules that read a name (`password = …`) fired 789
   times on code such as `let password = read()`. Requiring the value to look random (letters and
   digits, no call syntax, entropy of 3 bits per character or more) brought that to 29, and cost one
   sample of recall. Run as 18 Python regular expressions in turn they reach 2.8 MB/s; one pass of
   Hyperscan over the same rules reached 1,030 MB/s (`bench/hyperscan_shapes.py`).

### Coverage by place

Samples taken whole, of 51 for each place (17 formats, three values each). Full tables, by format
too, in [`results/coverage.md`](results/coverage.md).

| Place | #386 | Forms | Shapes, with decoding |
|---|---|---|---|
| Prompt | 51 | 51 | 36 |
| Tool output | 51 | 51 | 43 |
| File in a request body | 45 | 51 | 42 |
| JSON escaped the Go way | 45 | 51 | 42 |
| Tool arguments, JSON in JSON | 45 | 51 | 48 |
| URL query, percent-encoded | 36 | 51 | 51 |
| URL password, percent-encoded | 36 | 51 | 51 |
| `Authorization: Basic` | 0 | 51 | 51 |
| Base64 inside a file | 0 | 51 | 45 |
| Hex | 0 | 51 | 0 |
| Shell trace | 39 | 51 | 42 |
| Split across two lines | 0 | 19, 30 partial | 0, 16 partial |
| CRLF line ends | 45 | 51 | 42 |
| Rewrapped at 76 | 27 | 51 | 21 |
| Screen of 80 columns | 24 | 51 | 24 |
| Raw model stream | 0 | 0 | 0 |
| **All, of 816** | **444** | **733** | **536** |

## What it changes in the plan

| Entry | Status | Change |
|---|---|---|
| `secret-values` | answered, no verdict | Notes: what the recorder must look for to keep `secret-never-written-down`, and the cost; it also answers the entry's "how replies are searched without slowing them" for the record |
| `secret-shaped-values` | new, open | What the record does with a value shaped like a secret that Silo was not given. Recommended: replaced by its shape, `[secret-like:<format>]` |

The record format (#386) would change in its section "Secret values": the redactor looks for the
forms of each value through hashed windows, follows a match across line breaks, and runs on whole
messages and whole outputs. That is a change to a draft, so it is written here and not pushed to
#386's branch.

## For polarzero to rule

- `secret-shaped-values`: replace a value shaped like a key by its shape, keep it, or keep it and
  mark the line.
- The exchanged tokens of #356 are real values inside a computer, which `secret-values`'s sentence
  leaves out (#356 already challenges it). Whatever that sentence becomes, the recorder knows those
  tokens, since Silo made them, and can redact them like a secret at no extra cost.

## What was not verified

- The redactors are Python models of a Zig writer, and the costs come from a C program, not from
  #386's Zig code: running a downloaded Zig toolchain was refused in this container. The C port of
  #386's table of starting pairs runs at 595 MB/s with no secret on code, where #386 measured
  286 MB/s on #371's lines. The ratios between methods are the finding, not the absolute numbers.
- The formats are shaped by their public descriptions (prefix, alphabet, length). The checksums
  some providers embed (GitHub's last six characters, for example) are not computed, and no rule
  checks them, which would remove false positives further.
- The places are built by hand from how Claude Code, Codex and common tools carry text. They were
  not captured from a real night.
- How often a real night puts a key into the record at all was not measured.

## Run it

```
python3 measure.py > results/coverage.md          # coverage, about 2 s
python3 falsepos.py origin/main legacy-2026-11-16 > results/false-positives.md   # from the repository root
python3 bench/run.py > results/cost.jsonl          # cost, needs cc
python3 bench/hyperscan_shapes.py >> results/cost.jsonl   # needs pip install hyperscan
```

| File | Holds |
|---|---|
| `corpus.py` | The 17 formats and the 16 places, made at run time |
| `redactors.py` | `Exact` (#386, ported), `Forms`, `Shapes` |
| `measure.py` | The coverage tables |
| `falsepos.py` | Where the shape rules fire on the repository |
| `bench/` | The cost: `bench.c` for known values, `hyperscan_shapes.py` for shapes |
| `results/` | What each script printed |
