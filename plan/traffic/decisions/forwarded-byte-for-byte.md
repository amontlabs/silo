# What the boundary forwards between an agent program and its provider

## The choice

An agent's part, 25 November 2026. #355 (page 5) challenged this entry together with
`what-silo-changes-in-traffic`: swapping a value you just made a secret in later model requests is a
change to the body. That change now lives in `what-silo-changes-in-traffic`, which this answer
points to, so nothing here changes for it. #355 measured that it costs no prompt-cache hit.

One sentence is added, from #358 (attack A3), #363 and #373. The case: an agent sends a request with
two `Content-Length` headers, or tabs inside a header, or two copies of a JSON key. Silo reads one
request and the host reads another, so a rule checks what the host never runs. #358 got past the
first boundary prototype this way, 7 ways; #363's strict reader refused 46 of 46 such requests and
read 49 of 49 real ones.

The options:

- Refuse a request whose framing can be read in two ways (recommended). It costs a program that
  sends such a request by mistake, which none of the 49 real ones did.
- Forward it, as the earlier answer allows. It costs every rule on that request.

Recommended: refuse it. It is the one case where forwarding as written is what gets around the
boundary.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
