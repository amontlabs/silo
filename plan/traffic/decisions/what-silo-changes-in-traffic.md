# What Silo changes in model traffic

## The choice

An agent's part, 22 November 2026. A rule on main denies `git push --force`, and Silo holds main's
Anthropic API key as a secret.

The options:

- These two changes only (recommended). Silo puts the real key in on the way out, and makes the
  model see the denied call with an error that says it was denied. Everything else passes as it was
  written, and each change is in the record. It costs anything else Silo might do to traffic, such
  as trimming a long request to save money, which would each need your yes first.
- Read only. Silo changes nothing in model traffic. It costs denied tool calls, and the API key has
  to sit inside the computer, where an agent can read it.

Recommended: these two changes only. Each one does something you asked for, and the record shows it.

## Notes

- That every change is recorded is in force: `no-silent-change-to-traffic`.
- Whether a login is ever swapped in is open, with logins, in `agent-programs`.
- What the record holds of a secret is in `boundary`.
