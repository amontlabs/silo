## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- An agent fixes a flaw where it is, in the design, the tool or the dependency, or records its workaround as a decision. (`AGENTS.md`)
  - Recommendation: yes. A silent workaround costs tokens on every later run and hides the flaw.
- msb-upstream's AGENTS.md has the orchestrator write an upstream pull request's description; instead, the owner writes it. (`AGENTS.md`)
  - Recommendation: talk it through. It reads as a trusted contributor's work, but every upstream fix then waits on the owner.
- Reports and patches to Zig are written by the owner without agent help, as Zig requires, or Silo keeps a local patch instead. (`AGENTS.md`)
  - Recommendation: yes. Zig refuses contributions written with an LLM.
- A bug in a dependency is checked against its spec or tests, then gets a minimal reproduction, an upstream report and a patch in Silo's copy. (`AGENTS.md`)
  - Recommendation: yes. One path for every dependency keeps a bug from being quietly routed around.
- When a live check fails for an unclear reason, the worker's first task is a minimal reproduction, before any fix. (`.atelier/rules/WORKER.md`)
  - Recommendation: yes. A model asked to fix a failure may fix an unrelated bug and believe it was the cause.
- A bug hunt targets one class of bug, starting with the parsing of untrusted input and integer overflow, with a harness that records traces. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. An agent told only to find bugs reports noise.
- A bug hunt and a security review of the engine run again with each new model. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. Security is not the current focus, and a hunt needs a reason.
- Bug hunts rotate their angle: each feature, each file, each commit, the tests' blind spots, and old logs for failures a retry hid. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. It is standing process, and the rule against retries already treats a hidden failure as a defect.
