# Whether anything done inside a computer can change its rules

## For the owner

An agent's part, 22 November 2026. Main is limited to github.com. An agent in main, with `sudo`,
tries to reach paste.rs: it edits the network settings, changes `/etc/hosts`, ends processes and
sets up a proxy of its own.

The options:

- Promise it (recommended). None of that changes the rule, and tests that attack a real computer
  from inside keep it true. It costs those tests, which must pass before Silo says it in public. An
  agent that needs another host asks you.
- Do not promise it yet. Rules are applied outside the computer, and Silo claims nothing more. It
  costs the sentence the rest rests on: "a hostile agent can do what the rules allow and nothing
  else".

Recommended: promise it. Full access inside a computer is acceptable only because the rules are out
of its reach.

A proposal the design relies on, not yet decided. That a rule
is enforced by the engine, outside the computer, is in force (`how-a-rule-is-enforced`).
