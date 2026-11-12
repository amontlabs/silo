# Silo engine extraction

The Cargo workspace is rooted at `app/SiloUI/Cargo.toml`, with its lockfile at
`app/SiloUI/Cargo.lock`. The Tauri application remains the `silo-ui` package
(`app/SiloUI/src-tauri`); the Tauri-free library is the workspace member
`app/SiloUI/src-tauri/engine` (`silo-engine`). The other members are `silo-api`
(`app/SiloUI/api`), `silo-client` (`app/SiloUI/client`), `silo-host`
(`app/SiloUI/host`) and `silod` (`app/SiloUI/src-tauri/silod`), so `silod`
depends on the engine without depending on Tauri.

`silo-engine` owns channel identity and defines `EngineContext`, `EnginePaths`,
`EngineIdentity`, `Event`, `Audience`, `MainThread` and `ProcessControl`. The Silo
UI's `TauriEngineContext` supplies those capabilities. Backend module groups
move into the library incrementally; commands remain in `silo-ui` as adapters
until their group is extracted.

## Current extraction state

The build channel, runtime operation gate (including its process-wide
`OPERATIONS` instance), and image-cache repair logic now live in `silo-engine`.
`silo-ui` re-exports the gate and image-cache functions for backend modules that
have not moved yet. Every workspace member is a Cargo default member, so
`cargo test --locked` from `app/SiloUI` covers the engine tests.

## macOS VM window host seam

The macOS VM host must not depend on a Tauri webview. When the VZ display code is
extracted, keep VM creation and `VZVirtualMachineView` attachment in the engine,
and expose the attach operation through a host seam that accepts the native
`NSWindow` supplied by the process hosting the engine. The operation must use
`EngineContext::main_thread()` for AppKit work. `silod` can then supply its own
window while Fjord and Silo UI hosts retain their own window policy. The current
window creation remains in `silo-ui` until that extraction step.

## Silo.app is a client of `silod`

One engine runs per Mac and channel (95 A-1). Silo.app does not construct one: it keeps a
connection to `~/.silo/engine.sock` (`~/.silo-dev` for Silo Dev) and every Tauri command that
used to call the engine in-process forwards to the canonical `silod` method over that socket.

- **Crates.** `app/SiloUI/client` (`silo-client`) is the thin native client: framing, the
  handshake, request correlation by id and the session event feed. It depends on `silo-api`
  only. `silo-ui` uses it from `src-tauri/src/engine_client/`.
- **Launch.** `engine_client::install` runs in the Tauri setup. The launcher uses whatever
  engine already serves the socket, whichever app supplied it. With none, it registers the
  LaunchAgent that `Silo.app/Contents/Library/LaunchAgents/<identifier>.engine.plist`
  describes (`SMAppService`, from the calling bundle) and waits for the socket. If the agent
  needs approval it opens Login Items and says so. The agent restarts only after a crash
  (`KeepAlive` on an unsuccessful exit), so a job that is loaded but idle after a clean exit is
  started with `launchctl kickstart` (a no-op for a running job). When the engine becomes
  reachable after the app started, the frontend runs the failed dependency checks and the
  application load again (`silo://engine-connection`). The connection is kept for the life of the
  app: it reconnects with backoff, resubscribes, and reconnects before the engine's per-connection
  request-id limit.
- **Commands.** Command names, argument names and result shapes are unchanged, so the React
  UI is unchanged. Each command is a small inner `async fn` over `&EngineClient` plus a thin
  `#[tauri::command]` wrapper, and has a forwarding test against `engine_client::testing::FakeEngine`,
  an in-process fake socket server. A canonical job is awaited (`EngineClient::run_job`), then
  acknowledged. Losing the connection while waiting never turns into a failed job. A method the
  engine does not implement, or a request for another device, comes back as the engine's typed
  `unsupported_remote_operation` error and reaches the UI as the unavailable state; nothing falls
  back to in-process work.
- **Events.** Every frame is relayed as `silo://engine-event`. `engine_client/relay.rs` also
  produces the established `silo://` events the UI listens to.
