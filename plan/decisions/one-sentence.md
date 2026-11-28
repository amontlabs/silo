# Silo in one sentence

The wording is proposed. The design behind it is agreed.

The design record follows the sentence with: "The computer is a virtual machine on your Mac. It
runs macOS or Linux. Nothing of yours is inside unless you put it there."

And gives the reason it can be said: only agents work there, so a complete account is possible
(`only-agents-work-there`).

## The choice

An agent's part, 27 November 2026. polarzero read the sentence of 24 November, which ended on
"send and spend", and said the description was "just a list that is not super attractive or
promising". polarzero then said what Silo is for:

> you have the vm, you expect everything that happens inside it to be hostile and untrustworthy
> and you observe as much as possible from outside the vm. [...] from outside it tracks LLM
> requests and responses, vm filesystem, etc, and compares with what's reported from inside the vm
> (the agent harness). Then you could flag if there is a discrepancy

The sentences so far name what Silo keeps: a computer, a record, a checkpoint. None says where
Silo stands, and that is what the rest follows from. An agent program's own rules and its own
log live inside the computer, where the agent can reach them, and Silo's live outside it.

A developer lets Claude Code run with every approval off. At the end it writes "All tests pass."
Silo read the model calls from outside, and the last tool call that ran the tests came back with
a failure. Silo shows the two side by side.

The options:

- Where Silo stands, then what it does from there (recommended). "Your agents work with full
  access in a virtual machine of their own. Silo sits outside it, where they cannot reach. It
  records what they run, change and send, compares what they say with what it saw, and can hold a
  request at the edge for your answer." It keeps full access, which this audience wants. Each of
  its three verbs is in the plan: the record (`what-the-record-holds`), a report compared with
  facts (`report-matched`, `own-account-as-a-check`), and a held request (`hold-limit`). It costs
  three sentences where there were two, and it leaves out the checkpoint, which `what-to-say` keeps.
- The sentence of 24 November. "Your agents work with full access in a virtual machine on your
  Mac. Silo records what they run, change, send and spend there, and takes it back to a
  checkpoint." It costs "spend", which is now the work of an extension
  (`spend-and-budgets-ship-as-an-extension`), and it says nothing of the outside.
- The outside and the checkpoint together. "Your agents work with full access in a virtual machine
  of their own. From outside it, Silo records what they do, compares it with what they say, holds
  what you told it to hold, and takes the machine back to a checkpoint." It costs a fourth verb in
  one sentence.

Recommended: where Silo stands. An agent program cannot say this of itself, since it runs inside.
"Hostile" stays out of the sentence: it is how Silo is designed (`security-framing`), and
`guides/writing.md` asks that the site not speak through fear.

"Can hold" and not "holds": a request waits only when a rule or a precaution you set holds it.
"For your answer" and not "until you answer": a hold has a limit and Silo ends it (`hold-limit`).

## The earlier choice, 24 November 2026

An agent's part, 24 November 2026. A developer opens the home page from a link. The first words
they read decide what they file Silo under. Many products for agents already describe themselves
as computers for agents, so a first sentence that says "a computer of their own" files Silo with
them before the record is mentioned.

The options:

- What they get, named (recommended). "Your agents work with full access in a virtual machine on
  your Mac. Silo records what they run, change, send and spend there, and takes it back to a
  checkpoint." It keeps full access, which is what this audience wants. It names four things a
  visitor can picture and check, each backed by the plan: what ran, changed and connected
  (`what-the-record-holds`), what was sent (`record-shows-what-was-sent`), what was spent (the part
  `traffic`) and the checkpoint (`automatic-checkpoints`). It costs "on your Mac", which leaves out
  the servers and the cloud of `where-it-runs`, and it drops "shows you what matters".
- The sentence as proposed on 22 November. "Agents work in a computer of their own. Silo records
  what they do there, shows you what matters, and takes you back to a saved moment." It is true. It
  costs a first sentence that many other products could say of themselves, and "shows you what
  matters" gives a visitor nothing to picture.
