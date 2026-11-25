# How long may a hold last

## The choice

An agent's part, 25 November 2026, from the challenges of #350, #355 and #369, as #364 reconciles them. The
earlier answer rested on Claude Code ending a command at two minutes. Claude Code 2.1.331 moves a
command to the background at 2 minutes and stops it at 12, and Codex 0.172 kept one for 20 minutes
(#347, #355). What gives up first is the tool, not the agent program.

The case: at 02:00 an agent runs `aws s3 rm` on a bucket a rule holds, and another runs `git push` to
a branch a rule holds. boto3 closes its connection at 60 seconds and sends the request again, three
times. git waits as long as it is held.

The options:

- A length per client, and model replies kept alive (recommended). The rule for a host carries a
  length below its client's timeout: under 60 seconds for `aws`, up to 10 minutes for `git push`,
  90 seconds when nothing is known. A held model reply gets its headers at once and a keep-alive
  every 5 seconds (`ping` on Anthropic's Messages, a comment on Gemini's stream, an event Codex
  ignores on OpenAI's Responses), and every program tried then waited 300 seconds or more (#369).
  It costs a length per known client, kept with the rule, and a keep-alive chosen per format, since
  an unknown event broke OpenCode and Cline.
- 10 minutes for every hold (#355). It costs the tools that give up sooner: `aws` closes at 60
  seconds, Silo's words go to a closed connection, and boto3's three resends each join the ask.
- 90 seconds for every hold, as the earlier answer says (#350). It cuts a `git push` hold short for
  no reason, and a model reply held 90 seconds when no program needed it cut.

Recommended: a length per client, with model replies kept alive. Since an ask now outlives its hold
(`hold-sentence`), a hold no longer has to last until you wake up; it only has to end before the
program stops listening, which is a number per client.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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

## How long each program and tool waits (24 November 2026)

An agent's part, 24 November 2026. Measured on Linux, Claude Code 2.1.331 and Codex 0.172.0, with
a fake model that makes one shell call to a host that holds each request and never answers. The
scripts are in the project's files, under `research/2026-11-24-holds/`.

The choice above rests on "the agent program ends any command that runs for more than two
minutes". Neither program does that today:

- **Claude Code** waits 120 seconds on a command, then moves it to the background, where it keeps
  running for up to 10 more minutes. The model gets "moved to the background" as the call's result
  and goes on. When the command ends, Claude Code sends its output to the model as a new message,
  also in `claude -p`. A hold of 150 seconds ended with the model reading Silo's words, 30 seconds
  after the command had gone to the background.
- **Codex** returns from a command after at most 30 seconds, whatever the model asks, and keeps
  the command running. The model then waits on it with `write_stdin`, up to 300 seconds per wait.
  The command lives as long as the model keeps waiting, and dies when the turn ends. A hold of 200
  seconds ended with the model reading Silo's words, on its second wait.

So the agent program is not what gives up first. The tool inside the command is:

| Tool | Waits for a reply | Then |
| --- | --- | --- |
| pip 24.0 | 15 s | tries again, 6 tries in all, over 98 s |
| node `fetch` (undici) | 300 s | fails |
| npm 10.9.9 | 300 s | tries again twice, 10 s and 60 s later |
| curl 8.5, git 2.43, wget 1.21, Python `urllib` | no limit seen | still waiting at 900 s, where the test stopped them |

With a hold of 90 seconds:

- curl, git and plain Python wait it out, in both programs. The agent reads Silo's words.
- npm and node wait it out too.
- **pip never does.** It gives up at 15 seconds and sends the same request again, six times. Each
  try is a new request to Silo. If each one starts a hold of its own, you get six asks in 98
  seconds, and a yes to any of them lets nothing through, because pip has already closed that
  request.

So 90 seconds is short enough for both programs and long enough for most tools. What the measure
adds is the case of a tool that gives up and tries again on its own. A proposal for it: while an
ask is open, a new request from the same program, to the same host, with the same method and
path, joins that ask instead of opening a new one. A yes then lets the next such request through,
and a no or the limit refuses each of them at once. Whether a yes should also cover a request that
arrives after the first one closed is for the owner: it is what makes pip work, and it is a yes
that applies to a request you did not see.

## Holding an upload, and boto3 (24 November 2026)

An agent's part, 24 November 2026. The table above measured waiting for a reply. An upload is
held earlier: Silo reads the request's headers, knows the host, method and path, and stops reading
the body until you answer. Tried with a 20 MB PUT and a hold of 150 s:

- **curl 8.5 and Python `requests` 2.34 waited it out.** curl asked to continue
  (`Expect: 100-continue`), got no answer, began sending, and its sending simply blocked until
  Silo read again. `requests` did the same without asking. Both finished with 200 at 150 s.
- **boto3 1.44 did not.** It asked to continue, waited 60 s, closed the connection and sent the
  whole request again, three times, and failed at 184 s with "Connection was closed before we
  received a valid response". boto3's default wait for each step is 60 s (`read_timeout`, which
  applies to downloads too; not tried), and it retries on its own. So an agent's S3 upload or download held past 60 s becomes three requests
  and, unless they join one ask as proposed above, three asks.

So boto3 joins pip as a tool that gives up first: holding a body costs nothing by itself, and the
limit is each tool's own wait. The script is in the project files, `research/2026-11-24-holds/uploads`.

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

## What the phone changes (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-ask-reaches-a-phone/`. With the person away and awake, a model of the
time to answer on a phone gives these shares of asks answered while the request still waits: 0 to
1% for pip's 12 seconds, 2 to 10% for 50 seconds, 7 to 18% for 90 seconds, 62 to 68% for 10
minutes. Away from the Mac, the length chosen here decides little; the late yes of `hold-sentence`
serves most answers. The lengths per client stand: they keep Silo's words reaching a program that
still listens.
