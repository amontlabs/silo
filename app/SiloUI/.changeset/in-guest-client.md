---
"silo-ui": minor
---

An agent living inside a Linux computer can now fork it and ask for review from inside with `silo --in-guest`. `silo guest-client install <computer>` gives the computer a token the engine checks on every call: the agent can fork itself, diff its forks and ask for review, and cannot keep, promote, undo, drop or change secrets, whatever its `fold` setting. Promote and undo revoke the tokens of the computers they swap.
