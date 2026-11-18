## Proposed, not confirmed by the owner

### For whoever builds it

- To find out: Can the component that swaps secrets for Linux computers also serve a macOS computer, or does macOS need a gateway that copies microsandbox's swap, as first intended?
- Property: Every property stated for secrets on Linux also holds for a macOS computer.

## From the old code

The old code had it partly built, about 7,700 lines, and it needs reworking.

Lessons:
- The host cannot create files the guest sees as root-owned, so bootstrap files must be replaced in place, never created.
- A first macOS install still uses Apple's network until bootstrap files are inside the template.
- Closing the first-install window needs the network bootstrap files inside the template, which rebuilds every user's template.
- Recovery boots get no network card.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/macos_computers/ports.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/files.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/ssh_alias.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/guest_access.rs`
- `docs/SiloUI-MACOS-COMPUTERS.md#L538`
- `app/SiloUI/src-tauri/engine/src/macos_computers/proxy/bootstrap.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/proxy_ca.rs`
- `app/SiloUI/src-tauri/macos-gateway/README.md`
- `app/SiloUI/src-tauri/guest/macos/silo-network-bootstrap.swift`
- `app/SiloUI/src-tauri/guest/macos/NETWORK-BOOTSTRAP.md`
- `app/SiloUI/src-tauri/guest/macos/remove-inherited-proxy-ca.sh`
- `docs/direction/evidence/T7.txt`

Calls: `silo ssh-setup`.
