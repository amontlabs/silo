# May Silo make a destructive call undoable before it lets the call pass

## The choice

An agent's part, 24 November 2026. At 3 a.m. an agent sends AWS RDS `DeleteDBInstance` with
`SkipFinalSnapshot=true`. The automated backups go with the instance. You are asleep, the hold
ends, and the request is refused. In the morning the agent asks again.

The options:

- Yes, made undoable, on your yes or a rule (recommended). The ask offers a third answer: yes,
  made undoable. For RDS, Silo sends the delete with a final snapshot instead. For a Git force
  push, Silo first writes a backup ref at the old commit, then lets the push pass. A rule can
  choose this answer by itself for an operation you name. It works for a few dozen operations,
  not all. It costs money at the provider (a
  snapshot is billed), a change to what reaches the host, which the record and the agent are
  told about, and a copy that may fail or take minutes, so not every operation can have it.
- No. Silo only allows, holds or denies. The ask names the provider's own undo, if it has one.
  It costs the cases where a yes loses data that one more parameter would have kept.
- Only the provider's own switches, never a call of Silo's. Silo may change a parameter, such as
  the final snapshot, but makes no call of its own. It costs the Git, Neon and DNS cases, where
  the provider has no parameter but one cheap call first keeps everything.

Recommended: yes, made undoable, on your yes or a rule. It keeps what a yes would lose, for the
operations where the provider or one cheap call makes that possible.

## What it challenges

- `what-a-precaution-may-do`, taken: a precaution may alert, checkpoint, pause, deny or hold. This
  entry keeps that: a precaution never makes a call undoable.
- `kinds-of-action`, taken, has no kind for "let it pass, changed". It is not protective there: a
  wrong one costs money at the provider, and it keeps data you may have meant to erase, such as
  the copy of a customer's records. So it is granting, triggered by you or a rule. A change that
  can only make the provider refuse more, such as PostgREST's `Prefer: max-affected`, is
  protective.
- `what-silo-changes-in-traffic`, answered, says Silo changes model traffic only by swapping a
  secret. This entry is about the traffic to other hosts, but it is a second change Silo makes to
  a request, and `no-silent-change-to-traffic` applies: the original and what was sent are both
  facts.

## An agent's notes

- Of 48 operations studied, a change to the request keeps the provider's own way back for 11:
  the final snapshots of RDS, Aurora, ElastiCache and Redshift; the waiting windows of Secrets
  Manager and KMS; Cloud SQL's final backup; Atlas `retainBackups`; Railway `projectDelete` sent as
  `projectScheduleDelete`, which returns the same type; Gmail and Drive deletes sent as moves to
  the trash. On AWS it needs Silo to sign the request (`research/2026-11-24-gap-fixes/`).
- A change can break the agent's next step. A secret deleted with a waiting window keeps its name
  for 30 days, so a delete followed by a create of the same name fails. An RDS delete with a final
  snapshot stays in `deleting` until the snapshot is done. The refusal or the record says so.
- A call of Silo's own first works with one call for a GitHub backup ref, a Cloudflare D1
  bookmark, a Cloudflare DNS export, EC2 `DeleteOnTermination=false` and a Fly volume snapshot.
  On Neon, a backup branch under the branch to delete makes the delete fail.

- Per operation, which path works, what it costs, how long it takes and whether the copy
  survives the delete: `research/2026-11-24-destructive-calls/details/undo-paths.md`.
- A copy that dies with what it copies is no undo: Railway's volume backups, RDS automated
  backups, Fly snapshots when an app is destroyed, Netlify DB snapshots.
- Copies that take minutes, such as an RDS or EBS snapshot made by Silo, do not fit in a hold.
  A parameter that makes the provider take the copy, such as the final snapshot, does.
