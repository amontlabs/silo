## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- AGENTS.md picks agents by role, never by provider; instead, designs and risky changes are reviewed by two model families. (`AGENTS.md`)
  - Recommendation: talk it through. Each family finds what the other misses, but the rule ties the pipeline to the models behind roles.
- After three rejected rounds or failed tries, the item stops until its brief, what the agent could see, or its reference code is fixed. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. Rule 3 has no cap on rounds today.
- ORCHESTRATOR.md rule 3 skips review for a mechanical change; instead, every change gets a review by a fresh session before the owner looks. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. A mechanical change gives a reviewer nothing to find, and the gate runs the whole check anyway.
- ORCHESTRATOR.md rule 3 stops when one round finds nothing critical or high; instead, review stops only after three clean rounds from every reviewer. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. It triples the cost of every review, and rule 3 already stops on a stated rule.
- A brief to a reviewer or to judge states the work and the criteria, and never the author's view or the answer hoped for. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. A reviewer told the hoped-for answer tends to give it.
- Before the orchestrator repeats a worker's reason for deleting or changing something, it checks the reason against the code. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. A wrong reason repeated to the owner becomes a fact in the plan.
- A reviewer flags what it is unsure of, and the owner reads those flags. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. The decision log already marks what the owner should look at, and doubt from every review would land on the owner.
- The orchestrator counts each reviewer role's false findings. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. It is bookkeeping for a choice of models the owner makes outside the repository.
