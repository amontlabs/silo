# silo-runner: is there a simpler lifecycle design? (7 Nov 2026)

Two code-review rounds on PR #83 keep finding lifecycle and ownership bugs in
`app/SiloUI/runner/` (scaler.go, reconcile.go, state.go). Round 2 (gpt-6.1-sol, on 508c87d3):

1. P1 Retention (`EnforceKeep`) can delete a computer marked kept before `RemoveRunner` succeeded
   (count 0, or concurrently with deregistration; also reconcile's final EnforceKeep).
2. P1 Normal cleanup deletes by name without checking the recorded id: a replacement computer with the
   same name is deleted; a name collision makes Fork fail and `abandonStart` deletes the pre-existing one.
3. P2 "Drop can't find the name and List doesn't show it" is treated as deleted; a renamed computer or
   a cached inventory read makes the ledger forget a live computer. Check by id.
4. P2 Reconcile failures (runner lookup, deregistration, delete) only log; reconcile runs once at
   startup, so a transient GitHub outage strands capacity (both macOS slots) until restart.
5. P2 Failed `SetID` during reconcile, then the final sweep drops the ledger entry.
6. P2 A failed `JobCompleted` arriving while `abandonStart` has the runner in `stateFinishing` is
   discarded; cleanup deletes the failed job's computer. Successful completions during silent
   settlement are recorded as unknown.
7. P2 Rebuild memory reservation uses the scale-set config, not the recipe's real memory.
8. P2 Ledger writes are not fsynced (temp file and parent dir).

Fable (threat model, round 2): LGTM; low notes: a job in flight across a helper restart is always kept
(result unknown); the threat model overstates "recorded by id before the computer exists".

The in-memory runner carries flags `jobSeen`, `early`, `watching`, `adopted`, `exited`, `pending`,
`retryAt`, several goroutines touch it, and the ledger (`state.go`) is a separate store.

## Candidate simplifications

- **One loop owns all state.** A single goroutine receives every event (scaleset message, runner exited,
  ready, timer tick) and makes every transition; slow calls (fork, RemoveRunner, drop) run as tasks
  whose results come back as events. No locks on runner state, no concurrent retention.
- **One state per computer, persisted:** `reserved -> forked(id) -> ready -> busy -> done(result) ->
  deregistered -> kept | deleted`. Retention only touches `kept` (which means deregistered). Every step
  is idempotent and retried by the loop's tick, so reconcile is not a one-off at startup: it is the same
  tick comparing the ledger with `silo list --json` (by id) and GitHub's runners.
- **Everything by id:** the CLI acts on computers by id (check whether `silo drop/stop/exec` accept ids);
  absence is "no computer with this id in a fresh list".
- **Ownership in the engine instead of a local ledger?** If Silo can tag a computer (owner label or
  metadata) the helper could find its computers from `silo list` alone. Probably not available; is it
  worth adding?
- **Fork with a fresh unique name** and never reuse names; a collision is a fork error with nothing to
  delete.
- **Unknown result after restart:** look the job up via the REST API, or accept "kept" and document it.
- **Recipe memory:** read it from the recipe file (or from the builder computer's size).

Question: which of these (or something else) gives the simplest design that is easy to explain and
removes this class of bugs, rather than patching each finding?

## Decision (7 Nov 2026)

Fable's design, with the advisor's pure capacity function. Both advisors agreed on one loop, everything
by id, absence only from a fresh successful list, reconcile on every tick, and no engine-side owner tag.
The advisor's zero-disk variant (ownership by name prefix, reap everything) is not taken: it loses keep
on failure and reintroduces deletion by name.

**State.** The ledger is the only state; in-memory structs cache it plus the live runner process. One
record per computer: `Entry { Name, ID, Label, Phase, Result, At, Request }`, with
`Phase: forking | live | finishing | kept`.

- `forking`: written before `silo fork`; ID empty; name `ci-<label>-<uuid8>`, never reused.
- `live`: ID known. Runner sub-state (starting, idle, busy) in memory only; re-derived from GitHub
  after a restart.
- `finishing(Result)`: job ended, start failed, runner exited or idle timeout. Deregister at GitHub,
  then stop (keep) or drop.
- `kept(Result)`: deregistered and stopped. The only phase retention may delete.
- Gone: entry removed only after a fresh, successful `silo list` lacks the ID.

**One goroutine** per helper receives every event and is the only writer of records and the ledger. Slow
calls run as tasks returning an event: `ForkDone`, `StartDone`, `Ready`/`StartFailed`, `RunnerExited`,
`RemoveRunnerDone(ok|jobRunning|err)`, `StopDone`, `DropDone`, `Listed`, `GitHubRunners`; listener
messages, a 30 s `Tick` and `Shutdown` are events too. At most one task in flight per record. No
mutexes on runner state, no per-runner flags.

**`plan()`** runs after every event; for each record without a task in flight it starts the next step:
`forking` binds the ID when a fresh list shows that exact name, or removes the entry if the fork failed
(a name collision is a fork error, nothing to delete); `live` with no process (after a restart) asks
GitHub for the runner by name: busy stays, idle or absent goes to `finishing("orphan")`; `finishing`
removes the runner (not found counts as removed; job still running goes back to `live` busy), then stops
(keep) or drops by ID; `kept` past `keepFailed` days or count goes to drop. Scaling: start forks while
`live + forking < min(max(assigned, outstanding), maxRunners)` and capacity allows. Startup is not
special. A failed task sets a retry time and the tick retries.

**Capacity** is a pure function of the latest list (every running macOS VM on the Mac, the RAM of running
computers against the budget) plus `forking` records not yet listed. No reservation counters. Recipe
memory comes from the recipe file.

*Note, 7 Nov 2026:* recipe rebuilds hold capacity while they run (`Fleet.Reserve`), and live records the
latest list does not show running count as pending, except a record with no start in flight that a list
started after its fork shows stopped. A fork refused because the name is taken forgets the record and
deletes nothing; only an error where the engine may have made the computer (timeout, killed command,
unreadable output) keeps the record for a fresh list.

**Invariants.** 1) After fork, engine calls use the ID. 2) A computer is ours iff its id equals a record's
ID, or the record is `forking` and the names match exactly. 3) Absence means not in a fresh successful
list; a failed list or drop changes nothing. 4) Delete only from `finishing` after a successful or
not-found `RemoveRunner`, or from `kept`. 5) `kept` only after deregistration and stop succeeded.
6) A result arriving while `finishing`, before the drop, replaces the stored result (keep wins).
7) Ledger saves fsync the file and the directory.

**Restart with a job in flight:** adopted as busy; when GitHub no longer has the runner it finishes with
result `unknown` and is kept. Documented, no REST lookup.

**Survives:** silo.go (by ID; no list-by-name fallback in drop; drop stops a running computer first),
capacity.go as a pure function, state.go's `OpenLedger`, `save` (+fsync) and `Lock` with `Put` and
`Remove`; from scaler.go `ComputerName`, `keepResult`, `waitReady` (as a task), the `finished` dedupe
and backoff. Removed: reconcile.go and the scaler's separate finish, abandon, settle, retry, retention,
adopt and housekeeping paths.
