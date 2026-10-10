# Connections

Connections link devices, so Silo on one device can manage the computers of another device while Silo is running on that device. The Tauri application at `app/SiloUI` owns this implementation. It does not install an independent management daemon.

## User behavior

1. On the owning device, open Settings → Connections and enable **Allow connections from other devices**. The OS must also accept SSH connections (Remote Login on macOS). Copy the displayed address.
2. On the controlling device, choose **Connect device…** and paste the address. Existing OpenSSH configuration, aliases, keys, and agents are reused. SSH URIs support custom ports and IPv6.
3. If SSH has not been authorized, use the explicit Terminal authorization action to verify the fingerprint/unlock an existing key. If key access is absent, **Set up Silo SSH key…** adds this device's public key to the remote account after SSH's normal trust/password prompts. No private key is copied.
4. Remote computers appear in the existing flat list. Their small blue device badge includes a network icon; hover or keyboard focus reveals the device, address, and availability. Display names can match local computer names.
5. **Run on** selects the owning device when creating a computer. Existing computer ownership cannot be changed by editing its configuration. Start, stop, restart, edit, and delete operate on that device. A device may have no computers.
6. Terminals and editors launch on the controlling device and connect to the guest through the owner. Files, logs, repository push, and network connections route to the owning device.

A controller can connect during onboarding without creating a local computer. Local backup/restore, account configuration, secrets, runtime repair, and application updates retain their existing local ownership; the UI labels those settings **This device**. Remote account configuration and backups are managed on the owning device. They are not copied or synchronized by connecting it. Per-computer sharing, computer migration, internet discovery, and automatic SSH-server installation are outside this change.

## Close and Quit

Closing the window retains the existing status-bar behavior. **Quit Silo** blocks new work, saves pending preferences, coordinates accepted operations, lets a cancelled file transfer clean up within the remaining shutdown budget, gracefully stops and verifies all Silo-owned local computers, closes this controller's tunnels, and exits. Computers on other devices are not stopped.

The existing screen shows **Stopping local computers…** while shutdown runs. A stop or settings-save failure keeps Silo open with an actionable error and restores manual controls. Computers already stopped are not automatically restarted after a failed Quit. Failed provisioning before metadata publication is handled using its validated recovery journal; unknown managed identities block a successful Quit rather than being silently abandoned.

A confirmed `Crashed` computer has no active runtime and counts as already stopped for Quit and update preparation. It is not added to the updater's resume list. Inspection failures, unknown states, and replaced identities still require resolution. This follows the pinned MicroSandbox [status definition](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/crates/db/lib/entity/sandbox.rs) and [terminal-state handling](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/sandbox/handle.rs).

**Dismiss** on a crashed computer acknowledges that crash and displays it as stopped. The owner verifies the computer identity and crash state, saves the acknowledgement against its runtime update timestamp, and removes any pending failed lifecycle intent. It does not start the computer, delete data, rewrite runtime status, or erase activity history. A later runtime timestamp or a new explicit lifecycle action invalidates the acknowledgement. Remote dismissal is performed by the owning device's Silo.

This describes graceful Quit, not process crashes or forced OS termination. Silo must remain running on the owner for connections. Disabling connections ends Silo guest sessions and rejects new management requests; already accepted computer operations retain their owner. It does not revoke an OS account's pre-existing general SSH permissions.

## Implementation boundaries

