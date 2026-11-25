# How is a held request refused on each kind of connection

## The choice

An agent's part, 25 November 2026, from the challenges of #355 and #367, measured again in #369, as #364
reconciles them.

The case: main has a budget that is reached while Claude Code, Codex and Aider are working, and an
agent's `git push` to a held branch is refused. The earlier answer sends a 403 to all of them and
says a 403 is not retried.

What was measured: with a 403 on the model request, Aider retried and Copilot CLI showed its own
words; Claude Code retries a 403 when its sign-in calls are refused too (#367, explained in #369).
With 400 in the provider's error format (`invalid_request_error`, or `{"error": {"code": 400}}`
for Google), all eight programs tried stopped within 9 seconds and printed Silo's sentence. A 403 on
a push makes git print "Everything up-to-date" (#355).

The options:

- 400 in the provider's format for model requests, and Git's own report for a push (recommended).
  It costs a status that means "your request is wrong", which a budget is not; a program may one
  day answer it by changing the request, so the status is part of each program's live check.
- One status per program, as #347 proposed (451 and 402 for Claude Code). It costs a table to keep
  per program, and 402 sends Goose's users to their provider's billing.
- A 429 with a budget code, as #351 read from the programs' code. Codex hides the body of a 429,
  so Silo's words are lost.
- A 403 everywhere, as the earlier answer says.

Recommended: 400 in the provider's format, and Git's own report for a push. #369 ran eight programs
where the others ran two or read code, and it is the only status every one of them stopped on and
showed. #353's finding, that each provider's API is best refused in that provider's own error form,
fits the second sentence of the answer once its entries are merged.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. A rule holds every request from main to api.stripe.com, and you
do not answer before the hold's limit. Another rule holds an `ssh` connection from main to your
server.

The options:

- In each connection's own terms (recommended). A web request Silo reads gets the status 403, with
  words that say held and not answered, or denied. The usual programs do not retry a 403 by
  themselves. The `ssh` connection, which Silo does not read, is reset. A name lookup is refused.
  Other traffic with no connection to reset is told the port cannot be reached, so the program stops
  waiting at once. It costs the words on the `ssh` side, since a reset looks like a network fault to
  the agent. Which status and which words each agent program reads best is still to find out.
- As a server error. The request gets the status 503, as from a service that is down. It costs
  repeated asks: programs retry a 503 by themselves, and each retry is held and asks you again.
- In silence. Nothing comes back. It costs the agent's time: it waits for its own timeout, often
  minutes, then tries again.

Recommended: in each connection's own terms. A refusal that programs do not retry by themselves
means one ask per request, and the agent learns at once what happened.

## Notes

Left for when it is built, with what the message of a hold says. What is known:

- Silo can put words in a reply only while the caller still listens and where the reply has room
  for words.
- A raw connection can only be refused.
