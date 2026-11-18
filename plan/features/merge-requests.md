## Proposed, not confirmed by the owner

### For the owner to settle

- Should a final review by the strongest model, at $1 to $3, be a default step before a fork asks to merge?
  - Recommendation: No, a step the user turns on, because Silo runs no model of its own and the spend is the user's.

### For whoever builds it

- Rule: The note of a merge request has a fixed, short form with no jargon and reads in a few seconds above the diff.
- Detail: The note leads with what needs the person's attention and whether the checks passed.
- Detail: The note then says what was researched, planned, changed and checked, the fork's important decisions, the goals it missed with the reason, and what is left.
- Detail: Apart from the agent's own report, the request shows which checks ran and passed, which reviewing agents approved and whether a person read the diff.
- Detail: A fork that an agent has reviewed shows that state before it asks for the person.
- Detail: The fork keeps the request or spec that started it and shows it beside the diff, so additions beyond it stand out.
- Detail: A merge request can carry screenshots, a walkthrough or a recording of the running result, terminal or screen, shown in the request and in the diff view.
- Rule: A fork that asked to merge stays available as a live computer that checks can act on.
- Detail: The review that judged a fork is stored with the fork.
- Detail: A reviewer of a fork is given the fork's intent.
- Detail: A fork's record can show a test's red run, then its green run, as evidence for the merge.
- Detail: A review request can say what to look at, such as "pay special attention to …", written by the agent or the person.
- Detail: Declining a request can attach notes to lines of the diff, which travel with the fork and reach its agent.
- Detail: The note also lists what failed and was retried, with the reason, and what the agent found that nobody asked for, such as an unrelated bug.

## From the old code

The old code had it built and driven end to end, about 3,100 lines, and it needs reworking.

Lessons:
- Wire names keep 'keep' and 'fold' spellings while people read 'merge'; a rewrite should pick one word end to end.
- A merge approval and its review request are linked; denying one must close the other.
- Once linked to an approval only the engine writes the request's text.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/review_requests.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/requests.rs`
- `app/SiloUI/src-tauri/engine/src/fold_requests.rs`
- `docs/direction/evidence/V.txt`
- `docs/direction/evidence/M.txt`
- `app/SiloUI/cli/src/review.rs`
- `app/SiloUI/cli/tests/requests.rs`
- `docs/SiloUI-CHANGES.md#review-requests-changesrequest`

Calls: `changes.request`, `changes.requests.list`, `changes.requests.show`, `changes.requests.keep`, `changes.requests.markKept`, `changes.requests.throwAway`, `changes.requests.open`, `silo merge --request`, `silo requests`.
