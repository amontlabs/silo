# Does Silo show every path by which data can leave a computer

## The choice

An agent's part, 22 November 2026. You open the page of a fork to see where its data can go. It
lists github.com and your model provider. The fork can also look up any name on the internet, and a
lookup can carry data out.

The options:

- Every path (recommended). Silo promises that the list is complete, name lookups included. It costs
  the hardest promise in this part to keep: one side way found later breaks it, so it is said in
  public only once tests that attack it pass.
- The channels Silo opened. Silo shows what it opened for the computer and does not promise that
  nothing else exists. It costs you the reason to look at the list, since you cannot take it as
  complete.

Recommended: every path. polarzero asked to know completely what goes in and out of a computer, and a list
that may be incomplete does not answer that.

## Notes

How Silo computes the paths, and shows those a grant adds, is the part `extensions`
(`grant-shows-its-paths`).

## A forwarded port is a path to the internet (24 November 2026)

An agent's part. A page served from a computer and opened on your Mac sent the computer's secret to
an attacker 26 ways in Chromium (`research/2026-11-24-red-team-fixes/preview/`). The first place
outside the computer was your Mac, so this answer would show the path as ending there, and the
data reached the internet. The suggestion: an opened port counts as a path to every host the
browser that shows it can reach, unless the page opens in a browser whose requests go through the
computer's rules. In that set-up, 0 of the 26 left, and each attempt was recorded.
