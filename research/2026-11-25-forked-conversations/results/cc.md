# Claude Code: 24 model requests in 12 steps

## What each step sent

| Step | Computer | Case | session header | metadata session | device_id |
|---|---|---|---|---|---|
| 1 | main | A | `a` | `a` | `b` |
| 2 | main | A | `a` | `a` | `b` |
| 3 | fork | B | `a` | `a` | `b` |
| 4 | fork | B | `a` | `a` | `b` |
| 5 | main | A | `a` | `a` | `b` |
| 6 | forkid | C | `a` | `a` | `b` |
| 7 | old | D | `a` | `a` | `b` |
| 8 | fork2 | E | `a` | `a` | `b` |
| 9 | main | F | `c` | `c` | `b` |
| 10 | main | F | `c` | `c` | `b` |
| 11 | main | A | `a` | `a` | `b` |
| 12 | main* | G | `a` | `a` | `b` |

Each letter stands for one distinct value; the same letter is the same value.

## What each step's first request continues, by content

| Step | Computer | Case | Continues request | Sent by | Of step |
|---|---|---|---|---|---|
| 1 | main | A | none | | |
| 2 | main | A | 2 | main | 1 (A) |
| 3 | fork | B | 4 | main | 2 (A) |
| 4 | fork | B | 6 | fork | 3 (B) |
| 5 | main | A | 4 | main | 2 (A) |
| 6 | forkid | C | 10 | main | 5 (A) |
| 7 | old | D | 4 | main | 2 (A) |
| 8 | fork2 | E | 8 | fork | 4 (B) |
| 9 | main | F | 10 | main | 5 (A) |
| 10 | main | F | 18 | main | 9 (F) |
| 11 | main | A | 10 | main | 5 (A) |
| 12 | main* | G | 8 | fork | 4 (B) |

## How each way groups the requests

| Step | Computer | Case | Across computers (the answer as written) | One computer, fork starts at zero | One computer, linked to the parent at the fork |
|---|---|---|---|---|---|
| 1 | main | A | `#1` | `main#1` | `main#1` |
| 2 | main | A | `#1` | `main#1` | `main#1` |
| 3 | fork | B | `#1` | `fork#2` | `fork#2` (from main#1 at 4) |
| 4 | fork | B | `#1` | `fork#2` | `fork#2` (from main#1 at 4) |
| 5 | main | A | `#1` | `main#1` | `main#1` |
| 6 | forkid | C | `#1` | `forkid#3` | `forkid#3` (from main#1 at 10) |
| 7 | old | D | `#1` | `old#4` | `old#4` (from main#1 at 4) |
| 8 | fork2 | E | `#1` | `fork2#5` | `fork2#5` (from fork#2 at 8) |
| 9 | main | F | `#1` | `main#1` | `main#1` |
| 10 | main | F | `#1` | `main#1` | `main#1` |
| 11 | main | A | `#1` | `main#1` | `main#1` |
| 12 | main* | G | `#1` | `main*#6` | `main*#6` (from fork#2 at 8) |

- Across computers (the answer as written): 1 conversations, 1 holding requests of more than one computer.
- One computer, fork starts at zero: 6 conversations, 0 holding requests of more than one computer.
- One computer, linked to the parent at the fork: 6 conversations, 0 holding requests of more than one computer.

## What a conversation budget counts at the end of each step

Dollars as the stand-in billed. `across` counts every computer's turns in one conversation;
`fresh` starts a fork's at zero; `linked` starts it at what the parent's had spent at the fork.

`now` is #385's `carry` as built; `linked-unsplit` is `linked` when Silo does not treat main
as a new computer after a fork replaces it.

| Step | Computer | Case | Step's own cost | across | fresh | linked | now | linked-unsplit |
|---|---|---|---|---|---|---|---|---|
| 1 | main | A | 0.1021 | 0.1021 | 0.1021 | 0.1021 | 0.1021 | 0.1021 |
| 2 | main | A | 0.0167 | 0.1188 | 0.1188 | 0.1188 | 0.1188 | 0.1188 |
| 3 | fork | B | 0.0169 | 0.1357 | 0.0169 | 0.1357 | 0.1357 | 0.1357 |
| 4 | fork | B | 0.0170 | 0.1527 | 0.0339 | 0.1527 | 0.1527 | 0.1527 |
| 5 | main | A | 0.0166 | 0.1693 | 0.1354 | 0.1354 | 0.1354 | 0.1354 |
| 6 | forkid | C | 0.0170 | 0.1863 | 0.0170 | 0.1524 | 0.1524 | 0.1524 |
| 7 | old | D | 0.0166 | 0.2029 | 0.0166 | 0.1354 | 0.1520 | 0.1354 |
| 8 | fork2 | E | 0.0171 | 0.2201 | 0.0171 | 0.1699 | 0.1699 | 0.1699 |
| 9 | main | F | 0.0167 | 0.2368 | 0.1521 | 0.1521 | 0.1521 | 0.1521 |
| 10 | main | F | 0.0171 | 0.2539 | 0.1693 | 0.1693 | 0.1693 | 0.1693 |
| 11 | main | A | 0.0167 | 0.2706 | 0.1860 | 0.1860 | 0.1860 | 0.1860 |
| 12 | main* | G | 0.0168 | 0.2875 | 0.0168 | 0.1696 | 0.1696 | 0.2028 |

## A fork's first request and the provider's cache

The provider keeps a cache per key, not per computer. An estimate that keeps the cache per
computer prices a fork's first request as if nothing were cached.

| Step | Computer | Case | Cache read, billed | Billed | Estimate, nothing cached | Error |
|---|---|---|---|---|---|---|
| 3 | fork | B | 12599 of 12682 tokens | 0.0087 | 0.0811 | +836% |
| 6 | forkid | C | 12707 of 12787 tokens | 0.0087 | 0.0818 | +840% |
| 7 | old | D | 12651 of 12683 tokens | 0.0084 | 0.0811 | +869% |
| 8 | fork2 | E | 12815 of 12898 tokens | 0.0088 | 0.0825 | +840% |
| 12 | main* | G | 12867 of 12900 tokens | 0.0085 | 0.0825 | +871% |

## What the record stores, each message once by hash

| Stored | Bytes |
|---|---|
| Every request as sent | 1237118 |
| Messages once, a store per computer | 77701 |
| Messages once, one store per Mac | 14697 |

