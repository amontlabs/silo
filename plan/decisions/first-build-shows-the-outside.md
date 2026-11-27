# What the first build shows

## The choice

An agent's part, 27 November 2026. The first build proposed on 24 November
([the research](../../research/2026-11-24-first-build.md)) is a macOS main with checkpoints,
restore and a record of file changes. It says of itself: "There is no app, no rules, no model
traffic", and it leaves the boundary to the third build. polarzero then said what Silo is for, in
the words quoted in `one-sentence`, and added on 27 November:

> I think that whole thing really should be a good mvp we want to achieve fast.

The build of 24 November shows going back. It shows nothing of what polarzero named: reading from
outside, comparing with what is reported from inside, and an extension at the boundary.

What you would do with the build proposed here:

1. You run Claude Code with every approval off in a computer that holds only a placeholder for
   its API key. `silo` lists each model call the boundary read: the model, the tokens, the tool
   calls. These are facts.
2. Beside each call sits Claude Code's own account of it, a report, joined by the provider's
   request id (`own-account-as-a-check`). A model call made with `curl`, around Claude Code, has
   no account beside it and shows as a mismatch. When the agent ends a turn with "All tests pass",
   the extension `said-against-done` gives its opinion on that sentence from the record.
3. The extension `transaction-guard` holds one request a program in the computer sends. You answer
   the ask in `silo`, and the record keeps the ask and your answer.

The options:

- The outside first (recommended). The build is the engine, one computer, the record, the
  boundary with its rules and holds, model traffic read, the agent program's own account, and
  extensions. Checkpoints follow, because the record of file changes compares the disk at each
  checkpoint with the one before (`file-changes-from-outside`), and restore follows them. It costs
  more decisions before anything is built: by an agent's count with a script, about 130 with no
  verdict, against 70 for the build of 24 November. It also costs going back, which arrives later.
- The build of 24 November. It needs fewer decisions and gives a Silo you can sleep on. It costs a
  first build that an agent program with a snapshot tool could match, and it shows none of the
  three steps above.
- Both at once. It costs the most decisions, and the first thing you can try arrives last.

Recommended: the outside first.

## What has run, and what has not

An agent's part, 27 November 2026, from reading the research. It bounds what "fast" can mean.

- Every prototype ran on Linux, in containers. No virtual machine has booted under Silo, and
  nothing has run on a Mac.
- One boundary written in Zig passed its 39 cases: the placeholder swapped for a secret, a
  certificate per host, a recorder, and holds answered by a separate process
  ([the research](../../research/2026-11-25-boundary-end-to-end/README.md)). Its computers were
  network namespaces. Claude Code ran through it against stand-in providers.
- No real API key carried agent traffic through it.
- Claude Code's own account matched the traffic on every request, behind an ordinary inspecting
  proxy and not behind Silo's boundary (`own-account-matches`).
- The reader of `agent-summary-checked` was measured on records rebuilt from public issues, never
  on the record of a real night.
- Nothing about a transaction guard has been tried.

## Which system main runs first

An agent's part, 27 November 2026. This choice is polarzero's, in `main-on-macos-first`, where the
lean of 18 November is a macOS main. The first build proposed here reaches its three steps sooner
with a Linux main on the Mac, for two reasons. Every extension that executes runs in a Linux
computer (`extension-ships-a-computer`), so the Linux driver is needed in this build anyway. And
the macOS main waits on macOS 27, on Mac tests that have not run, and, for its processes, on a
permission Apple has not granted. The notes of `main-on-macos-first` give the case. Nothing in this
entry decides it, and the order of the stage "app" keeps `computers-main-on-a-mac` where it was.

## The features, in order

`access-model-checked`, `engine-program-and-clients`, `silo-command`, `engine-matches-wanted`, the
computer, `agent-programs-run-unmodified`, `agent-programs-reach-every-computer`,
`record-of-a-computer`, `allow-hold-or-deny`, `open-limited-or-closed`, `secrets-held-outside`,
`model-traffic-read`, `model-calls-in-the-record`, `program-account-checked`,
`refusals-that-say-so`, `held-requests-and-asks`, `opinions-beside-the-record`,
`importing-an-extension`, `extension-that-reads-and-alerts`, `said-against-done`,
`extension-that-protects`, `transaction-guard`, then `checkpoints-taken-on-their-own` and
`file-changes-in-the-record`. The stage "app" of `order.json` follows this, with the features each
of these needs placed before it.
