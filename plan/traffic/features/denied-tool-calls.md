# Denied tool calls

## The choice

An agent's part, 23 November 2026. This feature lets a rule deny a tool call by a pattern on its
text, such as `git push --force`. A rule cannot deny a tool call (`rule-on-a-tool-calls-text`): a
tool call is made to fail inside the computer, by the probe, and a rule is enforced outside it.

The options:

- Drop this feature (recommended). `precaution-on-a-waiting-call` already lets any extension that
  protects deny a tool call, and a pattern on the text is one such extension. What this feature
  needed now sits with that one. It costs a pattern built into Silo: someone writes it as an
  extension.
- Keep it, as a precaution built into Silo. It costs a second way to do what an extension already
  does.

Recommended: drop it, with `plan drop`. A push to GitHub, the example this feature gave, is denied
by a network rule on GitHub, not at the tool call.
