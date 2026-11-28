# What is in Silo's MVP

## The choice

An agent's part, 28 November 2026. You want to start shipping Silo on your Mac and use it with your
agent programs every day, without wading through 159 undecided decisions before writing the first
line of code.

The options:

- Tight daily-driver MVP (recommended). Build the Zig engine, the `silo` command with JSON lines, a
  Linux main on your Mac reached over SSH with browser sign-in, an outside network boundary
  (open, limited or closed), the append-only record, APFS disk checkpoints with restore and forks,
  outside file diffs, model traffic at the boundary, and receipts comparing what an agent claimed
  with what Silo saw in `silo` and a small Swift Mac app. It costs deferring the extension runtime,
  classifiers, precautions, held requests, Nix recipes, secret placeholders and a macOS main until
  after MVP.
- Keep `first-build-shows-the-outside` as written. Build the extension runtime, classifiers,
  precautions, held requests, Nix recipes and secret placeholders before restore, forks and the app.
  It costs 32 features and 159 undecided decisions before the first build finishes, and leaves
  restore, forks and receipts out of it.

Recommended: tight daily-driver MVP. It gives you the real daily workflow (full access inside a
Linux VM, automatic rewind and forks, tamper-proof outside record, claims beside facts in a receipt,
and JSON lines that agents can query right away) in 14 features and 18 decisions, on the exact Zig
core and boundary that everything after MVP plugs into.

## polarzero's words, 28 November 2026

polarzero confirmed this MVP cut and asked for a Linux main first and an aggressive cleanup of the
plan so fresh conversations can decide and build one step at a time.
