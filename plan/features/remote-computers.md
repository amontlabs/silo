## Proposed

- Property: A dropped connection to a device loses no work, and the app and the command line pick up where they were when it returns.
- Rule: A request to a device that is offline waits only until its deadline, then fails and says so, and it never runs later.
- A computer can move whole to another device, keeping its name and access, and leaves the first device once it runs on the second.

## Notes

- `fork --on <device>` makes the fork on another device; one call lists pending merge requests from every connected device.
- A task that ran on a server ends with someone forking its checkpoint onto their own device to check the work. That shows more than a pull request does, which reads as ordinary CI (2026-11-17).

## From the old code

The old code had it partly built, about 3,200 lines, and it needs reworking.

Lessons:
- Prefer a --device flag over name@device so computer names stay plain.
- Ports are published by the device a computer runs on; URLs differ per side.
- A device never forwards a request on; each handler refuses a request that still names another device.
- A retried change must attach to the original operation id, and a change not started before its deadline or after its controller left must never start.
- An unrelated process can take a reserved port during SSH authentication; trust only `ssh -O check` on a private control socket before exposing the endpoint.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/engine.rs`
- `app/SiloUI/cli/src/share.rs`
- `app/SiloUI/cli/tests/devices.rs`
- `app/SiloUI/cli/tests/mcp_devices.rs`
- `docs/SiloUI-CLI.md#another-device`
- `docs/SiloUI-CONNECTIONS.md#L95`
- `app/SiloUI/src-tauri/silod/src/methods/remote.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/all_devices.rs`
- `docs/hosts/PLAN.md`
- `app/SiloUI/src-tauri/engine/src/remote/operations.rs`
- `docs/SiloUI-CONNECTIONS.md#L5`
- `app/SiloUI/src-tauri/silod/src/relay.rs`

Calls: `silo --device`, `silo list --all-devices`, `silo requests --all-devices`, `guest.exec`, `computers.list`, `--device`, `events.subscribe`, `list`, `requests_list`, `guest.ssh`.
