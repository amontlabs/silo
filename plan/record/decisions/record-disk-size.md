# The record's size on disk

## The choice

An agent's part, 22 November 2026. Main runs agents all day: builds, tests, three long
conversations, and one agent that drives the screen for an hour. Rough estimates say the lines of
one busy computer take tens of megabytes a day, and a screen recording far more.

The options:

- Measure first (recommended). Several days of real work in main, and one run made to flood the
  record, give the bytes per day of each kind of line, with message text and recordings counted
  apart. The default size limit, and the share at which Silo warns you, are set from those numbers
  and come to you then. It costs a record with no default limit until the measures exist.
- A default from the estimates now. The limit is set today from the rough figures. It costs a guess
  on your disk: recordings swing the figures most, so the limit can remove recordings and message
  text too early, or let the record fill the disk.

Recommended: measure first. The default limit decides what gets removed from your record, so it
should rest on what real work writes.

## Notes

To find out, per kind of fact and report.

Rough estimates for one busy computer per day: tens of megabytes for process, file and connection
lines once merged and compressed; a few megabytes per long conversation; recordings are the heavy
item and are kept only while an agent drives the screen.

## One real conversation, measured (24 November 2026)

An agent's part, 24 November 2026. The transcript of one real Claude Code session was measured:
the agent that wrote these notes, working for about two hours. It ran experiments, wrote code and
edited the plan, with many long command outputs. Only sizes were computed. The text was not kept.

| | |
|---|---|
| Model requests | 134 |
| Messages (the user's, the model's, tool calls and results) | 436 |
| Every request kept whole, as the provider receives it | about 73 MB |
| Each message kept once, as `store-messages-once` says | 0.95 MB |
| The same, compressed with zlib | 0.43 MB, about 0.2 MB an hour |

- The whole requests were counted from the transcript, each holding every message before it, plus
  50 KB for the system prompt and tool definitions, the size seen in the fake runs of
  `denial-per-program`. Kept once, those 50 KB count once.
- Kept whole, the requests are 77 times larger than the messages kept once, and the gap grows with
  the length of the conversation.
- Tool results and the user's messages were 36 % of the text. The largest single message was
  27 KB.

So the message text of a busy agent comes to about 0.2 MB an hour, compressed: a few megabytes for
a day of several agents. That agrees with the notes' rough "a few megabytes for a long
conversation" (`conversation-text-kept`). Recordings of the screen and the lines of the record
itself are not in this number.
