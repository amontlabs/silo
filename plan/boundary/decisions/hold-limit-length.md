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
