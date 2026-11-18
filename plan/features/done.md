## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Does a feature carry a mark beyond `built`: 'hardened' (it survives a restart, a reconnect and an app out of step), 'used by the owner', or neither? (`plan/decisions.json`)
  - Recommendation: talk it through. A used-by-owner mark is one field; hardening is better written as tests in each feature.
- A check fails when a built feature names no end-to-end test. (a check in `tools/ci/check.sh`)
  - Recommendation: talk it through. It makes 'proven' visible, but adds a field to every feature file and a test name to keep current.
- A worker never reports a pass on a mock as a pass on the real integration. (`.atelier/rules/WORKER.md`)
  - Recommendation: yes. A large green suite has missed memory leaks and a broken first run before.
- A feature lands behind a flag until the owner turns it on. (`AGENTS.md`)
  - Recommendation: no. Features merge one at a time and only when proven, and a flag is a code path nobody asked for.
