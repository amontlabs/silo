# Kinds of facts and reports

## The choice

An agent's part, 22 November 2026. An extension that follows your deployments wants to write
"deployed to production" into the record of main, as a kind of line of its own.

The options:

- Nine kinds, fixed (recommended). Control, process, file, connection, model call, screen, meter,
  gap and silence. What an extension has to say is an opinion, kept beside the record. It costs
  extensions a place of their own in the record.
- A list that extensions add to. The record also holds kinds that each extension invents. It costs
  every view, rule and receipt having to cope with kinds it does not know, and a record that no
  longer holds only what Silo saw or was told.

Recommended: nine kinds, fixed. The record is worth what it is because every line in it is a fact
or a report of a known kind.

## Notes

| Kind | What it says | Fact or report |
|---|---|---|
| control | A computer was created, started, paused, resumed, stopped or dropped. A checkpoint, a fork, a restore. A rule or a precaution was set. An ask was raised, and answered yes or no, or ended without your answer, with the reason it ended. A question was put to you and answered. An alert was raised. A classifier gave an opinion. A precaution acted. A secret was given or taken back. An extension was granted a tool. Each with who did it. An answer to an ask and a grant also say what Silo did and what came of it. | Fact |
| process | Started: command, parent, user. Exited: code, and totals of processor, memory and disk use. | Report |
| file | Created, changed, deleted or renamed: path and change in size. | Fact. Which process did it is a separate report that points to it. |
| connection | Host, port, bytes each way. Allowed, denied or held, and what decided it: a rule or a precaution with its version, your answer, Silo's limit on a hold, or no rule at all. For opened traffic: method, path, status and sizes. The secret used, by name. | Fact. Which process owned it is a separate report. |
| model call | Provider and login by name. Model asked and model that answered. Tokens in, out and cached, and cost. Conversation and turn. The tool calls the reply asked for, and the result sent back for each, which includes the output of commands. A refusal or a reroute. What the provider says is left of a limit. | Fact, when the traffic can be read. An agent program's own account of a call is a separate report. |
| screen | A stretch of recording while an agent drives the screen, and the input events. | Fact |
| meter | Disk, memory and processor use of a computer, sampled. | Fact |
| gap | A source lost facts or reports between two moments, with the count when known. | Fact; a count given by the inside is a report |
| silence | A source sent nothing for longer than its fixed interval, from when to when. It loses nothing by itself: the source's next line shows whether lines were lost, and a loss is a gap. | Fact |

A no, and an ask nobody answered, are kept as a yes is: they are part of what happened. One case: on
Monday at 15:10 you said yes to codex in main reaching api.stripe.com with the secret stripe-test,
and on Tuesday an ask from main ran out while you were away. A week later the record shows Monday's
yes, what passed and what Stripe answered, and Tuesday's ask, ended unanswered, with why. Keeping
only what you granted would cost telling "I said no" from "it never asked me".

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **The compiler lists what a new kind must touch.** A new variant must be handled everywhere it
  matters. Guillotine keeps written checklists; a `switch` with no `else` makes the compiler produce
  the list. Adding a kind of line, input or effect fails to compile until every place handles it. A
  written checklist covers only what the compiler cannot see, such as docs and the app.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/HARDFORKS.md?plain=1#L469))
- **Every entry of a table is tested.** Test every entry of a lookup table, and pin the number of
  variants of an enum that mirrors an outside list. A test walks every action and checks its kind is
  filed; the short fixed list of line kinds is pinned by count.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/opcodes/REVIEW_SUMMARY.md?plain=1#L105))
