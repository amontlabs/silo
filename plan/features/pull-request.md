## Proposed

- A pull request from a fork records the agent program and the review it went through, as commit trailers or in its body.

## Open

- Does a pull request from a fork carry the prompt that started the work, or only the agent program, the model and what was run?
  - Suggested: Only those by default, with the prompt added when you ask, since a prompt can hold private context and the repository may be public.

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
