## Proposed

Why: Conflicts between parallel tasks show up at merge, after the work is done.

- Property: Two forks whose diffs do not overlap merge in either order with the same result.
- Overlapping forks are flagged before the first of them merges.