- **Windows.** Dialogs, windows, the tray, menus, the Linux desktop viewer shells (a guest webview
  that the engine's `display.viewer.*` feeds) and launching the chosen terminal and editor stay in
  Tauri. The macOS VM window belongs to `silod` (`display.show`).
- **Quit** (95 A-2). With computers running the user chooses Cancel, Keep running, or Stop and quit.
  Only Stop and quit asks the engine to stop them (`engine.shutdown`). Quitting, updating or
  crashing the app never stops computers.
- **Boundary.** `src-tauri/src/engine_boundary_tests.rs` fails if the crate starts constructing
  the engine, calls engine operations in-process, or depends on the engine's runtime crates.
  `silo-ui` does not link `silo-engine`, directly or transitively. The stateless host helpers
  (`applications`, `notifications`, `system_integrations`, `system_shutdown`, `terminal`, the
  channel and the installer lock) live in the `silo-host` crate (`app/SiloUI/host`), which both
  `silo-ui` and `silo-engine` depend on.
- **Packaging.** `scripts/embed_engine.py` embeds `Silo Engine.app` (the signed `silod`, its
  manifest and plist) under `Contents/Resources/SiloEngineDistribution/` and copies the agent plist
  into `Contents/Library/LaunchAgents/`. `silod` resolves its tools from its own bundle, so the
  engine also carries copies of the app's signed `msb`, `libkrunfw.5.dylib`, Git tools,
  `silo-macos-gateway` and `silo-network-bootstrap` (`Contents/MacOS`, `Contents/Frameworks`) and
  the `microsandbox/` and `git-support/` resources. `scripts/build_desktop.py` (both
  `desktop:build` and `desktop:build:debug`) and `scripts/package-macos-release.py` sign the
  tools, embed the engine, then re-seal the app. Fjord's embedded engine (from the engine release
  tarball, which holds only `silod`) still lacks these tools.
  It also writes `SiloEngineCodeRequirement`, the signer requirement `silod` checks at run time, into the
  engine's `Info.plist` before signing ([engine releases](SiloUI-RELEASES.md#engine-releases)).

## Signing the dev silod with a stable identity

`silod` is sealed ad hoc (`codesign --sign -`) by default. An ad hoc signature is different
for every build, so a Keychain item whose access list names the previous `silod` build refuses
the next one. The engine never waits for the "wants to use your confidential information"
dialog (it forbids Keychain UI for the whole process): the call fails and the client gets the
`credential_access_denied` error ("Allow Silo's engine to use its saved sign-in"). Unattended
work (launch migration, timers, startup) skips and retries later.

To make approvals survive rebuilds, sign the dev engine with a stable identity.

1. Create a self-signed code-signing certificate once: Keychain Access, Certificate Assistant,
   Create a Certificate, name `Silo Dev Signing`, identity type Self Signed Root, certificate
   type Code Signing. (Or use an Apple Development certificate.)
2. Check it: `security find-identity -v -p codesigning`.
3. Build with the identity in the environment:

   ```sh
   export SILO_ENGINE_SIGNING_IDENTITY="Silo Dev Signing"
   npm --prefix app/SiloUI run desktop:build:debug
   ```

   `scripts/embed_engine.py` passes it to `codesign` for the nested `Silo Engine.app`
   (hardened runtime and the virtualization entitlement are unchanged). Unset, it stays `-`.
4. The first time the engine hits an existing item, approve it once in Silo (or in Keychain
   Access, Get Info, Access Control, add `Silo Engine`), and choose Always Allow. The next
   build with the same identity matches the same designated requirement and is not asked again.
   (Correction from the live run: without a Team ID each item's partition list names the
   creating build's cdhash, so every rebuild is refused again. See the user-started repair
   below.)

