## Proposed, not confirmed by the owner

### For whoever builds it

- Detail: The diff also lists what left the fork and stays done after a drop, such as pushes, messages sent and other requests made with a secret.

## Notes

- The old secrets proxy logged nothing on success, and macOS dropped its stderr; it needs host and method logging on both platforms before this line can exist.

## From the old code

The old code had it built, about 1,500 lines, and it needs reworking.

Lessons:
- Cap the event file and keep a never-rotated counter summary, so a busy computer cannot grow it without bound.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-SECRETS.md`
- `app/SiloUI/patches/microsandbox-secret-usage-events-0.7.14.patch`

Calls: `computers.credentials`.