- `remote.rs` owns saved device identity, framed SSH RPC, the private Unix socket, the fixed bridge CLI, SSH setup, and durable operation acceptance/results.
- `runtime/remote_ops.rs` uses the existing runtime operations and mutation lock. Remote edits/deletes compare the expected computer configuration with the owner's current configuration. No cached controller inventory replaces a device inventory.
- Every remote target combines a stable device UUID and computer UUID (`silo-remote:<deviceId>:<computerId>`). Address changes do not change ownership; an address returning a different device identity is rejected until explicitly reconnected.
- `remote_access.rs`, `editor.rs`, and `terminal.rs` route guest access. Per-controller guest private keys stay local; only public keys are authorized by the owner. Guest streams cannot start stopped computers.
- `remote_network.rs` owns controller loopback SSH tunnels. A remote loopback address is never shown as a local endpoint without a live tunnel. Removing a connection, disappearing/replaced endpoints, loss of remote access, and successful Quit close owned tunnels.
- Every device downloads its own ChatGPT for Linux copy at its own start (see [ChatGPT app](SiloUI-CHATGPT-APP.md)); a controller never prepares it for another device. The bridge serves `chatgpt.status` (read) and `chatgpt.retry` (change, device level: no computer id). Settings, Connections polls `chatgpt.status` of connected devices and shows only a failure or an unreadable status; an owner whose Silo reports a state this Silo does not know is never reported as a problem. A computer's computer-use state is computed by the owner.
- `macos_remote.rs` serves the `macos.*` methods (table below) from the owner's Silo and holds the controller commands for them. `macos_remote_viewer.rs` opens the viewer window and answers its two commands; `macos_display_gateway.rs` is its loopback WebSocket endpoint. See [macOS computers on another Mac](SiloUI-MACOS-COMPUTERS.md#on-another-mac).
- `runtime/shutdown.rs` coordinates local shutdown. Settings exit generations prevent an old canceled timeout from completing a newer Quit.
- Local and remote sources remain separate in `production-source.ts`; only presentation combines them. Remote failure does not mark local computers unavailable. Controller views refresh remote state periodically; this is not disk or configuration synchronization.

Operational configuration permits an empty inventory; onboarding retains its explicit initial setup flow.

## Checkpoint request deadlines

Remote checkpoint creation, fork, and restore share a 122-minute complete-request
window. The controller sends `startWithinMs` as half its remaining request time:
initial admission must occur within 61 minutes, leaving 60 minutes for all owner
stages and one minute for framing and transport. The work allowance follows
[`RESTORE_EXPECTED_DURATION`](../app/SiloUI/src-tauri/src/runtime/checkpoints.rs),
which also sets Restore's slow-operation threshold. The 900-second capture limit
fits within that allowance; it does not define a complete restore's deadline.

Both the controller exchange and owner bridge use
[`request_timeout`](../app/SiloUI/src-tauri/src/remote.rs). Both devices need the
updated budget. Lost-connection retries retain the operation ID and attach to the
existing owner registry result. Admission expiration still prevents queued work
from starting; it does not cancel work that has already started.

Fake-clock regressions cover a 601-second capture, a 900-second fork, an hour of
restore work admitted just before the queue deadline, and reconnection to one
retained result without replay. Registry tests cover expiration before admission.
These fixtures do not qualify a slow live two-device checkpoint.

## Published-port readiness

Each published-port tunnel runs the system OpenSSH client as a foreground master
with `ExitOnForwardFailure=yes`, `ControlPersist=no`, and
`ForkAfterAuthentication=no`. Its control socket lives in a unique mode-0700
temporary directory. Silo waits for a successful `ssh -F none -S SOCKET -O check`
reply and verifies that its child still runs before exposing the local endpoint.
An unrelated listener taking the reserved port during authentication cannot
provide that reply. The control-only check uses no SSH host configuration and cannot
start a new transport or ProxyCommand when the socket is absent.

