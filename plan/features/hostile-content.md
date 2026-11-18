## Proposed, not confirmed by the owner

### For whoever builds it

- Rule: A summary written by an agent is untrusted content, so a merge decision never rests on it alone.
- Test: A very large log is produced or pasted inside a computer, and the device stays responsive.

## From the old code

The old code had it built, about 700 lines, and its approach can be reused.

Lessons:
- The guest serves the viewer's JavaScript, so only Silo's own UI may start a clipboard or file transfer.
- Output is capped (64 KiB errors, 960 KiB runs) and never written to the device's disk.
- Registering a Rust command does not grant it; each window's capability file must list it.

Where it lived, at the tag legacy-2026-11-16:
- `docs/THREAT-MODEL.md#L433`
- `docs/THREAT-MODEL.md#L638`
- `docs/SiloUI-URL-PROXY.md`
- `app/SiloUI/src-tauri/capabilities`
- `app/SiloUI/src/test/native-permissions.test.ts`
- `app/SiloUI/src-tauri/src/command_permissions_tests.rs`

Calls: `clipboard.transfer`, `files.list`.
