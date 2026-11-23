# What Silo changes in model traffic

## The choice

An agent's part, 23 November 2026. Silo holds main's Anthropic API key as a secret, and an
extension that protects main denies a tool call.

The options:

- One change only (recommended). Silo puts the real key in on the way out, and the record shows
  that it did. The denied tool call is made to fail inside main, and the reply passes unchanged
  (`denied-tool-call-mechanism`). Everything else passes as it was written. It costs anything else
  Silo might do to traffic, such as trimming a long request to save money, which would need your
  yes first.
- Read only. Silo changes nothing in model traffic. It costs the API key, which then has to sit
  inside the computer, where an agent can read it.

Recommended: one change only. It does something you asked for, and the record shows it.

## Notes

- That every change is recorded is in force: `no-silent-change-to-traffic`.
- Until 23 November 2026, a denied tool call was a second change, made to the reply and to the
  later requests. The way proposed for a denial is now the probe, which changes nothing.
- Whether a login is ever swapped in is open, with logins, in `agent-programs`.
- What the record holds of a secret is in `boundary`.
