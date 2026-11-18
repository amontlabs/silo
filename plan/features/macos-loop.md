## Open

- To find out: Can a macOS computer be forked while it runs, given the limit of two macOS computers per Mac?

## Notes

- Old findings: a macOS fork re-ran the developer tools install its source never ran (46 s) and shut down and booted three times (27 s). Personalization printed a password error yet exited 0; check the password change applies.
- Hoy (Cua, Nov 2026, closed source) forks a running macOS VM in about 1 s on its own engine over Hypervisor.framework. The cold-boot fork is therefore a limit of Virtualization.framework, not of macOS. Its forks keep the memory and change name, address and ssh keys after resume. Untested idea: restore a fork with the source's MAC and identifier on its own link behind the gateway, then change identity inside the guest.

## From the old code

The old code had it partly built, about 2,000 lines, and it needs reworking.

Lessons:
- A saved memory state is refused after some host macOS updates, so record the host build and fall back to a cold boot from the same disk.
- A memory state needs the same MAC address it was saved with; always build a computer from the MAC in its record.
- A memory state cannot move to a new MAC and machine identifier, so forks always cold boot from disk.
- Jobs apps register through SMAppService cannot be parked offline without breaking their signature; list them instead.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-MACOS-COMPUTERS.md#L210`
- `app/SiloUI/src-tauri/engine/src/macos_computers/checkpoints.rs`
- `docs/research/macos-checkpoints-2026-10-10.md`
- `docs/SiloUI-MACOS-COMPUTERS.md#L270`
- `app/SiloUI/src-tauri/engine/src/macos_computers/fork_first_boot.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/offline_setup.rs`
- `docs/SiloUI-MACOS-COMPUTERS.md#L320`
- `app/SiloUI/src-tauri/engine/src/macos_computers/quiet.rs`

Calls: `checkpoints.create`, `checkpoints.restore`, `checkpoints.delete`, `checkpoints.fork`, `silo fork`, `checkpoints.fork quiet`.
