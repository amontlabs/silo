## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Fork, start, diff, merge and an idle computer's memory each get a budget, written before their code as a p99 and a worst case.
  - Recommendation: yes. Speed decides whether the loop gets used, and an average hides the slow cases a screen adds up.
- Forking, diffing, merging and opening the app are at least as fast as creating a git worktree or starting a container.
  - Recommendation: talk it through. A fork boots a whole VM, so matching a container may need memory checkpoints early; the bar sets how much goes into speed.
- Objection to the build order: a way to see a browser running inside a computer moves into the starter, since people ask for it first.
  - Recommendation: talk it through. working-recipe already asks for GUI automation in main, which needs a desktop; ports already cover development servers.
- Silo ships only what the owner and the agents building it use themselves.
  - Recommendation: talk it through. It is a good filter for the starter, but first-user plans team features the owner will not use alone.
- Silo opens to other users only when it works without them reporting bugs or waiting for fixes.
  - Recommendation: talk it through. Taken literally no release meets it; the working bar is the starter used daily without workarounds.
- A risky feature ships only when it works on every setup Silo supports; until then Silo ships without it and says it is coming.
  - Recommendation: yes. A feature that works on some Macs costs more trust than it earns.
- A task's usable result comes first; checks and reviews run after it and can be stopped.
  - Recommendation: yes. The diff can be read at once, and whether a merge waits for the checks is up to its rule.
- Before 1.0, a changed interface replaces the old one at once with an automatic migration, and two versions never run side by side.
  - Recommendation: yes. With one user a clean break is cheap, and two versions would double every test.
