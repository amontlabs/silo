# Managed SSH access

Computer rows keep Terminal, Editor, Start/Stop, “SSH ▾”, and a more-actions menu
visible. Restart, Edit, Duplicate, and confirmed Delete are in the menu.

The computer Overview lists local and remote computers with expandable SSH
controls. Independent Local SSH and Network SSH badges remain beside the computer
status badge when collapsed; each has a small address-copy button. Enabling network
access keeps the local address available, and expanded controls show both.
Network is reserved for service port forwarding. Controls and
connection commands name the device that owns the computer. A connected Silo
controller can change access on that device; the owning Silo app runs the
listeners and stores the configuration.

## Using access

1. Expand a computer and enable **Allow SSH from DEVICE**. Silo generates a separate
   Ed25519 connection key automatically. No key setup is required.
2. The initial port is 2222, or the next available port. Edit the connection
   line using **Edit port** or **Edit address and port** in its menu to choose another port.
3. The **Copy terminal command** action in each address’s menu prepares a private key file on the device running the
   UI and copies a ready-to-run `ssh -i KEY_PATH -p PORT USER@ADDRESS` command.
   For a computer on another device, that key is this device's own; only its
   public key is sent to the owning device.
   `USER` is `silo`. Older computers require migration before SSH or editor access.
   The **Save key file** action in that menu opens a save dialog for clients that take a key file. Exported
   keys have mode 0600. The copied command's path belongs to this device;
   download the key when setting up a client on another device.
4. Enable **Allow SSH from other devices** for LAN/VPN access. Silo first asks
   for confirmation, naming the address and port that become reachable; turning
   SSH back on while it is still set to allow other devices asks the same way.
   A single available interface
   is selected automatically; choose one when the device has several. The address
   can be copied separately for clients such as ZCode.
5. The connection address tooltip includes the host-key fingerprint when available.

Network access requires an authorized key and a route to the selected address.
Silo does not configure internet routing or open a firewall. IPv6 and wildcard
listeners are not supported. Ordinary service port forwarding remains loopback
only. If the selected interface disappears, SSH closes and reports an error;
choose the new interface address to restore access.

Enabled does not mean listening. A stopped computer waits for its next start;
connections never start it. Missing keys, occupied ports, missing interfaces, and
unverifiable computer identity produce errors. Closing the window does not Quit Silo.
Disabling access, changing keys, changing the port or network address, stopping
the computer, and quitting Silo on the owning device disconnect existing managed SSH sessions. Changes restart the
whole computer endpoint, including sessions authenticated with other client keys.
Silo preserves previously authorized client keys; its generated connection key is always included when access is enabled.
Turning access off revokes every key Silo manages for that computer: its generated
connection key, which is replaced by a new one on the next enable, and the keys
other devices registered. Keys added by hand stay authorized for next time.

## Managing a computer on another device

For example, a laptop can manage a computer running on an office device. Connect
the office device using Silo's Connections setup, then expand
its computer in Overview. The controls display the office device's name,
available interface addresses and connection controls.
Updates run in the office device's Silo app.

The displayed SSH `127.0.0.1` address belongs to the office device. To connect from
the laptop, enable access on a reachable LAN or VPN address of the office
device and copy that network address or command. Managing access remotely
does not create a tunnel or make the office device's loopback address local
to the laptop.

Disconnecting the laptop, removing its saved connection, or quitting its
Silo app leaves the office device's enabled endpoint running. Its lifetime
depends on the computer and Silo app on the office device. Connections
must be available to read or change settings from the laptop; losing that
connection does not revoke the independently configured SSH access. An
unreachable or incompatible owner reports an error instead of presenting stale
settings as current. Update Silo on both devices if the owner does not support
SSH access management.

## Published websites

Network website ports use a per-computer address such as
`http://dev-1a2b3c4d.localhost:43000` in every browser, including Safari. This
keeps host-only cookies separate between computers and other local services.
For remote computers, the website address reaches the tunnel on the device
opening the browser. It does not change the SSH access addresses described above.
The website forward still binds only `127.0.0.1`. Use **Copy 127.0.0.1 address**
for development servers that reject other host names.

