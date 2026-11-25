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

## A strict reader, measured (24 November 2026)

An agent's part. The case for a change: this answer lets a message the boundary cannot read pass
unchanged. A request with two `Content-Length` headers is one request to the boundary and two to a
lenient host, and the second one, with a `web_fetch` tool, is never read (PR #358, A3).

A strict reader for the boundary, in Zig, is in `research/2026-11-24-red-team-fixes/strict/`. It
reads a request whole or refuses it. Of 50 test requests, it read the 4 ordinary ones and refused
the 46 hostile ones. Node, Go's `net/http`, Python's h11 and a lenient parser read 33 of the hostile
ones two ways, or found a hidden request in them. Every JSON parser tried took the last of two
duplicate keys, and Go's also matches keys whatever their case. All 49 real requests captured from
Claude Code and Codex were read, none refused.

A suggestion for the answer: on a host the boundary reads, a request that is not strictly
well-formed, or whose body the boundary cannot read whole, is refused, and the record says why.
Forwarding byte for byte stays the rule for every request that is read. That is the property
`boundary-reads-whole` that #358 proposes.
