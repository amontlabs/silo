# Where a rule on the age of packages is enforced

## The choice

An agent's part, 22 November 2026. You set a rule that no package younger than seven days is
installed. An agent fetches the package by hand with curl, to get past the setting of the package
manager.

This is already settled by two decisions you took: `where-limits-are-set` (limits are set at the
boundary, never inside a computer) and `rules-inside-are-no-boundary`, which gives this very case.
You ruled on an earlier wording of this entry, so only you can close it.

The options:

- At the boundary (recommended). Silo reads the package requests where they leave the computer, so
  the rule holds whatever tool the agent uses. It costs nothing new.
- In the package manager inside the computer. It costs the rule: any other tool gets around it.

Recommended: at the boundary. Taking it changes nothing; it closes a question you already answered.

## Notes

An agent's part. The design record marks this line "(Notes for the rule; Agreed for where it
sits)" (l.430–431). The owner agreed where such a rule sits. Whether the rule exists at all is
from the notes, so this waits for `package-age-rule`.
