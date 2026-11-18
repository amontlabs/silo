## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Before a part is designed, a research step finds the best existing implementation, to reuse or learn from, and records it in the feature's notes. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. An agent otherwise builds from memory what someone has already done better.
- A Zig upgrade is its own item, run while no other item is open, and starts with a migration guide for the new release. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. A Zig release can cost days of migration across the whole tree.
- A reference's licence is checked before agents copy from it. (`AGENTS.md`)
  - Recommendation: yes. The repository is public and MIT-licensed, and copied code carries its own licence.
- A utility an agent can write in one prompt is written in the tree, as is the little Silo uses of a small package. (`AGENTS.md`)
  - Recommendation: yes. Each import is one more dependency to vendor, pin and trust.
- Silo's own build installs only package versions published at least a set number of days ago. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. Cheap protection against a poisoned release; the same limit for recipes is the recipes feature's call.
- One file lists the Zig compiler bugs met, each with the versions it affects and its workaround. (`AGENTS.md`)
  - Recommendation: yes. An agent that meets a known compiler bug otherwise spends hours on it.
- One file lists every dependency with a line on why Silo needs it. (`AGENTS.md`)
  - Recommendation: no. The notes of the feature that added it already say why, and the vendored folder lists them all.
