# Teams

## The choice

An agent's part, 22 November 2026. Three developers share one main where agents work on the team's
migrations. Each of them sees its record and its screen, and any of them can pause it.

The options:

- Build it (recommended). Several people follow one main. It costs who may do what: each person
  needs their own access and their own say over pausing, granting and stopping, and the record must
  say who did each.
- Leave it out. Silo stays for one person. It costs the developer in a company, whose work is shared
  with a team.

Recommended: build it, in its later stage. You already decided that team features come later, on the
same product.

## What a team asks for, 24 November 2026

An agent's part, from research on teams of 5 to 50 that run agents
([`research/2026-11-24-small-teams`](../../../research/2026-11-24-small-teams/README.md)). It keeps
the recommendation and splits the work into entries that can be decided one by one:

- The same rules on every member's Mac: `team-rule-set`, `rules-a-member-cannot-switch-off`,
  `managed-and-shown`, and the feature `team-rule-sets`.
- Spend held per member and seen by the lead: `team-budget`, `spend-the-team-sees`, and the feature
  `team-spend`.
- What agents did, for a reviewer or an auditor: `receipt-on-a-pull-request`,
  `shared-receipt-signed`, `record-edits-show`, and the feature `receipts-on-pull-requests`.
- Secrets from the team's store: `secret-from-a-store` and the feature `secrets-from-a-store`.
- A shared main: `shared-host-system` finds that Apple's licence for macOS forbids time-sharing
  "within your own organization", so the shared main this feature describes runs on a Linux server,
  and `person-in-a-control-fact` names who did what on it.

Most of a team's needs do not need a shared main. Each member keeps their own main, and the team
shares rules, budgets, receipts and secrets through what it already owns: a repository, its device
management, its secret store and its log tool.
