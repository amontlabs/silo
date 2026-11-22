# Reports matched to facts

## The choice

An agent's part, 22 November 2026. A program inside main says it wrote 2 MB to `out.log`. The disk,
seen from outside, shows that `out.log` did not change.

The options:

- Build it (recommended). That report gets "mismatch", and a report the disk agrees with gets
  "matched". A matched report is still unverified. It costs one more mark on screen, and one thing
  to learn: matched does not mean seen.
- Leave it out. Each report stands alone. It costs the one signal that what the inside says and what
  Silo saw disagree.

Recommended: build it. The comparison is cheap once both the file changes and the processes are in
the record.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

Matched and mismatch are opinions, so they are kept beside the record and say whose they are. This
feature therefore comes after `opinions-beside-the-record`. Which classifier gives them is not
said, and how a report is matched to a fact is open: `matching-reports-to-facts`.
