# The chain

```text
computer
  run              one launch of an agent program, and every process descended from it
    conversation   a sequence of turns with one model
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
| tool call → the processes it started | Time. Ambiguous only when one process runs several tool calls at the same instant | An opinion |

Reading tool calls from model traffic is in the part `traffic`.

## The record's format

An agent's part, 25 November 2026, from [the record's format](../../../research/2026-11-25-record-format/format.md). The format
keeps a tool call's command text in its model-call line, up to 1 KiB, beside the tool call's id.
The challenge above (match a tool call to the processes started in its window by its command text,
then take their descendants) reads that text, and a process report carries the command it ran, so
both sides of the match are in the record. Facts and reports of a computer are one chain in the
order the recorder wrote them, so a report points to a fact by its number in that chain, and an
opinion in the opinions chain points to both as `c1:<seq>`.
