## Proposed, not confirmed by the owner

- Each concept states its properties, and a simulator checks them on every change with random sequences of calls, crashes included.

## From the old code

The old code planned it but did not build it, about 6,500 lines, and it needs reworking.

Lessons:
- Keychain is absent under another HOME, so checks needing saved sign-ins cannot use their own instance.
- Only the real engine on the KVM runner showed several container bugs; keep a real-VM job in CI.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/scripts/live/run`
- `docs/SiloUI-ENGINE.md#L90`
- `.github/workflows/ci.yml`
- `.github/workflows/linux-host.yml`
- `app/SiloUI/scripts/ci-native-changes.py`
- `app/SiloUI/UI-PATTERNS.md#L89`
- `app/SiloUI/src/fixtures`

Calls: `scripts/live/run`.
