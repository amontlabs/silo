## Proposed, not confirmed by the owner

### For the owner to settle

- Does Silo count use at all: nothing, a local summary the user chooses to share (for example with a bug report), or opt-in counters sent in the background?
  - Recommendation: A local summary shared by choice, because it needs no telemetry store and nothing leaves the device unseen.
- Does the disclosure policy pay reporters, beyond crediting them by name and publishing the fix jointly?
  - Recommendation: 

### For whoever builds it

- Rule: Any measurement of use is opt-in, visible, and kept on the device where possible.
- Rule: A security contact address and a disclosure policy are published before the first security report arrives.
- Docs: The disclosure policy states that reporters are credited by name and that the fix is published jointly.
- Detail: If use is counted, one use is a first run that reached a merge, since Silo cannot see installs.
- To find out: Which suits a telemetry store better, OpenObserve (light, can start on SQLite) or PostHog (heavier, needs ClickHouse and Redis), with telemetry.dev as the view to aim for?

## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- Docs that no longer describe the code are deleted, not archived (owner, 2026-11-15).

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/PRODUCT.md`
- `docs/SiloUI-FAILURE-REPORTING.md`
- `app/SiloUI/THIRD-PARTY-NOTICES.md`
- `docs/ROADMAP.md#L421`
- `SECURITY.md`
