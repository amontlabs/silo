# What is in Silo's MVP

## The choice

An agent's part, 28 November 2026. You want to start shipping Silo on your Mac and use it with your
agent programs every day, without wading through more than a hundred undecided decisions before
writing the first line of code.

The options:

- Tight daily-driver MVP with boundary secrets, full Swift Mac app control and one settled API
  (recommended). Settle the engine API first (`mvp-api`), then build the Zig engine, the views-only
  Swift Mac app and the `silo` command over that same API, a Linux main on your Mac reached over SSH
  with browser sign-in, an outside network boundary (open, limited or closed), plain secret
  placeholder swapping at the boundary, the append-only record, APFS disk checkpoints with restore
  and forks, outside file diffs, model traffic at the boundary, and receipts comparing what an agent
  claimed with what Silo saw, with copy and export buttons so you can hand a receipt and its record
  data to any agent for analysis. It costs deferring tools inside computers, a macOS main, the
  extension runtime, classifiers, precautions, held requests, Nix recipes, non-swap secrets (SSH
  signing, OAuth exchanges, cloud signing) and spend budgets until after MVP.
- Keep `first-build-shows-the-outside` as written. Build the extension runtime, classifiers,
  precautions, held requests and Nix recipes before restore, forks and the Mac app. It costs dozens
  of features and over a hundred undecided decisions before the first build finishes, and leaves
  restore, forks and receipts out of it.

Recommended: tight daily-driver MVP with boundary secrets, full Swift Mac app control and one
settled API. It gives you the daily workflow (full access inside a Linux virtual machine, plain
secrets kept outside the computer, automatic checkpoints with restore and forks, an append-only
record, and receipts you can read in the Mac app or copy and export to an agent) in 16 features and
30 decisions, built API-first on the same Zig engine and boundary that everything after MVP plugs
into.

## polarzero's words, 28 November 2026

polarzero confirmed the Linux-main-first MVP cut and refined it with four points:

- Include plain secret placeholder swapping at the boundary (`secrets-held-outside`).
- Handle every MVP control and view from the Swift Mac app so you do not have to touch the command
  line.
- Cut Silo tools inside computers from MVP, and focus instead on copy and export in the interface so
  you can export a full receipt and its relevant record and diff data to give to an agent for
  analysis.
- Settle the API contract up front (`mvp-api`) and build API-first.
