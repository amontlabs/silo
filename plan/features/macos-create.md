## Proposed

- Rule: The Swift code that drives Apple's virtualization framework belongs to the engine side and never runs inside the app.

## Open

- Apple allows two macOS computers per Mac, so when a third is asked for, does Silo refuse, queue it, or offer to stop one?
  - Suggested: Refuse in one line that names the two and offers to stop one, as resource-floors refuses when memory would run out.

## From the old code

The old code had it partly built, about 45,600 lines, and it needs reworking.

Lessons:
- macOS guests must sit behind Silo's gateway, never Apple NAT, so policy and secrets apply (T7).
- Import of a 20 GiB file staged 65 GiB beside it; disk space is a real limit (T7).
- libkrun cannot boot macOS, so macOS needs a second VM engine (Virtualization.framework).
- Apple's license allows two macOS VMs per Mac and forbids shipping a pre-installed image, so each Mac downloads and installs its own.
- The framework needs the virtualization entitlement, so an unsigned `tauri dev` binary cannot run macOS computers.
- The VM lives inside the engine process, so an engine crash powers off every macOS computer.
- Before the account exists, Stop can only press the virtual power button, which asks for confirmation inside macOS.
- Lume keeps the VM in its own process (VNC only) and Tart is FSL-licensed, so both were rejected.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/macos.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers`
- `docs/direction/evidence/F.txt`
- `docs/direction/evidence/T7.txt`
- `docs/SiloUI-MACOS-COMPUTERS.md#L13`
- `app/SiloUI/src-tauri/engine/src/macos_computers/restore_image.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/engine.rs`
- `docs/SiloUI-MACOS-COMPUTERS.md#L718`
- `app/SiloUI/src-tauri/engine/src/macos_computers/run_marker.rs`
- `docs/research/macos-guest-engine-2026-10-09.md`
- `docs/SiloUI-MACOS-COMPUTERS.md`

Calls: `macos.setup.retry`, `macos.template.delete`, `macos.display.connect`, `macos.display.resize`, `macos.display.session`, `macos.display.stream`, `macos.create`, `silo create --macos`, `macos.action`, `macos.action setup`.
