# What the boundary forwards between an agent program and its provider

## The choice

An agent's part, 22 November 2026. Claude Code in main sends Anthropic a conversation of 100 000
tokens on every turn, with your Max plan's sign-in in each request.

The options:

- Unchanged, and the sign-in kept nowhere (recommended). Each request and answer arrives as it was
  written, apart from the changes Silo records, so Anthropic's cache works as it does without Silo.
  Your sign-in passes and is written nowhere. It costs a record that cannot replay a request
  exactly, since it lacks the sign-in. Anthropic can also tell that something other than Claude Code
  made the connection, as with a company's proxy.
- A full copy, sign-in included, kept encrypted. You could replay or inspect any request whole. It
  costs a copy of your login on disk, and Silo then stores a subscription's sign-in, which
  Anthropic's terms forbid to a developer.

Recommended: unchanged, and the sign-in kept nowhere. Your bill and your agents behave as they do
without Silo, and nothing of your login stays behind.

## Notes

"The changes listed above" are those of `what-silo-changes-in-traffic`.

That Silo's own boundary forwards byte for byte is still to find out: `reading-each-agent-program`.
