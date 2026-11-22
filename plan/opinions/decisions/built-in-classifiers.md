# Built-in classifiers

## For the owner

An agent's part, 22 November 2026. You open Now after lunch. Codex in main shows as stuck, and a
Claude Code run shows as running the same failing test again and again. Before you drop the fork
try-bun, Silo lists /work/app/.env as only here. Each of these is a judgement Silo makes itself, and
this entry is the list of them.

Four features build these judgements, and each asks you whether to build its part:
`which-run-did-it`, `how-a-path-comes-back`, `kind-of-each-call` and `how-each-run-is-going`. A yes
here commits to what they leave out: these judgements are Silo's own and work with nothing to
install, the list is the whole set for now, and Now rests on it for the state of each run. A no to
one of the four later takes its items off this list.

The options:

- These seven (recommended). Which run each process and each conversation belongs to; how each path
  would come back; the kind of each request; the state of each run, its stretches and its repeats;
  and which run left each file on the disk. It costs Silo the upkeep of seven judgements that are
  sometimes wrong, each shown as Silo's opinion.
- None, left to extensions people add. It costs the four features and the state of each run in Now.
  Each judgement would also need a computer of its own, which holds memory on your Mac.

Recommended: these seven. They answer what people ask first of a computer several agents share, and
only Silo sees every run from outside.

## Notes

The list, as put forward:

1. **Runs, and the run of each conversation.** The joins of the chain above that the boundary does
   not see. A conversation itself is read from model traffic at the boundary.
2. **How a path would come back**: by a pull, by a rebuild, or by nothing ("only here").
   Also a coarse kind: code, build output, dependency, cache, agent data, scratch, log,
   configuration, credential-like.
3. **Kind of call.** Model, package registry, Git, an outside action (a call that changes
   something elsewhere), other. Whether a host is seen for the first time is not here: it is a
   count of facts, as good as a fact.
4. **State of a run.** Working, waiting on the model, waiting on a command, idle, or stuck: no
   model call and no process activity, with a process that sleeps or waits for input.
5. **Stretches of a run.** Orienting, thrashing or progress, from exit codes and what was read
   and written. This item comes from the plan's notes before 18 November 2026.
6. **Repeats.** The same command, the same install, the same content entering context again.
7. **Owner of leftovers.** The last run that wrote a path, and whether that run is still alive.
8. **Heat.** Paths written by many runs. Agreed, for the map only: `heat-is-built-in`.

"The chain" is the chain of run, conversation, turn, tool call and trace, in the part `record`.
