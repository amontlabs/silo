## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- The merge gate runs the whole check (AGENTS.md); instead, each engine directory has its own checks, and the gate runs only those of changed directories. (`AGENTS.md`)
  - Recommendation: no. The whole check on the merged tree is what catches one unit breaking another.
- A check prints nothing when clean and one line per problem. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. Agents read every line of output, and the owner's rules already ask for small output.
- Every generated file is checked in with its source and the one command that rebuilds it, and a check proves each is current. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. calls-and-events generates the clients and the docs, which otherwise drift without anyone seeing.
- Repository work goes through `plan`, `tools/ci/check.sh` and `zig build`, and a new need extends one of them instead of adding a script. (`AGENTS.md`)
  - Recommendation: yes. Scattered scripts are where agents start going around the tools.
- Repository scripts are written in Node.js, as `plan` is, with shell only for thin entry points such as `tools/ci/check.sh`. (`AGENTS.md`)
  - Recommendation: yes. AGENTS.md already pins Node.js 26, and `plan` is written in it.
- A check fails when code turns off a lint rule. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. Agents silence a rule more readily than they fix what it found.
- Lint tooling is chosen by how cheaply an agent can add a custom rule. (`AGENTS.md`)
  - Recommendation: yes. Repeated mistakes are meant to become checks, so adding one has to be cheap.
- A rule no lint can express may be checked by a model that reads the diff for it. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: no. A model gives different answers on the same tree, which the flaky-test rule treats as a defect.
- Checks also run in a commit hook. (a check in `tools/ci/check.sh`)
  - Recommendation: no. Workers commit often and run only the tests their change needs, and the gate runs everything.
- When an entry changes, a check lists the docs, tests and code that depend on it, and marks what was generated from it as stale. (a check in `tools/ci/check.sh`)
  - Recommendation: no. It is heavy tooling for a plan of plain files, and the generated parts already rebuild in the whole check.
- After a rename, a check searches the help text, the docs and the rules files for the old name. (a check in `tools/ci/check.sh`)
  - Recommendation: no. Renames are rare, and the item doing one can search once.