The browser check on 2026-09-30 used macOS 26.5 and an IPv4-only temporary HTTP
server. Safari 26.5, Chrome 154.0.8037.58 and Firefox 156.0 all reached
`silo-check-1a2b3c4d.localhost:65217` with that Host header, although the system
resolver returned `::1` before `127.0.0.1` and `::1:65217` refused connections.
In each browser, a host-only cookie set on `a-<test-id>.localhost` returned to
that host and was absent from `b-<test-id>.localhost` and `127.0.0.1` on the same
port. The test deleted its cookie and stopped its own server by recorded PID;
it left browser windows open. Raw evidence is local and ignored at
`app/SiloUI/src-tauri/target/verification/safari/` (`verify.py`, `requests.jsonl`,
`results.json`, `run.log`, `cleanup.json`). A second server check opened the exact
root URL with `open -a` in all three browsers; `root-results.json` and
`root-requests.jsonl` record those requests, and `root-cleanup.json` records its
server PID shutdown. No Silo bundle or VM was launched.
The first exact-root repeat used a single-threaded server and timed out in
Firefox after Safari and Chrome succeeded. Repeating with the threaded server
used by the original check passed all three; the failed attempt is preserved
under `root-single-threaded-attempt/` in the same evidence directory.
This check covers those browser versions on that macOS version, not Linux or
every supported macOS version.

