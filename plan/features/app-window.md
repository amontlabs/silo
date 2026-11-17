## From the old code

The old code had it built, about 6,800 lines, and it needs reworking.

Lessons:
- A failed start leaves the computer Stopped; keep the result of the request separate from the computer's state, keyed by computer id.
- Names and disk sizes are fixed after creation; only CPU and memory can change.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src/features/application/components/application-shell.tsx`
- `app/SiloUI/src/features/application/application-app.tsx`
- `app/SiloUI/src/features/application/model/use-application-navigation.ts`
- `app/SiloUI/src/features/application/pages/overview-page.tsx`
- `app/SiloUI/src/features/computers/components/computer-configuration-list.tsx`
- `app/SiloUI/src/features/computers/components/computer-list.tsx`
- `docs/SiloUI-FAILURE-REPORTING.md`
- `app/SiloUI/src/features/application/pages/computer-detail-page.tsx`
- `app/SiloUI/src/features/application/components/lifecycle-control.tsx`
- `app/SiloUI/src/features/computers/model/computer-capabilities.ts`
- `app/SiloUI/src/features/computers/components/computer-editor.tsx`
- `app/SiloUI/src/features/computers/model/use-computer-editing.ts`

Calls: `exports.*`.
