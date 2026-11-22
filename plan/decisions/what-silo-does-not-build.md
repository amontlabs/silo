# What Silo does not build

Each line is marked Notes in the design record. One part of the chat-window line is proposed
in `computer-reached-over-ssh`, which has no verdict yet: you work in a computer through a terminal
over SSH, your editor, or a tool that connects in, such as T3 Code.

## The choice

An agent's part, 22 November 2026. The agent in main writes a handoff note and sends it to the agent
in a fork with Silo's tool for files. You also type a comment on a change, and the agent receives
it. This entry said that Silo passes no messages between agents and builds no chat window.

The options:

- Name the two exceptions (recommended). Silo does no orchestration. A file crosses between two
  computers only by a rule you set. From Silo you send an agent only an answer to its question and a
  comment on what a turn changed. It costs a strict line, since a file can carry a message. It also
  costs a second place to talk to agents beside their own apps, and polarzero said on 21 November that
  working with them is the client app's job.
- Name files only. A file crosses by your rule, and from Silo you send an agent only an answer to
  its question. It costs comments, which `comments-to-the-agent` recommends leaving out.
- Keep it strict. Silo stays out of every exchange. It costs comments to the agent and the tool that
  sends files between computers, which are both dropped.
- Drop the limit. The line on messages and on a chat window is removed. It costs the limit that
  keeps Silo from growing into a place where you talk to agents.

Recommended: name the two exceptions. Both features are useful and small. Naming them keeps Silo
from sliding into a chat app.

Whether an integration kept in Silo's own fork of T3 Code is an exception is asked elsewhere.
The question is put to polarzero in `t3-code-integration`, under "The choice".

## Found by an audit, 22 November 2026

An agent's part. This says Silo "passes no messages between agents" and "builds no chat window". The
agent in main sends `handoff.md` to the agent in a fork with `files-between-computers`, and you send
comments to an agent with `comments-to-the-agent`. The audit proposes: "It does no orchestration. A
file crosses between two computers only by a rule you set; Silo carries nothing else between agents.
It builds no window for talking to an agent: an ask, and a comment on what a turn changed, are the
two things you send from Silo." Otherwise `comments-to-the-agent` is dropped.
