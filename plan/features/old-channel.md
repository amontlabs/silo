## From the old code

The old code had it and then dropped it and it would be written again.

Lessons:
- It needed holds and cache dropping around full checkpoints and did not work for macOS.
- A guest could redirect the spool to device files, and fork ownership went by name.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-IN-GUEST-CLIENT.md#the-msb-folder`
- `docs/v1/evidence/phase-5/inguest-check.txt`
- `docs/SiloUI-IN-GUEST-CLIENT.md`

Calls: `silo guest-client`.
