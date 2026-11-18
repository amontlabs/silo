## Proposed, not confirmed by the owner

Why: Build output is written again in every fork, and on one Mac it reached 150 GB every few days.

### For the owner to settle

- May forks write to one shared cache, though fork's notes say a fork's cache writes never reach another fork?
  - Recommendation: No; a shared writable cache is a path between computers, so measure the disk use first.

### For whoever builds it

- To find out: How much disk each fork adds through its own build output, such as Rust's target folder, over a day of work.
