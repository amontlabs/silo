# Can Git history inside a computer serve as the record

## For the owner

An agent's part, 22 November 2026. An agent in main deletes three failing tests, commits the
deletion as "fix flaky tests", then rewrites the last five commits with `git rebase`.

The options:

- No, the record is Silo's own (recommended). The record shows the three test files deleted, as
  facts seen from outside main, whatever Git says afterwards. A commit message is at most something
  main told Silo, shown as unverified. A classifier may read Git to give its opinion. It costs the
  Git view in the record: Silo does not show the work as commits by itself, and a commit message is
  never more than unverified.
- Git history as the record of code. Silo shows the commits and their messages as what happened. It
  costs the record itself: an agent with full access rewrites Git history with one command, and the
  record then says what the agent wants.

Recommended: no, the record is Silo's own. The record is there for the case where an agent goes
wrong, and that agent can rewrite Git history.