**Keychain access repair.** `silod` never lets the Keychain prompt, so after an engine update
the items an earlier build made fail with `credential_access_denied`. `engine.credentials.status`
lists the engine's items (GitHub sign-in, token ledger, personal token, secrets vault, one proxy
CA per macOS computer) as accessible, denied or failed. `engine.credentials.repair` is the only
place prompts are allowed: a client calls it only from a user click; it allows Keychain UI for
that one call on its own thread, touches each denied item so macOS shows its dialog (choose
Always Allow), and forbids prompts again before it returns, even on an error or a panic. The
switch is process-wide, so unattended Keychain calls that overlap the call could also prompt;
one repair runs at a time.

The cargo-built `silod` used by tests and the hand-assembled bundles of live runs must be
sealed the same way: `codesign --force --sign "$SILO_ENGINE_SIGNING_IDENTITY" --options runtime
--entitlements app/SiloUI/src-tauri/silod/macos/Entitlements.plist <Silo Engine.app>`.

The engine bundle's `Info.plist` has no `LSUIElement`: `silod` sets its own activation policy
(accessory, regular while a VM window is open), and AppKit refuses that call for a bundle that
is already an agent app. If AppKit still refuses the accessory policy, `silod` logs it and runs
on.

## `silod` on a Linux host

`silod` also builds and runs as a headless daemon on Linux (x86_64 and aarch64), where it runs
Linux computers over KVM. The Silo app stays Mac-only; a Linux host is managed from another
device's engine through Connections. The macOS code paths are unchanged: everything below is
behind `cfg(target_os = "linux")`.

