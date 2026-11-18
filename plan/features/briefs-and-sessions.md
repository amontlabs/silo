## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- WORKER.md and ORCHESTRATOR.md rule 2b settle open points before the worker starts; instead, a worker proposes a plan and asks what is unclear before it starts. (`.atelier/rules/WORKER.md`)
  - Recommendation: no. Nobody is there to answer during the build, which is why rule 2b moved the asking earlier.
- ORCHESTRATOR.md never names a model in a task; instead, briefs are written per model, short for a strong one and fuller for a cheap one. (`AGENTS.md`)
  - Recommendation: no. Roles hide the model so that one brief keeps working when a role's model changes.
- AGENTS.md has agents chosen by role name; instead, the rules state the cost goal once, the orchestrator picks any model, and the result is measured. (`AGENTS.md`)
  - Recommendation: no. Roles let the owner change a model in one place for every task.
- AGENTS.md uses `cheap` only when the owner asks; instead, cheap models work by default, and a stronger one checks them until they are trusted. (`AGENTS.md`)
  - Recommendation: no. The owner chose cheap-on-request on purpose, and a cheap model's misses cost review rounds.
- The state of long work lives in files, and a session starts fresh before its context is near full. (`AGENTS.md`)
  - Recommendation: yes. The owner's own instructions say this for Claude only, and the repository says it only for a worker's fixes.
- An item is sized to fit one worker's context window, and Zig items, which take about twice the context, are cut smaller. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. A worker that compacts mid-item loses the brief's detail.
- A handover note says where the work is, where it should end, and what was tried and ruled out. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. ORCHESTRATOR.md asks for a short handover and does not say what goes in it.
- A brief quotes or links the owner's own words, and never passes on another agent's paraphrase of them. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. Each paraphrase drifts, and the owner's words are in the plan to link.
- A worker's report ends with one line on what it lacked and what would have made the task easier. (`.atelier/rules/WORKER.md`)
  - Recommendation: yes. It gives a new rule the failure it must cite, for one line of report.
- When it is unclear whether a task needs `build` or `build-hard`, the orchestrator picks `build-hard`. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. A weak build costs review rounds, and a strong one costs tokens on every borderline item.
- Every brief is read by a second agent before the worker starts. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. Rule 2b already sends open decisions to judge, and this adds an agent to every item.
- Each piece of work records the role and the model that produced it, and a run whose model changed mid-session is flagged. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. atelier knows the model behind each role, so this belongs in atelier and not in rules agents follow.
