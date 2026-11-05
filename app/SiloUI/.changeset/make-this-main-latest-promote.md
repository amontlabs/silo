---
"silo-ui": minor
---

Make this main now carries main's SSH entry, proxy certificate authority and hostname (macOS) to the new main, puts the fork's parked scheduled jobs back, and keeps forks of the old main pointing at whichever computer is main. `silo undo` takes back the newest of a promote and a keep, undoes only the latest promote of a computer, and refuses when the computers were renamed since rather than leave two named main. Ports changed while the fork was main stay with the main role when undone. An interrupted promote holds both computers until it is settled at start.
