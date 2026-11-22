# Switching several rules on at once

## The choice

An agent's part, 22 November 2026. For web work a computer needs the npm registry and github.com
allowed, and pushes held. You make a new Linux computer and set the same three rules again.

The options:

- Named sets (recommended). You switch "web work" on in one step, on any computer. It costs a step
  that can grant several things at once, which is why Silo shows what a set grants first.
- Rules one at a time. Every rule on every computer is set by hand. It costs setup each time, and
  people who find limits tedious leave the network open instead.

Recommended: named sets. Firewalls have profiles and agent programs have permission modes for the
same reason, and since rules have no order a set cannot hide a surprise in how it combines.

## Notes

A proposal the design relies on, not yet decided. What Silo
shows before a set is switched on is a separate proposal: `rule-set-shows-grants`.
