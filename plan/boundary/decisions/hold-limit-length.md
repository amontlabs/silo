# How long may a hold last

## The choice

An agent's part, 22 November 2026. At 10:00 a command of an agent in main pushes to a host that a
rule holds. You are in a meeting until 10:30. The agent program ends any command that runs for more
than two minutes.

The options:

- Short, under the program's own patience (recommended). The hold lasts 90 seconds unless the rule
  says otherwise. The agent then gets Silo's message, held and not answered, and can try again
  later. It costs you being at the Mac to say yes, and a second try by the agent when you were not.
- Long, so that you can answer when you are back. The hold lasts an hour. It costs the message: the
  program gives up after its two minutes, the agent sees a bare timeout, and your yes at 10:30 lets
  nothing pass because the request is gone.

Recommended: short, under the program's own patience. A hold only works while the program still
waits, so a long limit gives you an ask that no yes can serve. The number is a start, to be
measured.

## Notes

To find out. The record asks, with it, how each agent program takes the error at the limit.

Nothing has been measured yet.

## How long each agent program waits on a model reply (24 November 2026)

An agent's part, 24 November 2026. A stand-in for each program's model host never answered a model
request, not even with a first byte, for 12 minutes. Linux, fake keys. The research is in
`research/2026-11-24-every-agent-program`.

| Program | Gave up after | Then |
|---|---|---|
| Codex 0.172.0 | 15 s | sent the request again, five times, then fell back from its WebSocket to plain requests |
| Gemini CLI 0.71.1 | 60 s | sent it again, five times, then stopped with an error at 327 s |
| Qwen Code 0.28.4 | 120 s | sent it again, six times in 12 minutes |
| Goose 1.60.2 | 600 s | sent it again |
| Aider 0.86.2 | 600 s | sent it again |
| OpenCode 1.19.6 | never closed it | new requests on other connections at 303 s and 608 s |
| Claude Code 2.1.331 | never closed it | one more model request on another connection at 183 s |

This measures the wait for a model's reply, not for a command, which the 90 seconds were set
against. A hold on a model's reply, such as a tool call held while a precaution or you answer
(`classifier-wait-limit`), is cut by Codex at 15 seconds and by Gemini CLI at 60, each of which
then sends the whole request again. When the reply had already started before the hold, whether
the program counts its wait from the last byte was not tried.

## How long a held reply can last (24 November 2026)

An agent's part, 24 November 2026, from the research in `research/2026-11-24-agent-programs-follow-ups`.

The limits in the section above count from the request to the first byte. Once the status and
headers are sent, they no longer apply. After them, without anything more, Claude Code gave up at
180 s and Codex and OpenCode at 300 s, and each sent the whole request again. With a keep-alive byte
every 5 seconds (`ping` on Anthropic's format, a comment on Gemini's, an event on Codex's), every
program tried waited 300 s, and Codex 420 s, then took the reply.

So the 90 seconds are not bounded by the programs' waits on a model reply, as long as the boundary
sends the headers and a keep-alive while it holds. They are bounded by what the person waits for,
and by the command timeouts named above.
