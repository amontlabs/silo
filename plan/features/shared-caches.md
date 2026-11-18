## Proposed

Why: Build output is written again in every fork, and on one Mac it reached 150 GB every few days.

## Open

- May forks write to one shared cache, though fork's notes say a fork's cache writes never reach another fork?
  - Suggested: No; a shared writable cache is a path between computers, so measure the disk use first.
- To find out: How much disk each fork adds through its own build output, such as Rust's target folder, over a day of work.
