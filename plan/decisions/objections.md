# Objections to expect

## The choice

An agent's part, 22 November 2026. Silo is posted on Hacker News, and the first reply is "A virtual
machine per agent is too heavy."

The options:

- Seven prepared answers (recommended). Each objection gets one or two factual sentences, the same
  on the site, from you and from agents. Here: one main holds all your agents, and a fork is for the
  exception. It costs upkeep: each answer is a claim about Silo or about others, and has to be
  checked again when either changes.
- No prepared answers. Each objection is answered as it comes. It costs answers that differ from one
  place to the next, and the weak ones are found in public.

Recommended: the seven answers. The objections are known from the research, and a short true answer
is better found now than under a post.

## Notes

Proposed in the design record:

| Objection | Answer |
|---|---|
| "I run agents with every check off and take backups." | A backup returns files. It does not show what was sent out, what was spent, or what was done elsewhere. |
| "A virtual machine per agent is too heavy." | One main holds all your agents. A fork is for the exception. |
| "I can build this myself." | Some do, repeatedly. Silo is one command, and the parts they do not build: the record, and going back for the whole machine. |
| "Session timelines already exist." | Those are an agent program's view of itself. Silo's is the computer's view, under any program. |
| "Watching agents is easy to do yourself." | For logs, yes. Not for the disk, the network and the past of a whole machine. |
| "The built-in sandbox of my agent program is enough." | Its rules live next to the agent, and the agent gets around them. |
| "Sandbox products already exist." | They give a boundary. They do not say what happened inside or take you back. |

The answer of this decision is worded by whoever wrote the plan: the design record has only the
table.

## Checked against the market, 24 November 2026

An agent's part. Agents read about 70 products for agents from their own pages. Two answers were
reworded to stay true:

- "Sandbox products already exist." The old answer said they do not take you back. Several
  restore or fork a machine, and one takes disk checkpoints on its own. None found records what
  happened inside from outside the machine, and none found enforces a budget on model spend for any
  agent program. The answer now says that they do not stop what the agent spends.
- "The built-in sandbox of my agent program is enough." A public case of March 2026 backs the
  answer: an agent got past a deny rule on `npx` by calling it through `/proc/self/root`, and when
  the sandbox stopped that, it asked for approval to run outside the sandbox.
