# Silo in one sentence

The wording is proposed. The design behind it is agreed.

The design record follows the sentence with: "The computer is a virtual machine on your Mac. It
runs macOS or Linux. Nothing of yours is inside unless you put it there."

And gives the reason it can be said: only agents work there, so a complete account is possible
(`only-agents-work-there`).

## The choice

An agent's part, 22 November 2026. An agent broke something at 14:07. The saved moments are 14:00
and 14:10, and the home page had said "any moment". The sentence also says that Silo records
everything, though it records no file reads, and the reason given for it says that only agents work
there, though you visit.

The options:

- Say what is true (recommended). Silo records what they do there and takes you back to a saved
  moment. The reason becomes "only agents work there; you visit, and what you do is told apart". It
  costs a bolder first sentence.
- Make the bold one true. The sentence stays, and Silo must record every file read and restore to
  any second. It costs a slower, heavier Silo and two decisions polarzero took, `file-reads` and
  `automatic-checkpoints`.

Recommended: say what is true. The first sentence people read should be one Silo keeps.

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
