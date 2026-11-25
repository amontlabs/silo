# What a receipt holds

## The choice

An agent's part, 22 November 2026. A task ran for 1 h 12 in a fork: "fix the flaky login test". You
open its receipt.

The options:

- The full account, outcome first (recommended). Your words, the test results, the 38 files changed
  and the one only in the fork, then the commands, the hosts and the secret used, $6.40 spent, 50
  minutes of work and 22 of waiting, and a gap of 2 minutes. It costs a long page, read from the
  top; most of the time you stop after the first lines.
- A short account. What you asked, the files changed and the spend. It costs what you need when
  something looks wrong: which hosts it reached, which secrets it used and what Silo missed.

Recommended: the full account, outcome first. The first lines let you judge the work in a minute,
and the rest is there when something looks wrong.

## Notes

The design record gives this as a list with no mark. Its first item, "the request that started
the run", is written here "what you asked the agent": "request" names only what waits at the
boundary.

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 7 there). Option A has no probe, so the receipt's "processes it started that are still running" is "not known yet". That costs more than its line. #377's corpus of 49 false claims from public incidents, read again with the processes taken out (`checks/catch_rate_under_a.py`): on the 30 test cases, the receipt's outcome lines alone catch 3 instead of 7, and with `claims@0.3` they catch or flag 10 instead of 13. On all 49, 22 instead of 25. A process left running is the outcome line A loses, and it is the one `processes-in-the-record` brings back.
