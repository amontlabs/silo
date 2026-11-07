# Remote hosts build log

Plan: [PLAN.md](PLAN.md).

## Phase 1: Connections works (6 Nov 2026)

- **Found:** every Connections call failed on main. The forced SSH command runs
  `~/.local/bin/silo-remote[-dev] --remote-bridge`, the link points at silod, and silod rejected
  the flag (`unsupported silod argument: --remote-bridge`). `remote::run_bridge` had no caller since
  the engine moved from the app into silod, before this repository's history starts.
- **Fixed:**
  - silod handles `--remote-bridge` (`silod/src/lib.rs`, `app::run_remote_bridge`).
  - Started through the link, macOS found no main bundle (`silod has no bundle identifier`).
    silod now replaces itself with the real file in its bundle for `--remote-bridge` and
    `--remote-guest`.
  - About 1 bridge call in 6 failed with `Invalid argument (os error 22)`: macOS refuses to set a
    timeout on a socket whose peer already closed, while the owner's reply was still buffered.
    The bridge now ignores that refusal and reads the reply.
  - A controller that reaches an old bridge names it ("runs a Silo version that cannot accept
    connections") instead of the generic SSH help.
- **Live (Mac, Dev build):** before the fix, `silod --remote-bridge` exited 1 with the error above.
  After: the handshake and `computers.list` answer through the link and through a user-run sshd
  with Silo's exact restricted `authorized_keys` line; a command sent over that key still runs
  only the bridge.

## Phase 2: silod on Linux (6 Nov 2026)

- **Found:** every crate but `silod` built on Linux. `silod` stopped at its
  `compile_error!("silod is a macOS-host engine process")`, and behind it: `objc2` outside the
  macOS dependency table, the AppKit, Service Management and code-signature modules ungated,
  `getpeereid` for the peer check, `sysctlbyname("kern.osversion")`, a `ProbePaths` literal the
  engine's Linux-only field broke, and the restart exit code kept in the AppKit module. The
  engine built on Linux with 98 compiler warnings (macOS-only code and imports), and the
  daemon pulled in GTK through `silo-host`.
- **Fixed:**
  - The startup steps moved, in the same order, from `app.rs` to `agent.rs`; `app.rs` keeps
    the AppKit loop and `linux.rs` runs a plain main-thread queue. SIGTERM and SIGINT end the
    session; exit code 75 still asks for a restart.
  - On Linux the manifest is read from `resources/` beside `silod` with every check except the
    bundle identifier, and there is no signature check (signed archives are roadmap
    milestone 11). Data, cache and logs follow XDG; `~/.silo-dev` and `engine.sock` stay where
    clients look. The peer check reads `SO_PEERCRED`.
  - A file credential store in the engine (one 0600 file per item under a 0700 directory,
    written by rename) is the `keyring` default in `silod` on Linux;
    `SILO_CREDENTIAL_STORE=secret-service` keeps the Secret Service.
  - `silo-host` has a `desktop` feature (default, the app's) for its GIO and GTK code; the
    engine depends on it without that feature.
  - macOS-computer checks in the `silod` tests run only on Apple silicon, as in the engine.
  - CI's Linux job also runs the `silod`, CLI, client, API and `silo-host` tests and checks that
    `silod` has no GTK dependency.
- **Tests (Linux x86_64 container, as root, `cargo test`):** `silod` 316 unit tests and its
  10 socket tests passed; `silo-cli` 305, `silo-client` 15 and `silo-api` 49 passed;
  `silo-host` 84 with `desktop` and 67 without passed. `silo-engine`: 3032 passed, 19 failed,
  45 ignored; the same 19 fail at the phase 1 commit in this container (3024 passed there):
  8 `host_push::protocol_tests` that need the staged bundled Git, 9 failure-injection tests
  whose injected permission errors do not happen as root, and 2 `changes::collect` Postgres
  fixture tests.
- **Live (Linux container without KVM, Dev manifest, fixture HOME):** `silod --agent` served
  `engine.sock`, `silo list` answered with no computers, a second agent deferred to the owner,
  `--remote-bridge` started through a `~/.local/bin` link and, given no request, exited 1 with
  its connection-ended message, and SIGTERM and SIGINT each ended the engine
  with exit 0 and removed the socket. No computer was created: there is no `/dev/kvm` here.
  A build of `silod` in a fresh target with `pkg-config` refusing every GTK package succeeded.
- **Phase 2 review fixes:**
  - SIGINT: the engine starts `msb` (lifecycle commands, backups, SSH access and remote
    connections, guest commands) in its own process group on Linux, so a terminal's Ctrl-C
    reaches `silod` alone; after the first SIGINT or SIGTERM the handlers keep listening and a
    second one exits at once with status 1. The handlers are registered before the socket is
    bound, so a signal sent as soon as `engine.sock` appears no longer kills the engine with
    the socket left behind.
  - Account: `silod --agent`, `--remote-bridge` and `--remote-guest` refuse euid 0 and 65534
    ("use a dedicated account in the kvm group").
  - Engine directory (`silod/src/install_trust.rs`): every ancestor from `/`, the directory,
    `silod`, its siblings and everything under `resources/` must be owned by the account or root
    with no group or other write bit (sticky `/tmp` refused); links must stay inside. The
    refusal names the path. The agent then checks the MicroSandbox manifest against the
    compiled-in runtime inputs and the SHA-256 of `msb` and `libkrunfw`
    (`dependencies::verify_bundled_runtime`) before the first `msb` launch; an engine without
    the runtime still starts.
  - File credential store: directory descriptors for every step after the root (`openat`,
    `renameat`, `unlinkat`, `fsync` through `rustix`), ancestors checked from `$XDG_DATA_HOME`
    down, staged leftovers swept at engine start and on delete, refusals reported as
    `credential_access_denied` with the path (`credential_store::file_store_refusal_message`)
    and as refused in `engine.credentials.status`. The bridge and guest helper install the same
    store. The first start records the store in `<data>/credential-store`; a different explicit
    `SILO_CREDENTIAL_STORE` later stops startup.
  - A unit test that the credential directory is never a runtime mount source or inside or
    above one; the peer-credential test no longer hangs when its child cannot `setuid`.
  - Docs: [Linux hosts](../THREAT-MODEL.md#linux-hosts) in the threat model; the Secrets and
    engine guides; the install example no longer uses `/tmp`.
  - Not done: the hash check runs once at start, not before each launch (anyone who can swap
    the files afterwards could also swap `silod`), and `msb`'s expected hash comes from the
    manifest the ownership check covers, since it is built from patched source; signed
    archives stay roadmap milestone 11. Other engine children (SSH, Git, editors) keep the
    engine's process group.
  - Tests (same container, as root, `cargo test`): `silod` 327 unit tests and its 10 socket
    tests passed; `silo-cli` 305 passed; `silo-host` 84 with `desktop` and 67 without passed.
    `silo-engine`: 3037 passed, 19 failed, 45 ignored; the failures are the same 19 as above.
  - Live (same container, Dev manifest, a test account made with `useradd`): `silod` as root
    and the bridge as root were refused; as the test account from a root-owned
    `/opt/silo-engine` it served, recorded `file` in `credential-store`, and `silo list`
    answered. An engine under `/tmp`, a group-writable `engine-manifest.json` and
    `SILO_CREDENTIAL_STORE=secret-service` after a `file` start were each refused with the path.
    Two SIGINTs 20 ms apart exited 1 after the second; a SIGTERM sent as soon as
    `engine.sock` appeared ended the session with exit 0 in three of three runs. The bridge
    through a `~/.local/bin` link reported that Silo was not running. No runtime was
    installed, so the hash check reported its absence; its failures are covered by unit tests.

## Phase 3: Linux engine packaging (6 Nov 2026)

- **Found:** `silod --service` refused on Linux. The engine release built only the macOS archive,
  which holds `silod` and the files an app needs to make a bundle, not a runnable engine. No
  workflow started a Linux computer: the KVM jobs went with `be300f4f`.
- **Built:**
  - `silod --service register|unregister|status` on Linux (`silod/src/service_linux.rs`): a
    systemd user unit `~/.config/systemd/user/<channel>.engine.service` that runs the canonical
    `silod --agent` with `Restart=on-failure` (exit code 75 restarts, a clean exit does not),
    `RestartSec=10`, `UMask=0077`, `KillMode=process`, `KillSignal=SIGTERM`,
    `TimeoutStopSec=30`, `WantedBy=default.target`; then `daemon-reload` and `enable --now`.
    `status` prints the macOS status word, then state, unit, program and linger. Lingering is
    reported with the command an administrator runs; silod never calls `sudo`. A file at the
    unit's path that silod did not write is never replaced or removed. `KillMode=process`
    because the runtime's VM processes are detached on purpose and an update keeps them running;
    silod's own session end stops them on SIGTERM.
  - `package-engine.py --platform linux-x64|linux-arm64 [--channel development]`: the
    `silo-engine/` directory with `silod`, `silo`, `msb` and the Git tools beside it, and
    `resources/` with the manifest, the MicroSandbox manifest, licenses and `libkrunfw`, and
    `git-support/` (Git manifest, templates, licenses, the guest's `git-lfs-transfer`). The
    staged runtime is checked against its manifests and every executable's ELF machine against
    the platform. The macOS archive is byte-identical to before (same SHA-256 for the same
    stub binary).
  - `engine-release.yml`: a `build-linux` job on `ubuntu-24.04` and `ubuntu-24.04-arm`, in
    parallel with the macOS build; the release carries all three archives. Not run.
  - `linux-host.yml` (pull requests touching native app sources, and by hand): KVM through
    GitHub's udev rule and a KVM API probe, the Dev archive, a user service under a lingering
    runner account, then `scripts/linux-host-e2e.sh` (create `e2e-hosts-ci`, start, exec
    `uname -a`, checkpoint, fork, exec in the fork, drop both, with cleanup on failure), a
    `systemctl --user stop` that must end with exit code 0, and `--service unregister`. Logs
    are uploaded on failure. Not run yet.
- **Tests (Linux x86_64 container, as root):** `silod` 328 unit tests (12 new for the service)
  and its 12 other tests passed. `run-script-tests.py`: 45 of 45 modules passed, among them
  the package, engine release workflow, new Linux host workflow, workflow pin and native-change
  tests. actionlint 1.7.7 found nothing in the two workflows without shellcheck, and only
  info-level SC2012/SC2153 notes with it (one is in the existing macOS step).
  `systemd-analyze verify` accepted the generated unit, including a path with a space.
- **Live (same container, no systemd as PID 1, no `/dev/kvm`):** `silod --service register`,
  `status` and `unregister`, started through a link, wrote, reported and removed the unit with
  fake `systemctl` and `loginctl` on `PATH`. `runtime:prepare` built the patched `msb` and
  staged MicroSandbox, then stopped at the bundled Git download (the proxy here refuses
  `www.mozilla.org`). With stub Git and LFS files in the ignored staging directories,
  `package-engine.py --platform linux-x64 --channel development` built release `silod` and
  `silo` and wrote the archive; its checksum verified, the unpacked `silo` printed
  `org.silo.dev`, `msb --version` printed `msb 0.7.6` with the bundled `libkrunfw`, the
  unpacked `silod --agent` served a fixture HOME, `silo list` answered with no computers, and
  SIGTERM ended it with exit code 0. No computer was created: there is no KVM here.
- **Only CI can prove:** the Linux archives with the real Git runtime, the arm64 build, the
  systemd user service under a real user manager, and a Linux computer created, started,
  forked and dropped over KVM.
- **Review fixes:**
  - A Linux session end that should have stopped computers asks the runtime what still runs.
    When anything does, or it cannot tell, silod logs the names and exits with 70
    (`SESSION_END_INCOMPLETE_EXIT_CODE`), not 0 and not the restart code 75: systemd records
    the stop as failed and never restarts a unit whose stop it was asked for. A prepared
    update's released set still exits 0. `KillMode=process` stays.
  - `unregister` reads the unit's cgroup before `disable --now` and afterwards names every
    process left in it, with how to take them back (start the engine again).
  - The lingering advice and the docs say the last logout ends the user manager, which kills
    computers still running under it (`user@.service` uses `KillMode=mixed`; check the
    distribution's).
  - `loginctl show-user` gets the UID; the advice names the account from `getpwuid_r`, with the
    UID as fallback, never `$USER`.
  - The unit sets `StartLimitIntervalSec=0` in `[Unit]`; the trade-off is in the engine guide.
  - Reading the unit opens it with `O_NOFOLLOW | O_NONBLOCK` and checks the open handle; a
    FIFO is refused without blocking. A temporary file a crashed run left under the same name
    is replaced only when it carries silod's marker.
  - Linux archives require `resources/git-support/ssl/cacert.pem`, which both Linux Git builds
    need; the layouts in the engine and release guides list `git-support/ssl/`.
  - `engine-release.yml`: a `tag` job before both builds, the Linux archive names checked
    against its tag, and the release only after every build passed. `libssl-dev` dropped from
    it and from `linux-host.yml`: neither `Cargo.lock` nor MicroSandbox's lists `openssl-sys`,
    and the cached `msb` links no libssl.
  - `warm-release-caches.yml` saves the runtime and Cargo download caches for
    `ubuntu-24.04` / `x86_64-unknown-linux-gnu` and `ubuntu-24.04-arm` /
    `aarch64-unknown-linux-gnu`; the app dependency cache stays macOS only.
  - `linux-host.yml`: no `rust-cache` (pull requests never saved it), `$(id -un)` for
    lingering, `DBUS_SESSION_BUS_ADDRESS` exported with `XDG_RUNTIME_DIR`, and the stop step
    runs after a failure too (stopping only, asserting only after a passing run).
  - `linux-host-e2e.sh` fails when it cannot list computers before it starts, and stops both
    computers before dropping them (also in its cleanup).
  - Docs: an engine already running outside systemd makes the unit's engine defer and exit 0;
    the symlink-flip upgrade needs `--service register` again; the release needs both Linux
    builds; the journal is readable by host administrators (for phase 6's threat model); the
    GitHub App client secret in the Linux binaries is accepted, as on macOS; the Linux
    verification guide's header no longer reads as if all Linux support were discontinued.
  - Second review: the survivor check is one `msb list` (`update_recovery::listed_states`)
    counting starting, running, draining and paused computers; it no longer inspects each
    computer or refuses for an unfinished configuration, a removed computer or a transitional
    state, which made a clean stop exit 70. A client's `engine.shutdown` that a SIGTERM
    attaches to takes the same exit code (`UpdateState::session_ending`); any other shutdown,
    and every shutdown on macOS, still exits 0. `unregister` runs `reset-failed` before the
    reload, and finds the cgroup of an already inactive unit from its slice's.
    `linux-host.yml` runs `chmod -R go-w` on the unpacked engine for phase 2's
    engine-directory trust check.
  - Tests (same container, as root): `silod` 334 unit tests and its 12 other tests
    passed; `run-script-tests.py` 45 of 45 modules; actionlint 1.7.7 (no shellcheck here)
    found nothing in the three changed workflows. Not run: any workflow, and a session end
    against real computers.

## Phase 4: `silo host init` and adding a host by address (6 Nov 2026)

- **Found:** the CLI had no command for Connections; the app's Connect device was the only way
  to add a device. Pairing was controller-driven only: the controller installs its restricted
  key over SSH (`connections.ssh.keySetupCommand`), so a host that accepts neither a password nor
  one of the controller's keys could not be added. The engine had no method to install, list or
  return a key on the owner's side. On macOS the engine registers as a login item only from the
  app (`SMAppService` from `Silo.app`) or `silod --service register` from `Silo Engine.app`.
  `connections.status` dropped the engine's `cliLinkError` through its typed reply, so the
  message the app shows for an unlinked `silo` never arrived.
- **Built:**
  - Engine methods `connections.keys.authorize`, `connections.keys.list` and
    `connections.keys.own` (`remote.rs`, `silod` `methods/access.rs`, `silo-api`, schema and
    request constructors regenerated). They are served on `engine.sock` and not through the
    bridge; a unit test checks that. `authorize` writes the key setup's exact line and leaves a
    symlinked `authorized_keys` alone. `cliLinkError` is in the typed status.
  - `silo host init [--authorize KEY|FILE]...`, `silo host status`, `silo host revoke`
    (`cli/src/hosts.rs`): account and VM checks (`/dev/kvm` and its group on Linux, Apple silicon
    on a Mac), `silod --service register` from beside `silo` (Linux) or from the app's
    `Silo Engine.app` when no engine answers (Mac), the `~/.local/bin/silo[-dev]` link (Linux,
    only when absent or Silo's), Connections on, the keys, the addresses, and whether anything
    answers on port 22. Keys are read and checks run before anything changes.
  - `silo devices add <address> [--host-key ...] [--replace]`, `list`, `remove`, `key`:
    `connections.devices.connect`, with the host-key and key-setup commands run in the terminal
    when SSH needs them (each at most once, never for a changed host key, printed instead
    without a terminal). `user@host:port` becomes `ssh://user@host:port`. `--host-key` pins the
    host's key in `~/.ssh/known_hosts` by fingerprint (matched against `ssh-keyscan`), key or
    key file.
  - `scripts/hosts-e2e.sh` and a second part of `linux-host.yml`: accounts `silo-host` and
    `silo-controller` on the KVM runner, a root-owned copy of the engine, the runner's sshd;
    host init, add by `127.0.0.1` with a pinned fingerprint, create, start, stop and drop an
    `e2e-hosts-remote` computer with `--device`, remove the host, unregister both services.
    `SILO_E2E_REMOTE_EXEC=1` adds `exec` and `fork -- uname -a` through `--device`, which need
    phase 5; the workflow does not set it yet. Not run.
  - The app: Connect device already adds a host by address, and its Set up Silo SSH key action
    is the same key setup; no UI change.
- **Tests (Linux x86_64 container, as root):** `silo-cli` 328 passed (15 new in
  `tests/hosts.rs`, 8 new unit tests), `silo-api` 49, `silo-client` 15; `silod` 346 unit tests
  (1 new) and its other test binaries passed; `silo-engine` `remote::` 162 passed, 1 failed:
  `handshake_reports_failed_key_upgrade_and_preserves_managed_files`, which also fails at the
  phase 3 head in this container (its read-only directory does not stop root). 4 of the new
  engine tests are in that module. `run-script-tests.py`: 45 of 45 modules (4 new hosts-e2e
  tests and 1 workflow test). No actionlint or shellcheck here.
- **Live (same container, no systemd, no KVM):** two accounts made with `useradd`, a Dev engine
  in a root-owned directory, fake `systemctl` (starting `silod --agent`) and `loginctl` on
  `PATH`, a user-run sshd on 127.0.0.1:2222, and a character device made with `mknod` at
  `/dev/kvm` so the access check could pass (it cannot run a VM). Root was refused; the host
  account without the device's group was refused with the `usermod -aG` advice. After it joined
  the group, `silo host init --authorize controller.pub` registered the unit, linked
  `~/.local/bin/silo-dev`, turned Connections on (and the bridge link), wrote the restricted
  line, listed the controller's fingerprint and printed the addresses; a second run reported
  `alreadyLinked` and `added: false`. On the controller, `silo devices add
  silohost@127.0.0.1:2222` without a pin failed with the host-key command and no prompt, a wrong
  fingerprint was refused with the one offered, and the right one pinned `[127.0.0.1]:2222` and
  connected through the forced command. `silo --device <id> list` answered from the host;
  `silo devices remove` revoked the line and `host status` listed no controllers. `hosts-e2e.sh`
  against the same setup passed every step before computer creation, which stopped at the
  guest image download (no network for it here), then removed the host. The first live run found
  that `silo devices remove <name>` filled the global `--device`; the argument was renamed and a
  test added.
- **Needs a live check:** the KVM runner workflow; a Mac host (`silo host init` with the app's
  engine running, and with none, where `Silo Engine.app` registers its own login item next to
  the app's; Remote Login; macOS computers on a Mac host from a Linux controller); a real
  systemd user manager with lingering; the app's Connect device against a Linux host.

- **Review fixes (PR #79):**
  - Mac: `silo host init` no longer runs `silod --service register` from `Silo Engine.app`, which
    registered a second login item with the app's launchd label. With no engine answering it
    opens the app that holds this `silo` once (`open -g -j -a`), waits for the socket for
    `engine_wait` and names System Settings > General > Login Items when nothing answers. With
    an engine answering, `launchctl print gui/<uid>/<identifier>.engine` (read only) says
    whether launchd has the login item; `silod --service status` is not used there because,
    run from the engine bundle, it reports that bundle's registration, not the app's. Linux
    `host status` advises when the service is not `enabled`. `silo host` and `silo devices`
    refuse `--allow-other-channel` and a `--socket` outside this channel's home.
  - Bridged revoke: Silo's line passes the bridge `--key` and the key's `SHA256:` fingerprint;
    `silod --remote-bridge` accepts exactly that argument (else `usage: silod --remote-bridge
    [--key SHA256:<fingerprint>]`), the bridge replaces any `authenticatedKey` in the request
    with it, and the owner allows a bridged `connections.keys.revoke` only for that key. A line
    without `--key` is refused for a bridged revoke while the key has a bridge line. The
    controller's key setup still writes the line without `--key`, so an earlier host keeps
    accepting it; a handshake and `keys.authorize` rewrite it with `--key`. A controller removing
    a device sends a handshake and retries once when its own key is refused. A controller that
    reads the usage line reports an earlier Silo on the other device. The protocol version is
    unchanged.
  - `connections.keys.list` lists every line whose forced command is this channel's bridge (any
    options or comment, `~/`, `$HOME` or the home's path), with `canonical` and `identified`;
    `revoke` removes all of them, and `silo host revoke` also takes a fingerprint and reports
    only keys it removed.
  - Keys: `--authorize` takes an argument as a key only when it is an Ed25519 key (base64 blob
    with the `ssh-ed25519` type string and a 32-byte key), else as a file whose every line must
    be one; the engine's `authorize` checks the same.
  - `--host-key` looks the name up with `ssh-keygen -F` (hashed entries; `@revoked` refused,
    `@cert-authority` ignored) and checks the address with the engine's rules before
    `ssh-keyscan`; `ssh_target` reads `user@host:port`.
  - A dangling `~/.local/bin/silo` link is replaced only when its target was in a directory
    named `silo-engine*`. `init` and `status` warn when the home or `~/.ssh` is group- or
    world-writable (sshd `StrictModes`). `devices remove` no longer claims a key was revoked.
    A cross-crate test checks silo-client's link names against silo-host's.
  - CI: the shared engine copy is under `/var/lib/silo-ci` (root, 0755): the KVM runner's `/opt`
    is mode 777, which the engine's trust check refuses. `hosts-e2e.sh` logs `df -h` before
    creating the computer.
  - Private groups: the KVM job for PR #79 stopped with "`~/.local/share` can be changed by
    other accounts" because `silo-controller`'s `~/.local/share` was 0775 in its own group
    (umask 002, as on every Debian or Ubuntu account with a user-private group). The credential
    store's ancestor check, `install_trust` and the CLI's `StrictModes` advice now accept group
    write for the owner's user-private group, Debian OpenSSH's rule (the owner's primary group,
    no member but the owner, and no other `/etc/passwd` account with it as primary group;
    `/etc/passwd` only, as `getpwent` is not thread-safe), in
    `silo_engine::safe_directories` (the CLI keeps a copy, as it does not depend on the engine,
    and applies it only where `/etc/debian_version` says sshd is Debian's). World-writable and
    sticky directories stay refused; the refusal names the mode and group and gives
    `chmod g-w` or `chmod go-w`. Missing directories get fixed modes whatever the umask (0755 to
    `$XDG_DATA_HOME`, 0700 below; 0755 for `~/.config/systemd/user`). Live: a `useradd` account
    with a 0775 `~/.local/share` in its private group served from a root-owned engine copy and
    `silo list` answered; the same directory in a shared group was refused with the new
    message; a fresh home under umask 002 got 755/755/700/700. The account was removed after.
  - Security review of the private-group change: directories are created through descriptors
    (`openat` with `O_NOFOLLOW` of the deepest existing one, then `mkdirat`, `openat`, `fstat`
    and `fchmod` per level), and only inside an existing directory of the account or root that
    nobody else can change, so a link swapped in cannot make `chmod` widen another file. Root
    (uid 0) and gid 0 get no exemption; only a local account and group count (the owner's
    `/etc/passwd` line and the group's single `/etc/group` line must agree with the lookups);
    `/etc/passwd` or `/etc/group` over 16 MiB counts as unreadable and refuses. The threat model
    names what the rule cannot see (network memberships, `pam_group`, gshadow passwords, setgid
    programs).
  - Tests (Linux x86_64 container, as root): `silo-engine` `remote::` 169 passed, 1 failed (the
    pre-existing root-only `handshake_reports_failed_key_upgrade_and_preserves_managed_files`);
    `silod` 346 unit tests and its other binaries passed; `silo-cli`, `silo-api`, `silo-client`
    passed; `run-script-tests.py` 45 of 45.
  - Live (same container, as before: two `useradd` accounts, root-run sshd on 127.0.0.1:2222,
    fake `systemctl`/`loginctl`, `/dev/kvm` from `mknod`): a non-Ed25519 `--authorize` changed
    nothing; init wrote two `--key` lines; `devices add` with the pinned fingerprint connected,
    a wrong one was refused; `silo --device <id> list` answered from the host; a bridged revoke
    of the other controller's key over the controller's Silo key was refused with
    `permission_denied`, also with a forged `authenticatedKey` in the request; with the
    controller's line rewritten without `--key`, a bridged revoke was refused, `host status`
    marked the line, and `devices remove` upgraded it by handshake and removed it; `host revoke
    SHA256:...` removed the other key. Accounts, files and the device node were removed after.
- **KVM run:** the two-account job in `linux-host.yml` went green after four fixes, each found by
  a failed run:
  - The runner's `/opt` is world-writable, so the engine's trust check refused the shared copy
    under it; the copy moved to a root-owned `/var/lib/silo-ci` (0755).
  - The runner's `XDG_CONFIG_HOME` leaked through `sudo -u <account>` into the accounts'
    `silod --service` commands, which then looked for the unit in the runner's home; the accounts
    run under `env -i` with only `HOME`, `USER`, `LOGNAME`, `PATH`, `XDG_RUNTIME_DIR` and
    `DBUS_SESSION_BUS_ADDRESS` (`hosts-e2e.sh`, `as`).
  - Umask 002 made `~/.local/share` group-writable in each account's private group, which the
    credential store refused; handled by the user-private-group rule above.
  - A default ACL on the controller's directory overrode `ssh-keygen`'s umask, so the Connections
    key came out 0644 and `ssh-keygen -y` refused it; the engine sets a new key to 0600 after
    generation (`editor.rs`, `key`).
  With those, the job prepares `silo-host`, adds it from `silo-controller` by `127.0.0.1` with a
  pinned fingerprint, and creates, starts, runs `exec` and `fork -- uname -a` in, stops and drops
  a computer on it through `--device` (`SILO_E2E_REMOTE_EXEC=1`), then removes the host and
  unregisters both services.

## Phase 5: exec, fork with a command and computer use on another device (6 Nov 2026)

- **Found:** silod already routes `guest.exec`, `guest.exec.start`/`get`/`input`,
  `desktop.status`, `desktop.action` and `computerUse.*` to the owning device. The CLI did not
  send them there: they were missing from `ROUTED_PREFIXES` (`cli/src/engine.rs`), so
  `Engine::call` refused them under `--device`; `refuse_on_device` refused `exec` and
  `fork -- <cmd>`; the MCP `fork` refused `device` with `command`, and `exec`, `fork_gui` and
  the `computer_use_*` tools had no `device`.
- **Changed:**
  - The CLI adds `deviceId` to `guest.exec`, `guest.exec.start`/`get`/`input`,
    `desktop.status`, `desktop.action` and `computerUse.*` (named exactly; the other `guest.*`
    methods and `desktop.connect` stay local).
  - `silo --device X exec` and `fork -- <cmd>` (with `-n`) run there; their steps are recorded
    there. A fork on another device publishes no ports and reads no URLs here (`network.*` is
    not routed), instead of reporting a ports error. Write-back to recipe files is off for
    forks with a command on another device, as for `fold`.
  - MCP: `device` on `exec`, `drop`, `fork_gui`, `computer_use_ready` (a read) and the other
    `computer_use_*` tools; changes need the device in the agent's `devices:`. Driver sessions
    are keyed by (device, computer id) and closed on their own device on `drop`, `promote`
    (sessions of that device) and shutdown.
  - The driver polls every 250 ms instead of 50 ms on another device.
  - `auto_keep` `tests: pass` runs the recipe's `test:` on the fork's device.
- **Tests:** fake-engine cases for CLI `exec` and `fork -- <cmd>` under `--device`
  (`cli/tests/devices.rs`), `fork -n` with a command on a device (`fanout.rs`), `fork_gui`,
  a remote screenshot, separate local and remote sessions and the close on `drop` (`gui.rs`),
  MCP `exec` and `fork` with `command` allowed and refused (`mcp_devices.rs`, `gui.rs`), and
  auto-keep tests on a device (`policy.rs`). `cargo test -p silo-cli`: 315 passed.
- **Not checked live** in this phase: controller to host exec, fork with a command and a
  screenshot. The KVM job now runs exec and fork with a command through `--device`; the
  screenshot is still open.

## Phase 6: threat model and docs (7 Nov 2026)

- **Threat model** ([Linux hosts](../THREAT-MODEL.md#linux-hosts)): three new paragraphs, each
  claim checked against the code and anchored to it. Connections on the host: the restricted
  `authorized_keys` line `silo host init --authorize` writes, Ed25519 only; the bridge accepts
  exactly `--key SHA256:<fingerprint>` and replaces any `authenticatedKey` in the request with it;
  a bridged revoke is limited to the caller's own key; `connections.keys.authorize`, `list` and
  `own` are served on `engine.sock` only; lines Silo did not write are left alone and reported;
  what a controller's key gives (every method the bridge serves, the authority of boundary 7 on
  the host, until revoked) and that the host's sshd must accept the account's keys. Logs: silod
  has no log framework; the journal holds the lines it prints (owner refusals, signals, the
  runtime check, Connections start and accept errors, the link error, image cache repair, update
  recovery, computers a session end left running), none with a credential or token value, and
  root and the `systemd-journal` or `adm` group read them. Lingering: the unit starts at boot and
  outlives logouts, so a lingering host answers every authorized key at all times. The egress
  paragraph no longer says the eval "comes with the KVM CI job": the job is green but runs the
  lifecycle and the two-account run, not an isolation scenario; the evals have run on a Mac only
  (roadmap milestone 9 runs them on KVM). The ACL residual-risk note gains the `ssh-keygen` finding
  and the 0600 fix, and "not defended" gains the controller's key.
- **Docs:** [PLAN.md](PLAN.md) has a status per phase; the roadmap's milestone 6 progress names
  phase 4, the green KVM job with its two-account run, and the one open item (an engine release
  with the Linux archives); the CLI guide shows the Linux host end to end (unpack the archive,
  `silo devices key` on the controller, `silo host init --authorize controller.pub` on the host,
  then `silo devices add` on the controller) and the Connections guide points at that order.
- **Checked:** every `file#Lline` anchor in the Linux hosts section against the current tree, and
  the CI link check (`lychee --offline` with `ci.yml`'s globs) passes.
- **Remaining:** the Mac live checks: `silo host init` on a Mac, adding it from another engine,
  a screenshot over `--device`, and a Mac controller managing a Linux host.