The [OpenSSH manual](https://man.openbsd.org/ssh.1) documents master control and
`-O check`; [ssh_config](https://man.openbsd.org/ssh_config.5) documents forwarding
failure and foreground lifetime options. OpenSSH 9.9p2's
[`ssh_session2` and `ssh_init_forwarding`](https://github.com/openssh/openssh-portable/blob/V_9_9_P2/ssh.c)
initialize local listeners, abort on a failed bind with `ExitOnForwardFailure`,
then serve master control requests. This uses the supported OpenSSH control
interface rather than adding a TCP relay. Supported macOS and Ubuntu systems
supply OpenSSH with these options; its BSD-licensed implementation remains the
OS vendor's maintenance responsibility. The private directory excludes other
local users; it does not protect against a process already running as the same
account. Readiness confirms forwarding setup, not guest application health.
Published ports use the same pinned guest SSH identity and bridge ProxyCommand as
remote desktops. Their `-L` destination is the guest port at the guest's current
interface address, not the owner's publication port. The owner endpoint remains a
restart/replacement marker. A guest UDP socket selects its route/source address
without sending application data; IPv4 is preferred, then IPv6. This preserves
services bound to the guest interface as well as wildcard listeners. The bounded
probe uses Python already required by Silo guest images and adds no listener or
relay. Guest transport availability remains necessary, as it is for desktops.

The owner key now uses `restrict,command=...`, without re-enabling any forwarding.
[OpenSSH key restrictions](https://man.openbsd.org/sshd.8) disable TCP and Unix-socket
forwarding together. [ProxyCommand](https://man.openbsd.org/ssh_config.5#ProxyCommand)
provides the guest SSH byte stream, and the guest server's
[pinned direct-TCP implementation](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/sdk/rust/lib/sandbox/ssh.rs)
connects inside the guest. Silo retains host-key pinning, private guest keys, and
normal bridge admission. OpenSSH remains the supported, BSD-licensed OS transport;
no custom TCP relay or owner sshd Match configuration is required.

The bridge protocol version must match exactly (version 5), so both devices must run the same Silo version. A mismatch fails the version check before key migration. The requesting device names the device to update: "Studio runs an older version of Silo. Update Silo on Studio." or "Studio runs a newer version of Silo. Update Silo on this device." (an address that is not saved yet stands in for the name). The refusal has the error code `incompatible_version` and closes every connection to that device. A device older than version 4 answers "Silo versions are incompatible. Update Silo on both computers." with the code `internal`, which Silo reads as an older version. Version 4 devices start their refusal with "Silo versions are incompatible." and say which side runs the newer version, so a version 3 requester still treats it as a refusal. The matching handshake rewrites Silo's exact
old unrestricted or forwarding-enabled lines; personal/custom key lines remain
untouched. A failed or externally managed upgrade returns a repair error.
Restrictions affect new authentications; already authenticated SSH sessions must
end before their old authority is gone.

A published-port forward opens its `guest.ssh` stream with `"purpose":"port"` and
the guest `"port"` beside `computerId`; desktop and editor streams send neither. The
owner rejects any other purpose, and a port outside 1..=65535, before spawning
the guest session. It keeps the open port streams by computer id and declared port, and
unpublishing that port, from the owner or any controller, ends the matching
streams at once instead of waiting for the controller's next poll. The label is
metadata from Silo's own controller, not a boundary inside the SSH session.

Published-port and desktop forwards share `owned_tunnel.rs`. Its watchdog shell
leads a dedicated process group and watches a pipe held by the controller. EOF
on controller crash or exit terminates the group, including ordinary
ProxyCommand descendants. Closing a tunnel also closes the pipe and performs
bounded group cleanup. Explicit group signals are sent only while the owned
leader is unreaped, preventing a reused process-group ID from being targeted.
SSH stays in the foreground; user `ControlPersist` or
`ForkAfterAuthentication` settings cannot detach this tunnel from its owner.
No process-name sweep or change to the remote bridge is involved. Subprocess
fixture tests verify controller termination, listener closure, descendant exit,
and survival of an unrelated process; real OpenSSH/ProxyCommand crash behavior
still requires separate platform qualification.

## macOS computer methods

Protocol version 5 adds these methods for macOS computers on the owner (a Mac running
Silo). A request for a computer that is not a macOS computer, or to an owner that cannot
host macOS computers, fails with the owner's message; no method changes the Linux
computer methods.

| Method | Class | Params | Result |
| --- | --- | --- | --- |
| `macos.snapshot` | read | none | the owner's macOS computers, template and minimum disk, and `supported` with `unsupportedReason` for a device that cannot host them |
| `macos.create` | change | `request`: `name`, `cpus`, `memoryGiB`, `diskGiB` | the new computer's row; installation continues in the background |
| `macos.action` | change | `computerId` (UUID), `action`: `start`, `stop`, `force-stop`, `delete`, `setup` | the owner's macOS computers afterwards |
| `macos.display.connect` | read | `computerId` | `username`, `password`, `width`, `height`; requires a running computer whose setup has finished and whose Screen Sharing answers |
| `macos.display.resize` | read | `computerId`, `widthPx`, `heightPx` | `widthPx`, `heightPx` the display took (clamped to 640x400..7680x4320, even) |
| `macos.display.stream` | stream | `computerId` | after the reply frame, raw bytes to and from port 5900 of the guest |

`macos.action` and `macos.create` follow the change rules above: one `operationId`, replay
on a lost connection, `startWithinMs`. Resize is classified as a read because it is
idempotent and a viewer sends it repeatedly while its window is dragged. The stream is
admitted against the same stream budget as `guest.ssh`, ends when either side closes or
Connections are turned off, and opens the guest connection before the reply so a refusal
(not running, setup unfinished, Screen Sharing unavailable) reaches the viewer as a
message. The owner's `runtime.logs` also answers for macOS computers: their retained
setup log is read in place of a runtime log. `macos.display.connect` returns the guest
account's password to the requesting device only for the viewer window that asked, and
neither side logs it.

## Connections settings

Connections settings commands run on Tauri's blocking pool, including configuration
lock waits, file reads, fsync writes, bridge-link setup, and control-socket setup.
The desktop viewer already runs on a blocking worker and reads saved devices through
the synchronous helper. The regression holds the configuration mutex and requires
an independent future to run before releasing it. This follows
[Tauri's async command execution](https://v2.tauri.app/develop/calling-rust/#async-commands)
and [Tokio's blocking-work boundary](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

## Validation

Commands, counts, and final build evidence are recorded after the final verification run below. Automated tests use controlled subprocesses, sockets, and runtime responses; browser inspection uses deterministic fixture data. They do not prove real two-device hypervisor operation.

No user computers were started, stopped, created, or deleted during implementation verification.

Primary-source basis: [OpenSSH](https://man.openbsd.org/ssh) documents the reused SSH transport, configuration, URIs, and forwarding.

### Final verification results

- `npm --prefix app/SiloUI test`: 77 files, 695 tests passed.
- `cargo test --offline --manifest-path app/SiloUI/src-tauri/Cargo.toml`: 327 unit tests and 5 integration tests passed; 10 pre-existing opt-in tests ignored. The initial restricted run could not bind test sockets; the successful run had local socket access.
- `npm --prefix app/SiloUI run typecheck`: passed.
- `npm --prefix app/SiloUI run lint`: passed without warnings.
- `git diff --check`: passed.
- `npm --prefix app/SiloUI run desktop:build:debug`: passed, producing the ad-hoc signed debug bundle at `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app`. No notarization or release publication was performed.

Logs are in the ignored directory `app/SiloUI/src-tauri/target/remote-verification/`: `frontend.log`, `rust.log`, `build.log`, and `lint.log`. The earlier frontend failure log was preserved as `frontend-first.log`.

Transport regression tests use real Unix sockets and child processes to check exact EOF drainage, cancellation with full pipes, and process reaping. Other regression tests cover operation non-replay, public-key installation preserving existing authorized keys, last-computer deletion, graceful shutdown including partial provisioning, duplicate computer names, cold/later local failures, scoped network mappings, and idempotent remote activity/result merging.

Browser inspection used the production React components with deterministic fixtures. Observed: two `dev` rows remained distinct; the remote computer badge exposed `Office Mac`, `Connected`, and its address on focus; `Run on` offered this device and Office Mac; the connection error exposed explicit SSH authorization and key-setup actions. The temporary fixture page, server, and tab were removed.

Remaining acceptance evidence: an installed-app run connecting two real devices, creating a disposable computer, opening an editor through SSH, interrupting the connection, reconnecting, and quitting the owning app. Neither a real two-device test nor Linux installed-app validation was performed. The built app was not launched against the user's existing computers.
