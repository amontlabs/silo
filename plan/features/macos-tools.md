## Proposed, not confirmed by the owner

### For whoever builds it

- To find out: Can a macOS computer give an agent a way to test an iOS or Mac app end to end, for example with agent-device?
- Test: Start a Docker container with a volume inside a macOS computer; the container reads and writes the volume's files.

## From the old code

The old code had it built, about 1,600 lines, and it needs reworking.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/macos_computers/developer_tools.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/agent_install.rs`
- `app/SiloUI/src-tauri/guest/macos/silo-developer-tools.zsh`
- `app/SiloUI/src-tauri/guest/macos/developer-tools-lock.json`

Calls: `agents.install`.
