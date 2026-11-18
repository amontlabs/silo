## Proposed, not confirmed by the owner

### For whoever builds it

- Rule: The app and the command line also reach the engine through a unix socket protected by file permissions, never through a network port.

## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- The channel token is valid only on its own computer's port, so a copy of it in a fork or an export proves nothing.
- A restore resets the runtime's network rules unless they are all passed again, so restore and fork rebuild them from the stored policy.
- The old /.msb file-polling bridge was replaced; the share still has no security review.
- The guest can change files the host later reads there (checkpoint state, heartbeat); Silo parses nothing from it but the share has had no security review.

Where it lived, at the tag legacy-2026-11-16:
- `docs/THREAT-MODEL.md#L651`
- `docs/DIRECTION.md#L85`
- `docs/ROADMAP.md#L477`
- `docs/direction/PLAN.md#L265`
- `docs/ROADMAP.md#L358`
- `docs/THREAT-MODEL.md#L384`
