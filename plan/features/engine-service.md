## Proposed, not confirmed by the owner

- The engine is its own program, not a library inside the app, so computers keep running when the app closes.
- The core decides and touches nothing: it takes state and a call and returns the new state, events and things to do. Drivers do them (run a VM, set up the network, talk to GitHub).
- Layout: `engine/` (model, core, drivers, server, sim), `cli/`, `app/`, `guest/`, `tools/`.
- Zig pinned to one version (0.17.1 was the latest stable on 2026-11-02), with its docs at hand.

## From the old code

The old code had it partly built, about 10,400 lines, and it needs reworking.

Lessons:
- The app did not start its own Dev engine (launchd needs LWCR update with ad-hoc signing); first launch fell on a stale error screen.
- Quit stops Silo-owned local computers but never computers on other devices.
- launchd refuses ad-hoc-signed Dev engine builds ('needs LWCR update'), so live checks start silod directly.
- SSH starts the bridge through a ~/.local/bin link where macOS finds no bundle; the process must re-exec from its real path.
- The runtime socket path must stay under 104 bytes, which caps the HOME length at 28 bytes.
- Linux computers are detached runtime processes, so an engine exit need not kill them; systemd KillMode=process is required to keep that true.
- A session end that leaves a computer running must exit non-zero so the supervisor records it.
- Ctrl-C in a terminal reached msb children too; start them in their own process group.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/engine.rs`
- `app/SiloUI/api/src/transport.rs`
- `docs/SiloUI-ENGINE.md`
- `docs/direction/evidence/F.txt`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-ENGINE.md#L40`
- `app/SiloUI/src-tauri/silod/src/agent.rs`
- `app/SiloUI/src-tauri/silod/src/ownership.rs`
- `app/SiloUI/src-tauri/silod/src/app.rs`
- `docs/product/JOURNEYS.md#L19`
- `app/SiloUI/src-tauri/src/engine_client/mod.rs`
- `app/SiloUI/src/desktop/engine-connection.ts`

Calls: `engine.handshake`, `engine.state`, `engine.disconnect`, `engine.shutdown`, `silod --agent`, `silod --service`.
