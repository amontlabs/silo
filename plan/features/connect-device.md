## Proposed, not confirmed by the owner

### For the owner to settle

- Do devices reach each other over a private network the user already runs, such as Tailscale, or through device keys Silo manages itself as the old code did?
  - Recommendation: A network the user already runs, because Silo's own version grew large and never had a recorded run across two real devices.

### For whoever builds it

- Rule: The engine listens on loopback by default and requires a token on any other address.
- Property: When a coordinating service is down, devices and their computers keep working, and only joining a device and rotating keys wait for it.
- Detail: Connecting another device is one guided step, and an agent finds that step in the command line's help.
- Detail: A remote client that is refused is told exactly how to connect.
- Test: An agent given no hints connects a Mac's app to an engine on a Linux server over a private network, and the first run works.
- The docs give an SSH port forward and `tailscale serve` as supported ways to reach an engine from another device.
- Find out whether the engine can listen on a private network address with only its token in front, or needs a proxy there.
- To find out: If devices need messages, streams or a key-value store between them, compare using NATS with writing it.
- To find out: If devices ever share state, find out whether agreeing on the event log is enough, with no device replaying it.
- To find out: If devices ever share state, find out whether TigerBeetle's Viewstamped Replication over Silo's state machine is a route.

## From the old code

The old code had it partly built, about 15,000 lines, and it needs reworking.

Lessons:
- Both devices must run the same remote protocol version; an updated one refuses the older and names it.
- A computer's agent is refused any call that names another device.
- Every Connections call silently failed for a long time because the forced command's flag had no caller after the engine moved; keep a live end-to-end check.
- macOS refuses a socket timeout once the peer closed, about 1 call in 6, while the reply is still buffered: ignore it and read.
- Both devices must run the same bridge protocol version; name which device to update.
- A changed host key is never trusted from here; a typed fingerprint is the trust root.
- Both devices must run the same protocol version; the newer one names the device to update.
- `restrict` in authorized_keys disables TCP and socket forwarding together, so guest streams travel over the bridge, not SSH forwarding.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/connections.rs`
- `app/SiloUI/src-tauri/silod/src/methods/remote.rs`
- `app/SiloUI/src-tauri/silod/src/methods/guest_remote.rs`
- `app/SiloUI/src-tauri/engine/src/remote.rs`
- `docs/SiloUI-CONNECTIONS.md`
- `docs/hosts/BUILD_LOG.md`
- `app/SiloUI/cli/src/hosts.rs`
- `app/SiloUI/cli/tests/devices.rs`
- `docs/SiloUI-CLI.md#hosting-computers-for-other-devices`
- `app/SiloUI/src/features/application/components/connections-settings.tsx`
- `app/SiloUI/src-tauri/src/commands/remote.rs`
- `docs/SiloUI-APP.md#L14`

Calls: `connections.status`, `connections.setEnabled`, `connections.devices.list`, `connections.devices.connect`, `connections.devices.health`, `connections.devices.remove`, `connections.keys.list`, `connections.keys.own`, `connections.keys.authorize`, `connections.keys.revoke`, `connections.ssh.authorizationCommand`, `connections.ssh.keySetupCommand`, `silo devices add`, `silo devices key`, `Connect device…`, `silo devices list`, `silo devices remove`, `silo host revoke`, `silo host init --authorize`.
