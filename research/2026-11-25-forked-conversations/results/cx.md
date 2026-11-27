# Codex: 24 model requests in 12 steps

## What each step sent

| Step | Computer | Case | session-id / thread-id | prompt_cache_key | window | installation_id | history_initialization | forked_from_thread_id |
|---|---|---|---|---|---|---|---|---|
| 1 | main | A | `a` | `b` | `c` | `d` | new | none |
| 2 | main | A | `a` | `b` | `c` | `d` | cold_resume | none |
| 3 | fork | B | `a` | `b` | `c` | `d` | cold_resume | none |
| 4 | fork | B | `a` | `b` | `c` | `d` | cold_resume | none |
| 5 | main | A | `a` | `b` | `c` | `d` | cold_resume | none |
| 6 | forkid | C | `a` | `b` | `c` | `d` | cold_resume | none |
| 7 | old | D | `a` | `b` | `c` | `d` | cold_resume | none |
| 8 | fork2 | E | `a` | `b` | `c` | `d` | cold_resume | none |
| 9 | main | F | `e` | `f` | `g` | `d` | cold_fork | `b` |
| 10 | main | F | `e` | `f` | `g` | `d` | cold_resume | `b` |
| 11 | main | A | `a` | `b` | `c` | `d` | cold_resume | none |
| 12 | main* | G | `a` | `b` | `c` | `d` | cold_resume | none |

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
| 1 | main | A | 0.0875 | 0.0875 | 0.0875 | 0.0875 | 0.0875 | 0.0875 |
| 2 | main | A | 0.0207 | 0.1083 | 0.1083 | 0.1083 | 0.1083 | 0.1083 |
| 3 | fork | B | 0.0210 | 0.1292 | 0.0210 | 0.1292 | 0.1292 | 0.1292 |
| 4 | fork | B | 0.0212 | 0.1504 | 0.0422 | 0.1504 | 0.1504 | 0.1504 |
| 5 | main | A | 0.0208 | 0.1712 | 0.1290 | 0.1290 | 0.1290 | 0.1290 |
| 6 | forkid | C | 0.0212 | 0.1924 | 0.0212 | 0.1502 | 0.1502 | 0.1502 |
| 7 | old | D | 0.0208 | 0.2132 | 0.0208 | 0.1291 | 0.1498 | 0.1291 |
| 8 | fork2 | E | 0.0214 | 0.2346 | 0.0214 | 0.1719 | 0.1719 | 0.1719 |
| 9 | main | F | 0.0210 | 0.2556 | 0.1501 | 0.1501 | 0.1501 | 0.1501 |
| 10 | main | F | 0.0214 | 0.2771 | 0.1715 | 0.1715 | 0.1715 | 0.1715 |
| 11 | main | A | 0.0210 | 0.2981 | 0.1925 | 0.1925 | 0.1925 | 0.1925 |
| 12 | main* | G | 0.0213 | 0.3193 | 0.0213 | 0.1717 | 0.1717 | 0.2138 |

## A fork's first request and the provider's cache

The provider keeps a cache per key, not per computer. An estimate that keeps the cache per
computer prices a fork's first request as if nothing were cached.

| Step | Computer | Case | Cache read, billed | Billed | Estimate, nothing cached | Error |
|---|---|---|---|---|---|---|
| 3 | fork | B | 15357 of 15439 tokens | 0.0104 | 0.0795 | +662% |
| 6 | forkid | C | 15587 of 15667 tokens | 0.0105 | 0.0807 | +665% |
| 7 | old | D | 15400 of 15441 tokens | 0.0103 | 0.0796 | +676% |
| 8 | fork2 | E | 15817 of 15899 tokens | 0.0107 | 0.0818 | +667% |
| 12 | main* | G | 15860 of 15901 tokens | 0.0105 | 0.0819 | +681% |

## What the record stores, each message once by hash

| Stored | Bytes |
|---|---|
| Every request as sent | 1540881 |
| Messages once, a store per computer | 375341 |
| Messages once, one store per Mac | 67250 |