- **Build.** `cargo build -p silod` (from `app/SiloUI`, toolchain 1.95.0). It needs
  `pkg-config` and `libdbus-1-dev` (for keyring's optional Secret Service backend) but no
  GTK: the engine depends on `silo-host` without its `desktop` feature, which holds the GIO
  and GTK integrations the app uses (application discovery, autostart, notifications). Without
  it, application discovery lists nothing and system integrations read as unavailable. CI
  checks that `cargo tree -p silod -e normal` lists no GTK crate.
- **Layout.** A Linux engine is one directory: `<dir>/silod` with the `silo` command and the
  bundled tools beside it (`msb`, `git`, `git-lfs`, `git-remote-http`, `git-remote-https`), and
  `<dir>/resources/` with `engine-manifest.json` and the runtime resources:
  `microsandbox/manifest.json`, `microsandbox/<target>/lib/libkrunfw.so.5.6.1`,
  `microsandbox/licenses/`, and `git-support/` (the Git manifest, templates and licenses,
  `ssl/cacert.pem`, the certificate bundle the Linux Git build verifies HTTPS with, and the
  guest's `lfs-transfer/git-lfs-transfer`). `paths::default_resource_dir` resolves
  `<dir>/resources` from the running executable, so a link such as
  `~/.local/bin/silo-remote-dev` finds the same directory; `bundled_tools::resolve` is the
  executable's directory, where the runtime finds `msb` and Git. The release archives
  `silo-engine-<version>-linux-x64.tar.gz` and `-linux-arm64.tar.gz` hold exactly this
  directory as `silo-engine/` (see [Engine releases](SiloUI-RELEASES.md#engine-releases)).
- **CI runner.** The archive also holds `silo-runner`, the helper that makes the host run GitHub
  Actions jobs in a fresh computer each; it is its own service, registered with
  `silo-runner service register` and started after the engine's unit
  ([CI runner, Linux hosts](SiloUI-CI-RUNNER.md#linux-hosts)). The engine ignores it.
- **Identity.** The channel comes from `resources/engine-manifest.json`, validated as strictly
  as a bundle's (regular file under 64 KiB, known channel, semver version, non-empty build,
  protocol range) except for the bundle identifier, which a Linux engine has none of. There is
  no code-signature check on Linux: nothing seals the directory yet. Signed Linux archives
  (for example Sigstore) are roadmap milestone 11.
- **Account and directory checks.** Before it reads anything from its directory, every entry
  point (`--agent`, `--remote-bridge`, `--remote-guest`) refuses to run as root or `nobody`
  (use a dedicated account in the `kvm` group) and checks the engine directory
  (`silod::install_trust`): every directory from `/` down to it, `silod`, every entry beside it
  and everything under `resources/` must belong to this account or root and be writable by
  nobody else (a sticky `/tmp` is refused too), and a link must stay inside the directory.
  Write access for the owner's user-private group is allowed, the rule Debian's OpenSSH applies
  for `StrictModes`: the group is the owner's primary group and lists no member but the owner
  (`silo_engine::safe_directories`). Silo adds that the owner is not root and the group not
  gid 0, that only local accounts count (the owner's `/etc/passwd` line and the group's single
  `/etc/group` line must agree with `getpwuid_r` and `getgrgid_r`, so an LDAP or SSSD account
  is refused), and that no other `/etc/passwd` account has the group as its primary group. The
  files are parsed directly, since `getpwent` is not thread-safe; an unreadable file, or one over
  16 MiB, refuses. The refusal names the path, its mode
  and group, and the `chmod g-w` or `chmod go-w` that fixes it. Before its first `msb` launch, the agent checks
  `resources/microsandbox/manifest.json` against the runtime inputs compiled into the build and
  the SHA-256 of `msb` and `libkrunfw` against it
  (`silo_engine::dependencies::verify_bundled_runtime`); an engine without the runtime starts
  and reports that computers cannot start.
- **Paths.** Data and configuration: `$XDG_DATA_HOME/<identifier>` (default
  `~/.local/share/org.silo.dev` for Dev). Cache: `$XDG_CACHE_HOME/<identifier>`. Logs:
  `$XDG_STATE_HOME/<identifier>/logs`. A relative or empty XDG value is ignored, as the
  specification says. The private state directory and the engine socket stay where every
  client already looks on both platforms, `~/.silo` and `~/.silo-dev` (`engine.sock`,
  `engine.lock`, the owner record, Connections state), so `silo-host`, `silo-client` and the
  CLI need no Linux-specific socket path and the two channels stay apart.
- **Peer check.** The socket accepts only clients of the engine's own account. On Linux the
  check reads `SO_PEERCRED`, the effective UID the client had when it connected, which is what
  `getpeereid` returns on macOS.
- **Credentials.** The engine's `keyring` entries (GitHub sign-in, token ledger, personal
  token, secret values) go to a file store, `silo_engine::credential_file_store`, under
  `<data>/credentials/` (see [Secrets](SiloUI-SECRETS.md#linux-hosts)).
  `SILO_CREDENTIAL_STORE=secret-service` uses the desktop Secret Service instead; any other
  value except `file` stops startup. The first start records the store in
  `<data>/credential-store`, and a later start with a different value stops with a message
  naming that record.
- **Running it.** `silod --agent` (or no argument) serves until it is stopped. The process main
  thread runs the queue that `MainThread` work goes to, in place of the AppKit loop. SIGTERM
  and SIGINT both start the engine's bounded session-end shutdown, as SIGTERM and logout do
  on macOS; a second one of either exits at once with status 1. `msb` and guest commands run in
  their own process group, so a terminal's Ctrl-C reaches `silod` alone. Exit code 75 still
  means "restart me" for a supervisor. `--remote-bridge` and `--remote-guest` work through the
  `~/.local/bin` link. Run the engine as a dedicated account (in the `kvm` group to start
  computers), installed where only that account can change it.
- **Install from an archive.** Unpack the archive in a directory the account owns, for example
  `~/.local/opt`, check it first, and register the service:

  ```sh
  sha256sum -c silo-engine-<version>-linux-x64.tar.gz.sha256
  mkdir -p ~/.local/opt && tar -xzf silo-engine-<version>-linux-x64.tar.gz -C ~/.local/opt
  chmod -R go-w ~/.local/opt/silo-engine
  ~/.local/opt/silo-engine/silo host init
  silo list
  ```

  `silo host init` ([CLI guide](SiloUI-CLI.md#hosting-computers-for-other-devices)) checks
  `/dev/kvm` and the account, runs `silod --service register` (below) from the `silod` beside it,
  links `~/.local/bin/silo` to the archive's `silo` when nothing else is there, turns Connections
  on, accepts the controller keys given with `--authorize`, and prints the address another
  device adds this host by. `silod --service register` alone still installs just the service.

  The archive is not signed (roadmap milestone 11); the `.sha256` beside it on the release
  only shows that the download is whole. The host needs `/dev/kvm` readable and writable by the
  account (usually through the `kvm` group), `ssh` for Connections, and network access for the
  guest image the engine downloads on first use. The archive's `silo` is the production
  channel's and talks to `~/.silo/engine.sock` (a Dev archive's, `~/.silo-dev`, linked as
  `silo-dev`); `silo host init` puts it on `PATH`.
- **Service.** `silod --service register` writes the systemd user unit
  `~/.config/systemd/user/<channel>.engine.service` (`org.silo.preview.engine.service`, or
  `org.silo.dev.engine.service` for Silo Dev; `$XDG_CONFIG_HOME` is honored), then runs
  `systemctl --user daemon-reload` and `systemctl --user enable --now` on it. The unit starts
  the canonical path of the `silod` that registered it, with `--agent`, and sets
  `Restart=on-failure` (exit code 75 is a failure to systemd, so a requested restart starts a
  new engine, and a clean exit stays stopped), `RestartSec=10`, `StartLimitIntervalSec=0` in
  `[Unit]`, `UMask=0077`, `KillMode=process` and `KillSignal=SIGTERM` (only `silod` is
  signalled: its session end stops its computers, and computers it leaves running on purpose,
  for an update, are not killed), `TimeoutStopSec=30`, and `WantedBy=default.target`.
  `StartLimitIntervalSec=0` turns systemd's start rate limit off, so an engine that keeps
  failing is restarted every `RestartSec` for as long as it fails rather than left stopped;
  the trade-off is that a crash loop keeps running and fills the journal until someone stops
  the unit. `register` never replaces a file at that path that silod did not write (the first
  line marks silod's), and leaves an engine that is already running as it is: after
  registering from another directory it says to run `systemctl --user restart <unit>`.
  `ExecStart` names the canonical path, with links resolved, so an upgrade that installs a new
  directory and flips a link such as `~/.local/opt/silo-engine` to it keeps starting the old
  `silod`: run the new one's `silod --service register` (then the restart it asks for).
  `silod --service unregister` runs `disable --now` (which waits for the engine's session end),
  then names any process of the engine's computers still left in the unit's cgroup, removes the
  file and reloads. `silod --service status`
  prints, as on macOS, `enabled`, `disabled` or `notRegistered` on its first line, then
  `state:` (the unit's active state), `unit:` (its path), `program:` (whether it starts this
  `silod`) and `linger:`. `systemctl --user` needs the account's user manager: run the commands
  from a login session, not through `su` or `sudo`, or set
  `XDG_RUNTIME_DIR=/run/user/$(id -u)`. The engine's output goes to the journal
  (`journalctl --user -u org.silo.preview.engine.service`) and its logs to the XDG log
  directory above. The journal is also readable by the host's administrators (root and,
  depending on the distribution, members of groups such as `adm` or `systemd-journal`), so
  what silod writes there is not private to the account; the threat model for a Linux host
  must account for that.
- **One engine per channel.** The unit's engine takes the channel's ownership lock like any
  other. If an engine of the same channel already runs outside systemd (started by hand from a
  terminal, or from your own unit), the unit's engine defers to it and exits 0 (the journal
  says `An engine already owns this user/channel`), so systemd
  shows the unit as inactive rather than failed and does not restart it. Stop the hand-started
  engine first, then `systemctl --user start <unit>` (or `silod --service register` again).
- **Session end.** Stopping the unit (`systemctl --user stop`, `unregister`, the user
  manager ending) sends SIGTERM to `silod`, which stops this device's computers within a
  bounded budget (`SESSION_END_BUDGET`) and exits. The computers run in their own processes,
  and `KillMode=process` leaves them alone, so a computer that does not stop within the budget
  keeps running after `silod` exits. The engine then checks what the runtime still lists as
  running; when anything is (or it cannot tell), it logs the names to the journal and exits
  with code 70 instead of 0, so the unit's stop is recorded as failed. systemd never restarts
  a unit whose stop it was asked for; after a SIGTERM from anywhere else, `Restart=on-failure`
  starts a new engine. `unregister` names the surviving processes from the unit's cgroup.
  Starting the engine again (`systemctl --user start <unit>`, `silod --service register`, or
  `silod --agent`) takes the surviving computers back, and they can then be stopped from it.
  If the session end itself overruns `TimeoutStopSec`, systemd's SIGKILL reaches `silod` only,
  by design (`KillMode=process`): the computers are left running as above, not killed by the
  unit's stop. A prepared engine update is the exception: its saved computers are released on
  purpose, the exit is 0, and the next engine resumes them.
- **Lingering.** Without it, systemd starts the account's user manager, and so the engine, at
  the account's first login and stops it after the last logout. The last logout ends the user
  manager itself, and with it everything still running under it: systemd's own
  `user@.service` uses `KillMode=mixed`, which signals every process left in the manager's
  cgroup, so a computer that outlived the engine's session end is killed then, not just
  detached. Distributions can change that unit; check yours
  (`systemctl cat user@.service`). On a headless host, turn lingering on so the engine and its
  computers start at boot and keep running without a session. That needs an administrator;
  silod never asks for privileges, it reports `linger: no` and says how (it names the account
  from the user database, not from `$USER`):

  ```sh
  sudo loginctl enable-linger "$(id -un)"
  ```

- **Trying it by hand.** To run a Dev engine in a test home without installing it, assemble
  the directory by hand:

  ```sh
  mkdir -p ~/silo-engine/resources
  cp target/debug/silod ~/silo-engine/
  cp src-tauri/silod/macos/resources/engine-manifest.development.json \
    ~/silo-engine/resources/engine-manifest.json
  chmod -R go-w ~/silo-engine
  ~/silo-engine/silod --agent &
  target/debug/silo list
  ```

  A root-owned `/opt/silo-engine` works the same way when `/opt` itself cannot be changed by
  other accounts: some images, GitHub's runners among them, make `/opt` world-writable, and then
  silod refuses the engine under it. An engine under `/tmp`, or in a directory
  another account can write to, does not start. Group write is allowed only for the owner's
  user-private group, so a home made under a umask of `002` on Debian or Ubuntu works, while a
  shared group (Debian's group-writable `/usr/local`, group `staff`) is refused.

  This proves the daemon serves; creating or starting a computer also needs `/dev/kvm`, the
  runtime resources above and the guest image. After `npm run runtime:prepare`,
  `python3 scripts/package-engine.py --platform linux-x64 --channel development` builds a
  complete Dev engine archive instead.
- **CI.** `.github/workflows/linux-host.yml` builds that Dev archive on an `ubuntu-24.04`
  runner, installs it, registers it as a user service of the lingering runner account, and
  drives a Linux computer over KVM through the `silo` command
  (`app/SiloUI/scripts/linux-host-e2e.sh`): create, start, exec, checkpoint, fork, exec in the
  fork, stop and drop both. After the checkpoint, which is saved while the computer runs, it
  checks that no export holds memory: exporting that checkpoint must be refused, and the export
  of the running computer and of a fork of the checkpoint are opened and must hold disk captures
  only. It then stops the service, expects exit code 0 (a session end that
  left a computer running fails it), and unregisters it. After a failed step it still stops
  the engine, and keeps the unit for the logs it uploads. Then two new accounts on the same
  runner act as two devices (`app/SiloUI/scripts/hosts-e2e.sh`): `silo-host` runs
  `silo host init --authorize` from a root-owned copy of the engine (under
  `/var/lib/silo-ci`, root-owned 0755: the runner's `/opt` is world-writable), and `silo-controller`, with
  its own engine, adds it with `silo devices add silo-host@127.0.0.1:22 --host-key <fingerprint>`
  over the runner's SSH server, and creates, starts, stops and drops an `e2e-hosts-*` computer on
  it with `--device` before removing it.
  Before the computer steps, `scripts/runner-host-e2e.sh` runs the archive's `silo-runner` against
  the same engine and systemd user manager, without GitHub: `check -offline` on `/dev/kvm`,
  `service register` (a unit ordered after the engine's and starting it when it is down), the
  helper's log in the user journal, and `service unregister`.

### Container computers (optional)

A Linux host can also run container computers: rootless `crun` containers instead of VMs. The
host must be set up for them, and Silo never sets host prerequisites itself. `silo host init`
checks them on a Linux host, prints a "Container computers" section (and a `containerComputers`
key in `--json`: `ready` and a list of `findings` with `id`, `level`, `ok`, `what` and `fix`),
and names the command that fixes each one. It never changes the host, and it never fails
because of them. The engine refuses to create a container computer with the same messages until
every required item is met.

On Ubuntu 24.04 the tools come from one line (it ships crun 1.14.1, skopeo 1.13.3 and umoci
0.4.7):

```sh
sudo apt install crun skopeo umoci uidmap
```

The checks, by id:

- `crun`, `skopeo`, `umoci` are on `PATH`; `uidmap`: `newuidmap` and `newgidmap` are installed
  and setuid root.
- `subuid`, `subgid`: `/etc/subuid` and `/etc/subgid` give this account at least 65536 ids.
  Ubuntu's `adduser` creates them; otherwise `sudo usermod --add-subuids 100000-165535
  --add-subgids 100000-165535 $USER`.
- `tun`: `/dev/net/tun` exists and this account can open it for reading and writing.
- `userns`, `apparmor-userns`: unprivileged user namespaces are allowed, and on Ubuntu
  `kernel.apparmor_restrict_unprivileged_userns` is `0`.
- `overlayfs`: the kernel supports overlay filesystems.
- `cgroup-v2`, `cgroup-delegation`: the unified cgroup hierarchy, with the account's systemd
  user manager delegating `memory` and `pids` (`loginctl enable-linger $USER`; if the controllers
  are still missing, `Delegate=memory pids cpu cpuset` in
  `/etc/systemd/system/user@.service.d/delegate.conf`).
- `systemd-run` is installed and `XDG_RUNTIME_DIR` is set.
- Advice only (nothing is refused, a limit stays off): `cpu-limit`, when `cpu` is not delegated,
  so CPU limits are not enforced; `landlock`, when the kernel's `lsm=` list lacks it.

**The disk.** A container computer shares the host's disk: its files are a folder in the engine's
home, and the size it was created with (`--disk` and the root disk, together) is reported, not a
bound. A rootless host has no way to cap a folder. Silo protects the host instead. When the file
system that holds the engine's home has less than 5 GiB free, the engine refuses to start a
container computer, to make a checkpoint of one or to fork one ("This device's disk is nearly
full ..."), and it stops, with the ordinary graceful stop, the running container computers whose
files are larger than their stated size, the largest first, with the reason in the computer's
status. A computer within its size is never stopped by this, and stopping frees nothing: free
the space (remove files, drop computers or checkpoints) and start the computer again. The
margin is `SILO_CONTAINER_DISK_MARGIN_MIB` (in MiB) in the environment of `silod`, for example
`systemctl --user edit silod` and `Environment=SILO_CONTAINER_DISK_MARGIN_MIB=10240`. The guard
reads the free space every 30 seconds and measures layers (inside the user namespace, so
unreadable folders count) only while the host is below the margin. The `/.msb` share of each
computer is outside every size.

**The AppArmor trade-off.** Ubuntu 24.04 blocks unprivileged user namespaces by default. The fix
Silo names, `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` (kept after a reboot
by a file in `/etc/sysctl.d`), relaxes that guard for every program on the host. The narrower
alternative is an AppArmor profile that lets only Silo's helper create user namespaces; Silo does
not install one.
