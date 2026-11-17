## From the old code

The old code had it partly built, about 600 lines, and its approach can be reused.

Lessons:
- Never concatenate the diagnostic into the summary.
- A leftover record from an older engine blocks a rebuilt Dev app at startup.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src/components/error-details.tsx`
- `docs/SiloUI-RUNTIME-ERRORS.md`
- `docs/SiloUI-FAILURE-REPORTING.md`
- `app/SiloUI/src/features/application/pages/system-issue-page.tsx`
- `app/SiloUI/src/desktop/startup-failure.tsx`
- `docs/product/JOURNEYS.md#L22`
