## From the old code

The old code had it and then dropped it and how much can be reused is not known.

Lessons:
- Kept as evidence: MicroSandbox snapshots and forks were preferred to keep the credential proxy.
- Pause and checkpoint lost data under storage exhaustion.
- The comparison used an old MicroSandbox; 0.7.2 already had snapshots and forks, so check the newest release first.
- A fork starts from the source as it is now, disk only: the warm checkpoint saved only about 1 s, so memory forks and the warm-checkpoint machinery were removed.
- MicroSandbox writes into its image cache and keeps absolute paths in descriptors.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CHECKPOINTS-PLAN.md#L12`
- `docs/research`
- `docs/research/e2b-adoption-assessment-2026-09-24.md`
- `docs/archive/SiloUI-E2B-REPLACEMENT-PLAN.md`
- `docs/research/e2b-qualification-gates-2026-09-23.md`
- `docs/forks-ci/PLAN.md`
- `docs/forks-ci/BUILD_LOG.md#L77`
- `docs/SiloUI-ENGINE.md#L119`
