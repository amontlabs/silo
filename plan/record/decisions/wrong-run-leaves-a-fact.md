# Attributing work to the wrong run

## The choice

An agent's part, 22 November 2026. Claude Code and Codex work in main, each in a run of its own.
Codex deletes `src/auth.rs`, but first uses sudo to move itself out of its run, so the deletion
looks like nobody's, or like Claude Code's.

The options:

- Unverified, with what is unaccounted for shown (recommended). Which run did something is always
  shown as unverified, because only reports from inside main say it. The deletion itself is a fact.
  When no run's reports account for it, it is listed as unattributed, and a report the facts
  contradict is marked as a mismatch. A hostile test lists each way an agent can move its work to
  another run, and a trial looks for a way to make that impossible in Linux computers. It costs
  certainty: an agent working against you can make its work look like another run's.
- Only per computer. Silo says which computer did something, which it sees from outside, and nothing
  about the runs inside one computer. It costs the view of which agent wrote what in a main shared
  by several agents.

Recommended: unverified, with what is unaccounted for shown. An agent that hides its work this way
is rare, and when you need to be sure, an agent can work in a fork of its own, where what it does is
seen from outside.

## Notes

To find out, per system. The target: attributing work to the wrong run should be impossible
without an act that itself leaves a fact. Work that cannot be attributed is shown as unattributed.
