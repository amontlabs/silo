## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Decision old-code-outside opens the old code only through a lookup agent; instead, agents read the old tree while they build, and reviews compare the engine with it. (`plan/decisions.json`)
  - Recommendation: no. Settled on 16 November so that the lessons cross over and the old patterns do not.
- A behaviour the old Silo had gets black-box tests that pass on the old code before the new code is built against them. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. Tests proven on a working system are worth a lot, but the old Silo has to be run for each.
- Before an engine feature is designed, a lookup agent writes into its notes the behaviour and invariants the old code had for it. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. It is the lookup old-code-outside allows, done once per feature.
- Code that replaces a dependency first matches the original's output byte for byte, and that comparison stays as a check. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: talk it through. It is strong proof for a port, but the plan does not yet say Silo replaces any dependency.
- Anything Silo re-implements from another project gets a written spec first, and a target that keeps changing is reused instead. (`AGENTS.md`)
  - Recommendation: yes. A port without a spec copies the original's bugs and misses its intent.
- A scheduled agent reads the upstream commits of each ported dependency and proposes what to carry over. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. It is standing process, and the byte comparison shows drift whenever the dependency is updated.
