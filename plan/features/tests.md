## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- A check fails when a change deletes, skips or disables a test, or narrows a simulator's inputs, without naming the decision that allows it. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. Weakening a test is the easiest way for an agent to turn red into green.
- A worker shows each new test failing without its change, so a fixed bug keeps a test that turns red if the fix is reverted. (`.atelier/rules/WORKER.md`)
  - Recommendation: yes. It catches tests that pass whatever the code does.
- When the spec, a test and the code disagree, the spec wins over the test and the test over the code. (`AGENTS.md`)
  - Recommendation: yes. One line, and it tells an agent which of the three to change.
- Every test shows how the product is meant to be used, and a test that does not is removed. (`AGENTS.md`)
  - Recommendation: talk it through. Tests that read as usage guide later agents, but low-level tests of the core would go.
- Where a public test suite exists for something Silo implements or wraps, it runs against Silo in the whole check. (a check in `tools/ci/check.sh`)
  - Recommendation: talk it through. It is proof that someone else keeps current, but a big suite slows every merge.
- Before a driver is written, small tests pin the behaviour of the dependency it relies on, and stay in the tree. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. They catch an upstream change the day it lands and show agents how the dependency behaves.
- Before a large refactor, the worker adds tests of the current behaviour. (`.atelier/rules/WORKER.md`)
  - Recommendation: yes. Without them nothing proves the refactor changed nothing.
- Each user journey is written once, as a test that also serves as the user's documentation. (`AGENTS.md`)
  - Recommendation: no. Docs for people follow WRITING.md and tests follow the code, and one text rarely serves both.