[RFC 6761, section 6.3](https://www.rfc-editor.org/rfc/rfc6761.html#section-6.3)
reserves localhost names and their subdomains for loopback addresses. The live
check establishes the IPv4-only connection and host-only cookie behavior on
this machine. It does not prevent a page from sending requests to other local
services; open computer websites only when you trust their code.

## Ownership and persistence

`src-tauri/src/ssh_access.rs` stores configuration atomically as `ssh-access.json`
next to `computers.json`, with mode 0600. Records include the immutable computer ID,
computer name, enabled flag, port, bind address and public keys. Reconciliation
checks both saved metadata and the runtime's `silo.machine-id` ownership label;
reusing a computer name does not inherit another computer's access. The child
verifies the expected computer ID again on its captured runtime handle before
connecting, closing the name-replacement race between parent inspection and
child startup.

Generated private connection keys live in `MSB_HOME/ssh/managed-clients/COMPUTER_ID`,
separately from the internal editor key. They remain stable across port and
address changes and are deleted (and later regenerated) when access is turned
off. Export requires enabled access and a current computer identity, and only the
owning device ever reads this private key.

Settings also record the managed public key independently of those files, so
turning access off revokes an exported key even if its local key files were lost.
For older settings without that record, Silo recovers the public identity from
the private key or its `.pub` file. If both are unavailable, disabling reports an
error instead of retaining an unidentified managed authorization. Restore
`MSB_HOME/ssh/managed-clients/COMPUTER_ID.pub` and retry.

Port, address, and access-toggle saves omit the authorized-key list. The owning
device uses its latest saved keys while holding the operation gate, so an
older UI snapshot cannot revoke a newly registered controller. Disabling still
revokes managed keys and retains user-added keys. An explicit key list replaces
the authorization set; an empty list removes user and controller keys. Update
both devices before using this save behavior with an older owner.

External authorized keys live in `MSB_HOME/ssh/managed-access/COMPUTER_ID.authorized_keys`.
Neither `MSB_HOME/ssh/authorized_keys` nor Silo's internal client private key is
read or modified by this feature. Keys are structurally validated as Ed25519
OpenSSH public keys; duplicate identities with different comments are rejected.
The existing computer `ssh/host_ed25519` host key is reused, with shared editor
helpers for private files and OpenSSH fingerprint generation.

Each endpoint owns a `Child` running:

```text
msb ssh serve NAME --no-start --no-inactivity-timeout \
  --exit-on-stdin-close --authorized-keys ISOLATED_STORE \
  --expected-machine-id COMPUTER_ID --host EXACT_IPV4 --port PORT
```

The parent retains the child's stdin writer. EOF ends the child even when Silo
crashes and cannot run Rust `Drop`. Normal teardown kills and waits for each
owned child. No PID file, guessed PID cleanup, or surviving detached listener is
used. A network-enabled computer owns two children: loopback and the selected
interface, on the same port. Both must bind successfully; partial startup is
rolled back. `SILO_SSH_READY` is flushed only after authorization preparation and
successful bind, so an unrelated process on that port cannot be mistaken for
Silo's listener.

The bundled MicroSandbox patch exposes the existing authorized-key-path option
on the CLI, ties TCP serving to owner-pipe EOF, and observes the original guest
agent transport's closure. Losing that transport ends the listener and active
sessions without reconnecting to a replacement computer. `--no-start` refuses a stopped
computer, and `--no-inactivity-timeout` disables the existing SSH inactivity timer.

## Container computers

A container computer ([threat model](THREAT-MODEL.md#container-computers)) has the same SSH access,
editor connection and terminal as a virtual machine, through the same commands and the same
screens. It has no `sshd` either: the callers above start `<runtime executable> ssh serve NAME ...`,
and for a container the executable is `silod --container-runtime` instead of `msb`
(`runtime::command_for` picks it, so no caller branches on the kind). The flags are the same
(`--stdio`, `--no-start`, `--no-inactivity-timeout`, `--exit-on-stdin-close`, `--authorized-keys`,
`--expected-machine-id`, `--host`, `--port`) and so is the readiness line, `SILO_SSH_READY`.

The server is `engine/src/containers/ssh.rs`, built on the [russh](https://github.com/Eugeny/russh)
library MicroSandbox's own server uses (Apache-2.0; version 0.65.1 is pinned, with the ring backend
and no RSA support). It is not MicroSandbox's server because that one is written against its
sandbox and agent types and cannot take another backend; a backend trait upstream would let both
kinds share it, and that is the better end state. What it does:

| SSH request | Inside the computer |
| --- | --- |
| public-key login | the authorized-keys file the caller names (`MSB_HOME/ssh/authorized_keys` when none), read at each attempt; the login name is `silo` |
| shell | `/bin/bash -l` as `silo`, in its home |
| exec | `/bin/bash -c COMMAND` |
| `pty-req` | a pseudo-terminal made by `crun`; the window-change request resizes it |
| `sftp` subsystem | `/usr/lib/openssh/sftp-server`, from the image's `openssh-sftp-server` (it is in the guest image already) |
| `direct-tcpip` (`ssh -L`, `ssh -D`, the editor's port forwarding) | a connection from inside the container's network namespace (`silo-ctr connect`) to the container's own loopback: `localhost`, `127.0.0.0/8` or `::1`, ports 1 to 65535. Any other destination is refused (`administratively prohibited`), so a forward never reaches the host's network or the TAP's |
| `signal` | sent to the command's process group |
| anything else (environment, agent, X11, remote forwarding (`-R`), other subsystems) | refused |

The SSH login `silo` must be the working account the container's id mapping gives the engine's
user (id 1001): the server resolves the name in the computer's own account files, and refuses the
login if the id is anything else, root's included. The check is made at authentication, so a
refused login opens no command, terminal or port forward; the reason is in the engine's log. A
forward also checks that the computer's holder is alive before it joins the container's network
namespace, so a stale `outer.pid` that another computer's holder has reused is refused.

A connection has 30 seconds to log in whatever the inactivity timeout is, and only logged-in
connections count against the limit of 32 (at most 64 are still logging in at once), so idle
sockets cannot lock a person out. A command is never ended because it reads slowly. The
client's input is flow-controlled with the SSH channel window, as OpenSSH does: each channel
grants the client 2 MiB, and the server gives window back only as the command takes the input,
so a slow command slows the client down (`rsync`, `git` and a large `scp` just take longer)
while the server holds at most 2 MiB per channel, 32 MiB across the 16 channels of one
connection. The session loop never waits for the command, so a transfer in both directions
(`ssh NAME cat < big > copy`) never stalls the connection's other traffic. A client that sends
more than its window is out of protocol and its channel is closed. When a channel closes, its
writer task ends, even for a command in a process group of its own that never reads. russh
grants the window by itself as data arrives, so Silo carries a small patch to it (see
`app/SiloUI/src-tauri/vendor/russh/SILO-PATCH.md`).

Every command runs through the engine's own exec path (`silo-ctr enter`, `crun exec`, the init
shim), so the hardening baseline, the process-group kill when a channel closes, and the
placeholder environment of the computer's secrets apply to it. A terminal opened from Silo runs
`silod --container-runtime exec NAME --tty ...`, which runs the same `crun exec` on the user's own
terminal. Host-key pinning, the per-computer `known_hosts` and the editor's configuration are the
same files as for a virtual machine.

The live check (`scripts/linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1`) runs `silo ssh-setup`, then
`ssh NAME.silo-dev uname -a`, an SFTP put and get, a `ssh -tt ... tty` session, the terminal
command, the editor proxy in `--stdio` mode under a scripted `ssh`, port forwarding (`ssh -L` to a port served
inside the container, and a refused destination), a large file through `ssh NAME cat` both ways (hashes compared), a
killed client that takes its remote command with it, idle sockets that do not lock out a login, and the in-guest client.

## Reconciliation

Silo reconciles after normal starts, restarts, temporary `exec` starts/stops,
start-at-launch, deletes, and each restart in backup's separate stop/restart path. Stop/restart/
remove close managed SSH first. Quit closes listeners under the runtime mutation
lock before stopping local computers; the shutdown admission flag prevents reopening.
Final app Exit also closes listeners.

A background monitor reconciles every 15 seconds when it can obtain the runtime
mutation lock, and only while SSH access is enabled for some computer or Silo
still owns a listener or error; otherwise it takes no lock and inspects nothing.
Runtime inspection and listener start-up run outside the lock that guards the
listener table, so SSH status reads and closes never wait for them. This covers app startup with already-running computers, external stops,
process failures, interface loss, and recovery from occupied ports. The child
itself observes computer transport closure, independently of that polling interval.
Corrupt or unreadable saved state closes all managed SSH listeners. Configuration
commands share the runtime mutation lock with lifecycle changes.

`src-tauri/src/remote_ssh_access.rs` carries `ssh.access.state` and
`ssh.access.save` requests through the existing Connections bridge.
Requests pin the immutable owning-device ID and computer ID. The owner resolves the
computer ID and reads or changes SSH settings under its runtime mutation lock,
using the same persistence, validation, and reconciliation as local controls.
State, save and connection responses contain public keys only. For an explicit
`ssh.access.connection` request the controlling Silo app creates its own
Ed25519 key under `MSB_HOME/ssh/remote-clients/DEVICE_ID-COMPUTER_ID` (mode 0600)
and sends only the public key with its own Silo identity. The owner authorizes
it for that computer with the comment `silo-controller:CONTROLLER_ID`, replacing
that controller's previous key, and returns the port, address and account. The
controller then returns a command, or exports its own key using a native save
dialog. The private key never enters frontend state or crosses devices.
Removing the device deletes the controller's keys for it; turning
access off on the owner revokes every controller key. An owner or controller
from before this change is refused with an "Update Silo" message instead
of sharing the owner's key. Keys that older versions copied to controllers
(`MSB_HOME/ssh/connections/DEVICE_ID-COMPUTER_ID`) are deleted on the next
connection; the owner stops accepting them once access is turned off once.
Connections must be enabled and both immutable IDs must match. The
controller creates no SSH listener process.

## Verification

Ordinary Rust tests use a disposable TCP echo runtime, not a computer. They check
actual socket/session teardown, owner-pipe EOF, readiness under port collision,
key-change restart, reuse of unchanged listeners, rejection of stopped/disabled/
unverified/keyless configurations, interface validation, private settings,
symlink rejection, duplicate keys, and preservation of internal authorization.
Frontend fixtures cover collapsed badges, the named device, both connection
commands, switches, port and interface validation, key paste/import/removal,
errors, and stale status. Existing lifecycle, backup and shutdown tests remain
part of the ordinary suite. Remote regression coverage should verify device and
computer identity rejection, owner-name/address preservation, remote save routing,
errors from unavailable or older owners, and independence from controller
disconnect. These deterministic checks do not establish reachability between
two physical devices.

Run from the repository root, using the native test configuration described in
[Silo releases](SiloUI-RELEASES.md#local-setup):

```sh
cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml ssh_access::tests
npm --prefix app/SiloUI test -- src/features/application/pages/ssh-access-panel.test.tsx
npm --prefix app/SiloUI run typecheck
npm --prefix app/SiloUI run lint
```

For opt-in live verification, `scripts/test-managed-ssh-live.py` creates a
disposable computer in its own temporary `MSB_HOME`, using the bundled guest image.
It does not use the installed application's computer state. Supply a patched runtime
that can run computers on the device (signed with the required entitlement on macOS):

```sh
SILO_RUN_MANAGED_SSH_LIVE=1 python3 app/SiloUI/scripts/test-managed-ssh-live.py \
  --msb /absolute/path/to/msb --library /absolute/path/to/libkrunfw \
  --guest-image /absolute/path/to/guest-image \
  --output app/SiloUI/src-tauri/target/verification/managed-ssh-live
```

Add `--network-address DEVICE_IPV4` to verify simultaneous loopback and selected
interface listeners. The test checks authorized and rejected keys, host-key
pinning, 65 seconds of idleness with keepalives disabled, ownership-pipe EOF,
key replacement, the stopped-computer guard, identity pinning, and computer-stop cleanup.
This exercises the runtime on one device; it does not prove firewall reachability
from a second physical device.

Loopback tests require permission to bind local TCP sockets. Runtime CLI and
agent transport tests are also included in the maintained MicroSandbox patch.
A fixture test or successful build does not prove a live computer, a second physical
device, or an installed application's behavior.

## Primary implementation sources

The maintained patch applies to MicroSandbox commit
`5eca4de8bf233e57f114140f8c076ea8c96f21ab`. Relevant upstream source:

- [SSH CLI](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/crates/cli/lib/commands/ssh.rs): TCP/stdio serving and inactivity flags.
- [SSH SDK](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/sandbox/ssh.rs): isolated authorized-key store and persistent sandbox host key.

Repository patterns reused: `src-tauri/src/editor.rs`, `network.rs`,
`remote_network.rs`, and `runtime/shutdown.rs`. The runtime preparation script
verifies the additional CLI capabilities before accepting a bundled runtime.

### Verified on 2026-09-15

The results below describe the initial local-owner implementation. They do not
establish live remote-controller behavior; record remote verification separately.

The full frontend and ordinary Rust suites passed. Final focused checks passed
11 SSH UI tests and 9 managed-SSH Rust tests. Typecheck, lint, 31 release-tooling
tests, 12 runtime-preparation tests, the packaged patch hash check, and patched
runtime tests (7 SSH CLI, 7 SSH SDK, 9 agent-client) also passed.

The opt-in live test passed against a freshly compiled, ad-hoc hypervisor-signed
runtime at `app/SiloUI/src-tauri/target/verification/managed-ssh-runtime/msb`.
It used a disposable VM and verified all cases above, including simultaneous
loopback and the host's selected LAN address. Evidence is in
`app/SiloUI/src-tauri/target/verification/managed-ssh-runtime/live-final/live.log`.
The test VM was stopped and its isolated home removed. The UI was visually
checked with deterministic fixtures. No installed Silo bundle was rebuilt or
inspected, and no second physical computer was used.

### Remote-control verification

After adding Connections, the ordinary Rust suite passed 371 tests with
10 opt-in tests ignored. Focused native tests exercise the same owner-side
settings handler with a disposable TCP runtime: remote enable persists without
starting a stopped computer, the owner restores access after start, returning the
controller reply leaves its listener alive, identical settings preserve active
sessions, and remote disable closes listener and session. Replaced identities,
malformed settings, wildcard binding, private-key input, disabled management,
wrong device identity, and incompatible protocol versions are rejected.

The full frontend suite passed 785 tests across 86 files, and typecheck and lint
passed. Frontend regressions cover immutable device/computer routing, aggregation across owners,
healthy-device isolation from offline devices, stale-key response suppression,
unsupported owner versions, disabled stale controls, main-window permissions,
and owner-specific address labels. These are deterministic controller/owner
fixtures, not a live test between two physical devices. The remote-control
extension does not change the previously live-tested MicroSandbox runtime.

### Compact controls verification

The compact layout passed typecheck, lint, and 19 focused SSH/copy-button tests.
The macOS debug bundle was rebuilt, its signature verified, and the exact app at
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app` reopened. Native UI
inspection confirmed the disabled computer's expanded row shows only its named
SSH toggle and collapsed Keys/Advanced sections. No access settings were changed.
The local computer was stopped before restart; the remote Linux computer remained
running. Linux's updated frontend also built, but its final native build still
requires authorization to transfer the private GitHub build configuration.
