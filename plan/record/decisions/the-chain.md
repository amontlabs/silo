# The chain

```text
computer
  run              one launch of an agent program, and every process descended from it
    conversation   a sequence of turns, in which the model may change
      turn         one reply of the model
        tool call  one thing the reply asked for
          trace    what it left: processes, file changes, connections
```

Where each link comes from. The table stands under the chain and carries no mark of its own.

| Link | Known from | What it is |
|---|---|---|
| tool call → turn → conversation | Model traffic at the boundary | A fact, when the traffic can be read |
| trace → process | Inside | A report |
| process → run | The process tree, which is a report, and a data list that recognises agent programs, which is a judgement | An opinion |
| conversation → run | Which process owned the connection, which is a report, then the run of that process | An opinion |
| tool call → the processes it started | Time and the tool call's command text: in the window from the reply to the request that carries its result, the earliest process whose command holds the tool call's command, then every process descended from it | An opinion |

Reading tool calls from model traffic is in the part `traffic`.

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-morning-review`. A challenge to one row of the table: "tool call →
the processes it started", known from time. On a real night that got it wrong for most tool
calls of Claude Code, not only at the same instant: Claude Code runs git about six times a turn
on its own, in the same window as the command the model asked for. The case: claude #2 asked for
`rm -rf ~/work/shared-fixtures`, and by time alone its tool call took a `git status` Claude Code
ran for itself, started a few milliseconds earlier. What worked for every tool call that night:
in the window from the reply to the request that carries its result, the earliest process whose
command holds the tool call's command text, and every process descended from it. In its place:
"Time and the command text of the tool call, then descent", still an opinion.

## Two lines of these notes changed, 25 November 2026

An agent's part. The answer polarzero took does not change: facts and reports hang on one chain.
Two lines of the agent's notes above changed.

- The row "tool call → the processes it started" said "Time. Ambiguous only when one process runs
  several tool calls at the same instant". #371 found it wrong for most tool calls of a real night:
  Claude Code runs git about six times a turn on its own, in the same window as the command the
  model asked for, and by time alone claude #2's `rm -rf ~/work/shared-fixtures` took a `git status`
  Claude Code ran a few milliseconds earlier. Matching by the command text, then descent, matched
  every tool call that night. The link stays an opinion.
- The line of the conversation follows the word `conversation` as `recognising-a-conversation`
  proposes to set it. Until that entry is taken, the word in force is "a sequence of turns with one
  model".

## The record's format

An agent's part, 25 November 2026, from [the record's format](../../../research/2026-11-25-record-format/format.md). The format
keeps a tool call's command text in its model-call line, up to 1 KiB, beside the tool call's id.
The challenge above (match a tool call to the processes started in its window by its command text,
then take their descendants) reads that text, and a process report carries the command it ran, so
both sides of the match are in the record. Facts and reports of a computer are one chain in the
order the recorder wrote them, so a report points to a fact by its number in that chain, and an
opinion in the opinions chain points to both as `c1:<seq>`.
