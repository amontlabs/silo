## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- WORKER.md and ORCHESTRATOR.md rule 2b keep older saved data loading; instead, until the first release, no migration or compatibility code is written, and review flags any. (`.atelier/rules/WORKER.md`)
  - Recommendation: talk it through. Before the starter there is no data worth keeping, but once the owner lives in main, real data is at stake.
- Each call in the engine's definitions names the feature it belongs to, and a check fails when that feature is not taken. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. It makes 'nothing is built without a feature the owner took' mechanical for the engine.
- A review sends back a change that could lose half its lines without losing behaviour: dead code, useless tests, unasked features, a re-implemented tool. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. Agents add more than asked, and the old Silo reached 700k lines.
- A configuration option with no known user is removed. (`AGENTS.md`)
  - Recommendation: yes. Each option doubles the paths to test.
- An abstraction is added when its second use arrives. (`AGENTS.md`)
  - Recommendation: yes. Agents abstract early, and a wrong abstraction is harder to remove than a copy.
- Code the owner does not want is deleted, never refactored into shape. (`AGENTS.md`)
  - Recommendation: yes. Refactoring unwanted code spends tokens to keep what nobody asked for.
- When a pattern changes, every use moves in the same item, so old and new never sit side by side. (`AGENTS.md`)
  - Recommendation: talk it through. Agents copy whatever they find, but moving every use at once makes items large.
