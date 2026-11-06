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
