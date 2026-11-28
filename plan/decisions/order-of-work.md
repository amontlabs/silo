# The order of work

## The choice

An agent's part, 28 November 2026. You want to get from zero product code to a daily-driver Silo on
your Mac, one feature at a time, without blocking on the entire long-term platform first.

The options:

- Tight MVP first (recommended). Build the Zig engine and `silo` command, a Linux main on your Mac
  with SSH and browser sign-in, the outside network boundary and record, APFS disk checkpoints with
  restore and forks, outside file diffs, model traffic at the boundary, and receipts in `silo` and a
  small Swift Mac app. A macOS main, extensions, classifiers, precautions, recipes and budgets come
  in the stage after MVP. It costs deferring macOS-native builds and the extension runtime until
  after the first daily-driver release works.
- Full platform and macOS main first. Build macOS main, Nix recipes, classifiers, precautions, holds
  and the extension runtime before shipping a usable product. It costs a long stretch of undecided
  ground work before anything runs on your Mac.

Recommended: tight MVP first. Every stage leaves a working product, and every piece after MVP plugs
into the same Zig core, boundary and record.

## Notes

Earlier notes from 22 November 2026 leaned toward a macOS main first. On 28 November 2026 polarzero
confirmed starting with a Linux main and a tight MVP track in `order.json`.
