# Built-in classifiers

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
