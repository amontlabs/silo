## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- It needs signed releases, which wait; computers do not need it because their identity is their channel.
- Same-user malware stays a non-goal: identity makes actions attributable, it does not make that case safe.
- The runtime runs as the user's own process without the App Sandbox, so a VM escape gives the user's account.

Where it lived, at the tag legacy-2026-11-16:
- `docs/ROADMAP.md#L331`
- `docs/evals/README.md`
- `docs/ROADMAP.md#L499`
