## Open points

- May need `private-channel`, which comes much later in the order. The old loopback port with a token already met the need.

## From the old code

The old code had it built and driven end to end, about 8,700 lines, and its approach can be reused.

Lessons:
- One loopback port per computer plus a token only that computer holds; the network allows exactly that port.
- The engine puts the shipped program back at each start if an agent replaces or deletes it.
- The peer address is always 127.0.0.1, so only the token tells computers apart; never send the token itself, prove it per nonce and port.
- Any program on the device can connect to the loopback port; on a shared Linux host this is weaker than single-user.
- The earlier /.msb request-file bridge needed holds and cache dropping around checkpoints and did not exist for macOS; it was removed.
- A loopback TCP port has no owner: any account can connect or take it while the engine is down, so the token is the identity.
- Keep a port that is no longer a computer's bound with an accept-and-close listener until the computer restarts without it.
- Device-only tools (up, ssh_name, request_open) are hidden inside a computer; secrets and export/import are never MCP tools.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/guest_client.rs`
- `app/SiloUI/src-tauri/silod/src/methods/guest_client.rs`
- `app/SiloUI/src-tauri/engine/src/guest_client`
- `docs/SiloUI-IN-GUEST-CLIENT.md`
- `docs/direction/evidence/T5b.txt`
- `docs/direction/evidence/T5a.txt`
- `app/SiloUI/client/src/session.rs`
- `app/SiloUI/cli/src/endpoint.rs`
- `docs/direction/checks/t5_channel_check.sh`
- `app/SiloUI/src-tauri/engine/src/guest_client/mod.rs`
- `app/SiloUI/src-tauri/engine/src/guest_client/tokens.rs`
- `app/SiloUI/src-tauri/engine/src/guest_client/endpoint.rs`

Calls: `guestClient.enable`, `guestClient.revoke`, `guestClient.status`, `engine.handshake`, `silo tools`, `silo mcp`, `agents.install`, `silo tools status`, `silo tools enable`, `silo tools disable`.
