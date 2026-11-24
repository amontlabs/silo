# How is a held request refused on each kind of connection

## The choice

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

## How each agent program takes a 403 and a 429 from a model host (24 November 2026)

An agent's part, 24 November 2026. A stand-in for each program's model host answered every request
with 403, then with 429, with the body "Silo: this computer's budget is reached. Raise it in the
Silo app." in the shape every provider uses for an error. Each run lasted 60 seconds, on Linux. The
research is in `research/2026-11-24-every-agent-program`.

| Program | 403 | 429 |
|---|---|---|
| Claude Code 2.1.331 | retried, 7 requests in 60 s; nothing shown yet | the same |
| Codex 0.172.0 | 14 requests in 13 s, then stopped and showed the body | 2 requests, stopped, body not shown |
| Gemini CLI 0.71.1 | 4 requests in 16 s, body shown | 5 in 20 s, body shown |
| Qwen Code 0.28.4 | retried, 22 requests in 60 s; nothing shown | the same |
| Cline 3.0.75 | 1 request, body shown | 4 in 14 s, body shown |
| OpenCode 1.19.6 | retried, 6 requests in 33 s; nothing shown | the same |
| Goose 1.60.2 | 1 request, body shown | 7 in 7 s, body shown |
| Aider 0.86.2 | retried, 8 in 32 s, body shown each time | the same |

So the answer's reason does not hold: a 403 is retried by Claude Code, Qwen Code, OpenCode and
Aider, with growing waits. Only Cline and Goose stop at the first one. No status tried made every
program stop and show Silo's words. What the user reads has to come from Silo's own app, and the
record has to expect the same refused request several times.

## A hold the programs wait for, and a refusal they show (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`.

**While a hold lasts.** The 15 s of Codex and the 60 s of Gemini CLI in the section above are limits
on the first byte of a reply. When the status and headers were sent at once and the reply held,
Gemini CLI waited the whole 300 s, and Codex waited 300 s before sending the request again. With a byte every 5 seconds that the format documents and
that means nothing, every program tried waited the whole hold and took the reply: Anthropic's
`ping` event on Messages (Claude Code, OpenCode, Cline, Goose, Aider, Copilot CLI, 300 s), a
comment on Gemini's stream (300 s). Codex needs an event, not a comment: it waited 420 s on an
event of a name it does not know. Such an event broke OpenCode and Cline on Messages, so it is not
a general answer, and the Responses format documents no harmless event.

In a real hold the provider's reply has already started, so the headers have already gone to the
program. What a hold has to cover is the wait after them, and the measures say a documented
keep-alive byte covers it for every program but Codex, which needs an event on Responses.

**When a hold ends with a refusal.** This entry says a 403 is not retried by the usual programs.
Measured with a refusal of the model requests only: a 403 in the provider's error shape stopped
seven of eight programs, but Aider retried it and Copilot replaced Silo's words with its own. A 400
in the provider's error shape (`invalid_request_error`) stopped all eight within 9 seconds, and each
printed Silo's sentence. The table is in the notes of `budget-denies-or-holds`. So I would make the
answer 400 rather than 403, with Silo's words in the provider's error shape.