- The record first. "Silo records what your agents run, change, send and spend, under any agent
  program, in a virtual machine of their own that it can take back to a checkpoint." It leads with
  what no other product found does. It costs a long sentence, with the machine at its end.

Recommended: what they get, named. It says "any moment" and "everything" nowhere, as the audit of
22 November asked, and each of its four things holds in the plan.

When Silo runs on servers too, "on your Mac" becomes "on your Mac or your server", in a later edit.

## The earlier choice, 22 November 2026

An agent's part, 22 November 2026. An agent broke something at 14:07. The saved moments are 14:00
and 14:10, and the home page had said "any moment". The sentence also says that Silo records
everything, though it records no file reads, and the reason given for it says that only agents work
there, though you visit.

The options:

- Say what is true. Silo records what they do there and takes you back to a saved
  moment. The reason becomes "only agents work there; you visit, and what you do is told apart". It
  costs a bolder first sentence.
- Make the bold one true. The sentence stays, and Silo must record every file read and restore to
  any second. It costs a slower, heavier Silo and two decisions polarzero took, `file-reads` and
  `automatic-checkpoints`.

It recommended saying what is true: the first sentence people read should be one Silo keeps.

`what-to-say` and `only-agents-work-there` are reworded to match. The home page keeps its sentence
until you answer.

## Found by an audit, 22 November 2026

An agent's part. This says Silo "takes you back to any moment", and `what-to-say` says "You can go
back to any moment". `automatic-checkpoints`, taken, takes checkpoints at intervals: the agent broke
something at 14:07, checkpoints exist for 14:00 and 14:10, and 14:07 cannot be restored. The audit
proposes: "Agents work in a computer of their own. Silo records what they do there, shows you what
matters, and takes you back to a saved moment.", and "You can go back to any checkpoint, or continue
from it in a fork." in `what-to-say`.

## What each build lets the sentence say

An agent's part, 24 November 2026. The first build proposed in #348 has main, checkpoints taken
on their own, restore, and which files changed between two checkpoints. It records no processes and
reads no model traffic: those come in its second and third builds. A visitor reads "Silo records
what they run, change, send and spend there", installs Silo, and finds a list of changed files. The
first sentence they read is then one Silo does not keep yet.

Proposed: the sentence the home page shows follows what is built, and the recommended answer is
the one it ends on.

- With the first build: "Let your agents work with full access in a macOS virtual machine on your
  Mac. Silo takes checkpoints of the whole disk on its own, so anything they break, you can put
  back."
- Once processes and connections are recorded: "Your agents work with full access in a virtual
  machine on your Mac. Silo records what they run and change there and where they connect, and
  takes it back to a checkpoint."
- Once model calls are read: the recommended answer, with "send and spend".

The first build's sentence still says something few people can do today: going back that covers
what the agent did in the shell, in subagents and in ignored files, because the checkpoint copies
the disk. #350 ranks work destroyed by the agent as the most frequent serious pain.

## polarzero's words, 28 November 2026

Owner: "comparing what they say with what it saw is one main very strong use case. I'm not sure
it's the right way to frame as if it was the only thing it does."

Owner, on how he would present the first build: "if you're working with agents and if you're not
even using VMs you can install Silo and get basically exactly what you already have without any
friction, but additionally, you get a VM and you get excellent observability and logs". And: "it
costs nothing to install Silo and to use it, but it can prove to be extremely beneficial in the
future and you can also get your agent to build extensions using this observability". And: "You
don't need to have a use case right now."

An agent's part. The answer is rewritten around that. It says three things in his order: nothing
changes in how you work, you gain a virtual machine and a record made from outside, and the record
can be used now or later, by you, by an agent or by an extension. Comparing what an agent says with
what Silo saw is now one use among others (`said-compared-with-done`) and is no longer in the
sentence. The sentence says "record" where polarzero says observability, as `guides/writing.md`
asks. "Costs nothing" is not in it, because what Silo costs in memory and speed is not measured
yet (`silo-is-light`, `record-speed-cost`). It is still undecided.
