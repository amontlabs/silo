# One budget for a team's host

## The choice

An agent's part, 25 November 2026. A team of five gives its host $3,000 a month for models, and
each person $1,000. #362 found a pooled budget across Macs needs a count every Mac reaches before
each call, so it proposed a budget per person (`team-budget`). On one host, every computer's
model calls cross boundaries on the same machine, so one count is local.

#374 measured how a budget stops calls: holding each model's typical output before the call
stopped 7% to 9% under a $3 budget, and letting calls finish overspent by about 54%. A pooled
count works the same way, with every person's boundary reserving against one count kept by the
engine.

The options:

- A pooled limit on the host, beside each person's budget (recommended). Exact on that host. It
  costs one count the engine keeps, which every boundary on the host asks before a model call.
- Per-person budgets only. It costs the team's total as a limit: it is only a sum.
- A pooled limit only. It costs the case where one person spends the team's month in a night.

Recommended: a pooled limit beside each person's budget. It is what a team lead sets, and on one
host it costs a local call.

Not built: the reservation is #374's, run for one computer, not shared between boundaries.

## Where the drafts disagree, second pass (integration)

An agent's part, 25 November 2026, written while merging #390 and #392 to #396 onto #391 (`research/2026-11-25-integration-2/`).

#388's `pooled-budget-ledger` and this entry answer the same question in two places. This one keeps one count on a host and reserves each call against it; #388's gives each Mac a slice from a ledger. They agree once the ledger runs on the host, as #388 suggests. Recommended: computers on the host draw from the host's count directly, Macs take slices from the same count as #388's leases, and this entry needs `pooled-budget-ledger`.
