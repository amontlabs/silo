## From the old code

The old code had it built, about 1,100 lines, and it needs reworking.

Lessons:
- The push destination never comes from the fork; a fork's own origin naming another repo is refused.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/pr.rs`
- `app/SiloUI/cli/src/summary.rs`
- `docs/SiloUI-CLI.md#pull-requests`
- `docs/SiloUI-CLI.md`
- `docs/product/JOURNEYS.md#L73`

Calls: `silo merge --pr`.
