# May Silo make a destructive call undoable before it lets the call pass

## The choice

An agent's part, 24 November 2026. At 3 a.m. an agent sends AWS RDS `DeleteDBInstance` with
`SkipFinalSnapshot=true`. The automated backups go with the instance. You are asleep, the hold
ends, and the request is refused. In the morning the agent asks again.

The options:

- Yes, made undoable, on your yes or a rule (recommended). The ask offers a third answer: yes,
  made undoable. For RDS, Silo sends the delete with a final snapshot instead. For a Git force
  push, Silo first writes a backup ref at the old commit, then lets the push pass. A rule can
  choose this answer by itself for an operation you name. It costs money at the provider (a
  snapshot is billed), a change to what reaches the host, which the record and the agent are
  told about, and a copy that may fail or take minutes, so not every operation can have it.
- No. Silo only allows, holds or denies. The ask names the provider's own undo, if it has one.
  It costs the cases where a yes loses data that one more parameter would have kept.
- Only the provider's own switches, never a call of Silo's. Silo may change a parameter, such as
  the final snapshot, but makes no call of its own. It costs the Git, Neon and DNS cases, where
  the provider has no parameter but one cheap call first keeps everything.

Recommended: yes, made undoable, on your yes or a rule. It protects what a yes would lose and
grants nothing that the yes did not.

## What it challenges

- `what-a-precaution-may-do`, taken: a precaution may alert, checkpoint, pause, deny or hold. This
  entry keeps that: a precaution never makes a call undoable.
- `kinds-of-action`, taken, has no kind for "let it pass, changed". The pass is the grant, and is
  yours or a rule's. The change only keeps a way back, so it is protective, but it spends money,
  which is why it is limited to you and rules.
- `what-silo-changes-in-traffic`, answered, says Silo changes model traffic only by swapping a
  secret. This entry is about the traffic to other hosts, but it is a second change Silo makes to
  a request, and `no-silent-change-to-traffic` applies: the original and what was sent are both
  facts.

## An agent's notes

- Per operation, which path works, what it costs, how long it takes and whether the copy
  survives the delete: `research/2026-11-24-destructive-calls/details/undo-paths.md`.
- A copy that dies with what it copies is no undo: Railway's volume backups, RDS automated
  backups, Fly snapshots when an app is destroyed, Netlify DB snapshots.
- Copies that take minutes, such as an RDS or EBS snapshot made by Silo, do not fit in a hold.
  A parameter that makes the provider take the copy, such as the final snapshot, does.
