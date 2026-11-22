# What Silo does not build

Each line is marked Notes in the design record. One part of the chat-window line is decided and
lives in `you-visit-a-computer`: "You talk to your agent in a terminal inside the computer, or
through a tool that connects in, such as T3 Code."

## Found by an audit, 22 November 2026

An agent's part. This says Silo "passes no messages between agents" and "builds no chat window". The
agent in main sends `handoff.md` to the agent in a fork with `files-between-computers`, and you send
comments to an agent with `comments-to-the-agent`. The audit proposes: "It does no orchestration. A
file crosses between two computers only by a rule you set; Silo carries nothing else between agents.
It builds no window for talking to an agent: an ask, and a comment on what a turn changed, are the
two things you send from Silo." Otherwise `comments-to-the-agent` is dropped.
