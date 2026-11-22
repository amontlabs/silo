# Who runs a task's checks

## For the owner

An agent's part, 22 November 2026. An agent in a fork fixes a bug and says that all 214 tests pass.
Silo then runs `cargo test` itself on that commit, in a fresh computer the agent never worked in.
213 pass and one fails.

The options:

- Silo runs them (recommended). What you see is Silo's own run, not the agent's word, and no agent
  can change the command that checks. It costs a fresh computer and the time of the tests at each
  check. The code under test still runs in that computer, so a commit written to fake a pass can do
  it. A change to the tests shows in the receipt.
- The agent runs them and reports. You see the agent's run and its output. It costs your trust in
  its word, since agents say the tests pass when they ran only some of them, or ran them before
  their last change.

Recommended: Silo runs them. A receipt exists to check what an agent claims, and only a run it had
no hand in can do that.

## Notes

A proposal. Beside it, the design record has a line marked Notes: "checks that decide are
defined outside the fork and run by the engine".
