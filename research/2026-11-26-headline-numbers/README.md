# The recap's headline numbers, re-run (26 November 2026)

An agent's work, for polarzero. The silo-research recap leads with numbers from seven drafts:
#374, #385, #386, #387, #399, #401 and #402. A fresh agent took each from its own branch and ran
its "Run it" steps on a Linux cloud container (no KVM, no Mac), to see which numbers come out
again. Nothing here changes the plan. Time ran out before four of the seven were reached: they are
listed as not run, not as confirmed.

| Draft | Commit run | Claimed | Got | Verdict |
|---|---|---|---|---|
| #386 record format | `9861826` | Crash-safe at all 209 steps | 209 of 209 passed | **Reproduced** |
| #386 | | 16 of 18 tampers found | The same 19-row table, byte for byte: 16 found, 2 tampers missed, 1 legitimate removal not flagged | **Reproduced** |
| #386 | | Fuzzing finds nothing it misses | 300,000 runs, seed 44: 299,124 found, 876 unchanged, 0 missed; every count identical to `results/fuzz.txt` | **Reproduced** (300,000 of the 700,000) |
| #386 | | Model-call lines 49.6 MB with full lists, 5.5 MB with deltas (heavy night) | Same `night` output as `results/night.jsonl`, byte counts identical | **Reproduced** |
| #386 | | A night is 1.55 MB (268 KB zstd) | Needs #371's recorded night converted; not run | Not run |
| #387 catch rate | `b0fc2b4` | On 57 false claims: #377's checker 13, best fixed stack 17 | 6 + 7 = 13 and 10 + 7 = 17 | **Reproduced** |
| #387 | | Small model 43, large 49, neither wrong on 16 true claims | 13 + 30 = 43 and 20 + 29 = 49; 0 mismatches on the true claims for both | **Reproduced from the recorded answers** |
| #387 | | An injected line weakened 5 of 13 answers, none became matched | 5 of 13 weaker, 0 matched | **Reproduced from the recorded answers** |
| #387 | | `test_narrowed.py` passes | `0 wrong`, exit 0 | **Reproduced** |
| #402 way in before the relay | `fbc9606` | Mac end of the card: SSH gets in, 2 MB intact, 13 of 13 blocked, no port open | Same on two full runs | **Reproduced**, after two fixes to the run below |
| #402 | | 2 MB copied in 0.26 s by the one connection | 0.23 s, 35.29 s and 12.06 s on three runs | **Not stable**: up to 135 times slower |
| #402 | | Apple's NAT opens the Mac, the local network, the internet; the Mac's resolver sees the name | Same, every row | **Reproduced** |
| #402 | | gvproxy `-closed` opens only port 53 | Same | **Reproduced** |
| #402 | | Serial console: 2 MB in 37 s | 35.3 s and 37.0 s | **Reproduced** |
| #374 spend budgets | `803783a` | $8.465880 counted against $8.465877 billed over 134 calls; overshoot table | | Not run |
| #385 budget edges | `f1f3a32` | $2.99 to $3.03 on $3 when cut | | Not run |
| #399 attribution | `14e7cf6` | 453 of 461 file changes, none wrong | | Not run |
| #401 parsers fuzzed | `e403125` | 10 calls served under $50, 3 counted; 5.4 GB from 64 KiB | | Not run |

## What the re-runs found besides the numbers

1. **#402's `ways.sh` needs `ping`, and does not say so.** Without it the probe in main dies after
   its fourth case, silently (its stderr goes to `/dev/null`), and every way shows 4 cases instead
   of 13. Worse, Apple's NAT then shows "dns-exfil-seen-by nobody" and the internet rows vanish, so
   the run understates what NAT opens. The README's list of needs ("root, iproute2, iptables,
   sshd, socat, arping, curl and Python 3") should add `iputils-ping`, and the probe should fail
   loudly when a case cannot run. Output: [`results/402-first-run-no-ping.txt`](results/402-first-run-no-ping.txt).
2. **#402's `one-connection` way races its own start.** On the first run `fwdgram` started 0.5 s
   after `cardend` and found no socket yet (`connect: no such file or directory`), so no `tap0`
   existed and the way in failed. A second run passed. The script sleeps instead of waiting for the
   socket. Under rule 3 of AGENTS.md this is a flaky test: reported, not retried away.
3. **The one connection's 2 MB copy time is not a number to quote.** 0.23 s, 12.06 s and 35.29 s
   on the same container. The README's 0.26 s is one run. The way in works every time it starts;
   its speed through `cardend`'s netstack is not measured well enough to compare with the others.
4. **#387's model numbers are recorded answers, scored again.** The scoring reproduces exactly; the
   models were not asked again, so run-to-run variation (which #387 itself says it did not measure)
   is still open.

## What this changes in the plan

Nothing by entry id. For `way-in-before-the-relay`, #402's recommendation stands on these runs;
the speed of the one connection should not be used as an argument until it is measured over
several runs.

## Not verified

- #374, #385, #399 and #401 were not run: Zig 0.15.2, Codex 0.172.0 and Gemini CLI were installed,
  but the session's time ran out before the runs. They are the next step of this stream, in that
  order (#385 and #401 build on #374's boundary).
- Nothing ran on a Mac or with KVM.

## Run it

```
# #386
cd research/2026-11-25-record-format/proto && zig build test && zig build --release=safe
./zig-out/bin/silorec tamper /tmp/t; ./zig-out/bin/silorec crash /tmp/c
./zig-out/bin/silorec fuzz /tmp/f 300000 44; ./zig-out/bin/silorec night /tmp/n heavy full
# #387
cd research/2026-11-25-catch-rate && python3 check/catch_rate.py --set fresh --model model-reader/small-fresh.json
python3 check/tests/test_narrowed.py
# #402 (apt install iputils-ping first)
bash research/2026-11-25-gvproxy-component/build.sh /tmp/wib
(cd research/2026-11-25-way-in-before-the-relay/cardend && go build -o /tmp/wib/cardend .)
research/2026-11-25-way-in-before-the-relay/ways/ways.sh /tmp/wib nat gvproxy-closed one-connection relay serial
```

Zig 0.15.2, Go 1.24 or later, Python 3.13. Outputs are in [`results/`](results/).

## The research at a glance

Re-run from their branches: #386's crash, tamper, fuzz and night numbers and #387's catch rates
come out identical; #402's way in holds but needs `ping`, races its start once, and its 0.26 s copy
varied to 35 s; #374, #385, #399 and #401 are not yet re-run.
