## From the old code

The old code had it built and driven end to end, about 200 lines, and its approach can be reused.

Lessons:
- A recipe and a command cannot go in one call, and the computer stays running afterwards (journey 3 gap).

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/fanout.rs`
- `docs/product/JOURNEYS.md`

Calls: `silo fork -- <cmd>`.
