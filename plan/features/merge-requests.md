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
