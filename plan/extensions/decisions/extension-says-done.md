# An extension that says it is done

## The choice

An agent's part, 22 November 2026. Disk watch wakes every minute, reads /work on main and almost
always finds nothing to say. At 14:02 its program hangs, and main's disk fills up during the
afternoon.

The options:

- Each wake ends, and says so (recommended). The app shows "Disk watch: last check 14:01, nothing
  found". The 14:02 wake is ended at its limit, the minutes that come while it runs are skipped, and
  after three failed wakes you get an alert that Disk watch is not working. It costs the extension's
  author a program that ends at each wake rather than one that runs forever, and a slow extension
  misses wakes.
- Only when it has something to say. Disk watch alerts you when it finds something, and is silent
  otherwise. It costs telling silence from failure: you learn that it had stopped when the disk is
  full.

Recommended: each wake ends. A check that is silent only means nothing is wrong if you know it ran,
which is why scheduled jobs report how they ended and monitoring services alert on a check that
stops reporting.

## Notes

The owner asked this. It has no answer yet.
