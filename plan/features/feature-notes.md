## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- ORCHESTRATOR.md has one session carry a feature from discussion to built; instead, the discussion ends with the notes written, and a fresh session builds from them. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. A fresh session tests the notes as a brief, but whatever was not written down is lost.
- A check fails when a started feature's notes state no property, nothing that must always hold about it. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. It follows properties-checked, and a feature with nothing to state is not understood yet.
- A check fails when a started feature's notes still hold a TBD or an open question. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. It makes rule 2b's 'decide before the worker starts' mechanical.
- A feature's notes name every dependency it adds, agreed with the owner before `plan start`, and a worker adds none on its own. (`AGENTS.md`)
  - Recommendation: yes. Dependencies are hard to undo, and the discussion is when the owner is there to agree.
- Before `plan start`, a reviewer reads the feature's notes for contradictions and for parts that can be removed. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. It catches what rule 2b says code review misses, but it is one more round per feature.
- An engine feature's notes begin with its inputs, its outputs and its states. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. It matches the engine's data-first design and costs three headings.
- Before `plan finish`, the feature's notes are brought in line with what was built and what the build taught. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. The notes are the next feature's decision log, and stale-docs-are-deleted covers only docs.
- A feature's notes keep each rejected alternative with the reason it lost. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. The same choice is then not argued again, which is why decisions are written down.
- A decision put to the owner comes with a recommendation, the alternatives and what similar tools do. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. Sessions that talk with the owner already ask this way, from saved preferences.
- A feature's notes have fixed headings: acceptance, error cases, permissions, limits, logs, and how it is verified. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. Rule 2b's `decisions=` already covers every entry point, and a form gets filled whether it applies or not.
