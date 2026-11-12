# Codex boundary review: caller policy, gate and approvals (T1, T2)

Reviewer: lastdrop `reviewer`, ran on Codex gpt-6.1-sol (high). Read-only, static review of
`direction` at `672b40b31` on 2026-11-12. No edits, tests or engine launches.

## Findings

1. **High: approving resources silently opens networking.** `engine/src/policy/mod.rs:751`.
   A caller with closed networking asks for a larger macOS computer without `networkPolicy`.
   Policy computes the inherited closed policy but returns the resource approval before copying
   it into the parameters; the approval mentions resources only, and the computer is created
   with no saved policy, which defaults to open. Fix: write the resolved network policy before
   every creation return.
2. **High: approval summaries disclose hidden recipe commands.** `engine/src/policy/mod.rs:1535`,
   `silod/src/methods/approvals.rs:138`. A computer forks itself and asks to merge into itself;
   the summary includes the recipe's migration and after commands, and `approval_required`
   returns it to the caller before approval, leaking tokens in otherwise hidden commands. Fix:
   separate owner-facing approval details from the computer-facing summary and approval view.
3. **High: ignored parameters bypass memory admission.** `silod/src/methods/gate.rs:368`.
   `computers.start` on an owned child with an extra `"memoryGiB":0`: the resource gate trusts
   zero and reserves nothing, the lifecycle decoder ignores the field and starts the child at its
   real size; repeated starts cross the safety floor. Fix: derive requirements from
   method-specific validated parameters and the stored computer configuration.
4. **Medium: approval pins are checked before the mutation lock.** `approvals.rs:678`,
   `engine/src/changes/keep/mod.rs:514`. Two undo approvals with different ignored extra fields
   both pin the same latest merge, pass their checks when allowed together, then undo two
   different merges. Promote has the same gap. Fix: carry the expected pin into the job and
   compare it under the mutation lane.
5. **Medium: memory check and reservation are not atomic.** `gate.rs:478`, `gate.rs:526`.
   Parallel unapproved starts each see enough headroom before either reserves. Fix: assess and
   insert the reservation in one critical section.
6. **Medium: forks bypass the caller's resource ceiling.** `policy/mod.rs:652`,
   `runtime/checkpoints.rs:2725`. After the owner enlarges child B, smaller caller A can fork B
   freely; fork policy never compares resources. Fix: apply the creation resource comparison to
   forks and ask approval when inherited sizes exceed the caller's.
7. **Medium: acknowledging jobs resets the quota while results remain.** `gate.rs:620`,
   `silod/src/methods/operations.rs:182`. A computer repeatedly starts and acknowledges jobs;
   the registry keeps each result for the owner for ten minutes, exhausting the shared budget
   and refusing owner jobs. Fix: charge retained results to their computer until released and
   reserve owner capacity.
8. **Medium: computer traffic can suppress owner audit records.** `engine/src/audit.rs:426`.
   Request flooding fills the shared 4,096-row queue; full-queue handling drops owner rows too.
   Fix: rate-limit or coalesce before enqueueing and reserve an owner queue.
9. **Low: checkpoint usage reveals computers outside the caller's tree.**
   `runtime/checkpoints.rs:4157`. `usedBy` and deletion blockers name an owner-made fork of A.
   Fix: filter dependencies by caller visibility; use generic blockers for hidden dependants.

## What holds

Caller identity comes from transport, method classification is exhaustive, ordinary tree checks
use `created_by`, and approval decisions leave pending atomically once. No direct Computer route
to approval decisions, exports, rebuild, settings or Connections. Device entry points enforce
explicit method lists; no grant bypass found.
