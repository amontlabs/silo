## Proposed

- Rule: After a failure, the command line says what failed, what state remains and how to resume or drop it.
- Rule: A provider's error is shown exactly as received, and Silo's reading of it is labelled as Silo's and kept apart.
- Rule: When a failure is Silo's own, the error says so and says how to report it.
- Property: Every error carries a stable error code.
- Rule: A call to a removed or unavailable feature fails with an error that names what replaced it.
- Rule: An error about a missing driver names the driver.

## Open

- Should a failed step in a computer have a "fix this error" action beside it that hands the error to that computer's agent?
  - Suggested: Yes, because the agent already lives in that computer and the error's details are ready to hand over.

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
