# An extension tried on live work

## The choice

An agent's part, 27 November 2026. You install an extension that holds tool calls it thinks do not
follow from what you asked. On the first evening it holds forty, and thirty-eight were fine. You
remove it. The research measured the same thing on rules: on labels settled by a second labeller, the rules
for destructive calls held 90% of what should be held and wrongly held 22% of what should pass
([the research](../../../research/2026-11-25-second-labeller/README.md)).

`extension-trial-on-the-past` runs an extension on the record. It cannot replay a request whose
body the record does not keep (`response-to-an-extension`). A trial on live work can.

The options:

- A trial that only gives opinions (recommended). The extension sees each request after it left
  and says what it would have done. It costs a first period in which it protects nothing.
- Protect from the first minute. It costs your attention on every false hold, and an extension
  nobody measured.

Recommended: a trial that only gives opinions. `request-waits-only-by-precaution` rules out an
extension that looks at a request before it goes without being able to stop it. This one looks
after it went.
