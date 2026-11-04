# Computer working account

Silo computers use `silo` (UID/GID 1001, home `/home/silo`) for terminal, SSH,
editor, repository, file-transfer and desktop work. Passwordless sudo provides
guest administration. Root remains the runtime initialization and management
identity. Silo sets the account up after a boot whenever a computer lacks it, which
also moves computers from older Silo versions to it; see
[older computers](SiloUI-WORKING-ACCOUNT-MIGRATION.md). Silo never falls back to root.

## Single-account verification, 2026-09-21

The account simplification passed 467 native tests (12 opt-in tests ignored)
using synthetic GitHub configuration and local socket access. The desktop
subset passed again after tightening guest UID/GID validation: 21 passed.
Guest lifecycle tests passed: 20. `git diff --check` and shell syntax passed.
These are code and fixture checks; they do not validate an installed app.
See [older computers](SiloUI-WORKING-ACCOUNT-MIGRATION.md#history) for the separate
live migration evidence.

## Account record and setup

The guest's root-owned `/var/lib/silo/working-account.json` record is the
account policy. The setup writes it last, and its version, user and home must
match the supported policy. It lives on the computer's disk, so checkpoints, forks
and exports carry it. Silo keeps no copy of it on the device.

After every boot (start, restart, a temporary boot for a guest command, or a
restore), Silo checks the record. When it is missing, Silo runs
`guest/working-account.sh` and `guest/working-account.py` as root. The setup
verifies the `/workspace` ext4 mount, refuses unrelated UID/GID collisions,
creates the account and sudo rule (or moves an older computer's homes into it), takes
ownership of `/workspace`, and verifies account access before writing the
record. Each step checks what an interrupted earlier run already did. If the
setup fails, Silo stops the computer, so a running computer always has the account. A
record from an unknown policy is never overwritten. The desktop reads the same
record, and desktop installation does not choose or set up the account.

Guest images from v3 on bundle `sudo`, `python3` and
`openssh-sftp-server`, so a new computer's account setup needs no network access or
package installation. Only older images that lack them install them with apt.
The setup never substitutes a root working session. The current
[desktop installer](../app/SiloUI/src-tauri/guest/setup-desktop.sh) uses Ubuntu
packages and the pinned Selkies package when installation is needed; a healthy
v4 built-in desktop needs neither download. See [desktop installation](SiloUI-DESKTOP.md)
for legacy guests and repairs.

### Published v3 account evidence

The [v3 image is public](https://github.com/0xpolarzero/silo/releases/tag/guest-ubuntu-24.04-v3).
The [current image lock](../app/SiloUI/guest-image/image-lock.json) now pins v5;
these v3 results qualify the earlier account-provisioning image.
[Publication run 35546417121](https://github.com/0xpolarzero/silo/actions/runs/35546417121)
built source `a9827c263df3daee28959b2c2073d85c6f980e9d`. Earlier offline
acceptance used local candidates with different hashes. The published ARM64
archive has now passed all 13 offline live test groups using the signed packaged
runtime, with its public archive hash verified. Existing computer startup is unaffected.

## Why runtime initialization remains root

A disposable test of `create --user silo` using the existing image failed
before the relay became available: agentd could not resolve the nonexistent
account during boot. Silo therefore provisions as root and explicitly supplies
`--user silo`, `USER` and `LOGNAME` for working commands. MicroSandbox resolves
the selected user's home. Administrative operations continue to request root.

Generated SSH configs and copied commands use the persisted account. Zed and
Git transport URLs defer to the generated SSH config. Git/jj identity writes
and verification target the same working account. Updated owners reject older
remote clients that do not advertise account protocol 1 when preparing a
computer connection. Update both devices
before remotely managing unified-account computers.

## SFTP runtime fix

Preimplementation live tests found that SSH command execution selected the
correct UID, but the built-in SFTP server issued agent filesystem RPCs as root.
An authenticated `silo` session created root-owned files and could write under
`/root`. Merely changing usernames would have retained this defect.

The pinned vendor patch routes non-root SFTP sessions through
`/usr/lib/openssh/sftp-server` using the existing user-scoped exec channel.
Linux then enforces file access and ownership for that account. A missing
helper fails the transfer; there is no fallback to root filesystem RPCs.
Root sessions retain the legacy implementation. Named SSH sessions also set
`USER` and `LOGNAME`; those variables were absent in the original runtime.

The runtime advertises `--silo-working-account-protocol` version 1. Build
validation, new-computer creation and unified-account SSH preparation require that
capability; an incompatible runtime cannot retain or bind a managed listener
for a computer with the unified account. This change stays in
the existing vendor patch and retains the other bundled runtime integrations.

## Desktop and preservation evidence

The disposable ARM64 desktop test installed Xfce/KasmVNC after creating files
under `/root`, `/workspace` and `/home/silo`. Their hashes, modes, UID and GID
were unchanged afterwards. Xvnc, Xfce and the window manager ran as `silo`;
no `silo-desktop` account was created. Desktop shutdown preserved an unrelated
job owned by the same UID. Reinstallation preserved VNC configuration and
password hashes. Existing conflicting configuration is rejected before package
installation on computers with the unified account.

A headed ARM64 Chromium 153 test rendered a page without `--no-sandbox`.
`chrome://sandbox` reported PID/network namespaces and seccomp-BPF/TSYNC
enabled. This proves the tested browser/runtime combination, not arbitrary
browser packages or every supported device architecture.

Reproduction: `scripts/test-working-account-live.py` is an opt-in disposable
computer test for production account provisioning, exec/SSH identity, SFTP/SCP
ownership, permission failures, missing-helper behavior and restart persistence.
It accepts the exact runtime, library, guest-image and evidence paths. It never
opens the user's normal runtime home. Native account/recovery/backup tests and
`scripts/test_desktop_service.py` cover required policy validation and desktop behavior.

Private logs and disposable probes belong under ignored
`src-tauri/target/verification/working-account/`. No user computer, credential or
project was used for the live acceptance tests. Cross-architecture and packaged
UI verification must be reported separately from these ARM64 guest results.

## Historical verification of the initial dual-account implementation, 2026-09-21

These results cover the initial implementation using v2 plus provisioning-time
package installation. They remain evidence for account, desktop and transfer
behavior. The subsequent v3 verification below establishes acceptance of the
revised bundled-tool provisioning path.

- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --quiet -- --test-threads=1`:
  465 passed, 12 opt-in tests ignored, using synthetic GitHub build configuration.
  The parallel run hit the existing storage-history configuration-lock race,
  also present in the baseline. That unrelated test was not changed.
- `npm --prefix app/SiloUI test -- --pool=threads --maxWorkers=2`:
  98 files, 886 tests passed. Default worker runs exited without results in this
  environment; the complete thread-worker run passed. The changed runtime-cache
  suite was subsequently rerun: 13 passed, including rejection of an outdated
  cached account protocol.
- `python3 -m unittest discover -s app/SiloUI/scripts -p test_desktop_service.py`:
  20 passed. Both guest setup scripts passed `sh -n`.
- `npm --prefix app/SiloUI run test:release`: 36 passed. Typecheck, lint,
  runtime input/patch preflight and `git diff --check` passed.
- Nine focused upstream SSH tests passed. The release-mode upstream test build
  required excluding an unrelated existing debug-only configuration test in the
  disposable test source; no such unrelated edit entered the production patch.
- The opt-in native backup/restore test passed against disposable VMs, including
  restore after removal of the original VM/cache, retained policy, Git identity
  in `/home/silo`, and UID/GID 1001 ownership.
- `npm --prefix app/SiloUI run desktop:build:debug -- --config
  '{"identifier":"org.silo.account-verification","productName":"Silo Account Verification"}'`:
  built the separate ad-hoc-signed macOS ARM64 verification bundle at
  `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Account Verification.app`.
  The account acceptance script passed using that exact bundle's `msb`,
  `libkrunfw.5.dylib` and guest image: provisioning collision/refusal, interrupted
  setup recovery, malformed policy refusal, exec and SSH/PTY identity,
  SFTP/SCP ownership, permission denial, missing-helper failure, legacy root
  SFTP and restart persistence. The test used only disposable guest data.
- The packaged runtime also passed a normal-user Git commit/push/clone and
  1 MiB Git LFS roundtrip, content comparison and `git lfs fsck`, plus SSH Git
  fetch/push with repository files remaining owned by `silo`. The production
  credential helper returned its literal placeholder to `silo` and no credential
  for an unrelated host. These used local disposable repositories, not live
  GitHub credentials or external repository mutations. Evidence is under
  `target/verification/working-account/packaged-git/live.log`.

The packaged GUI was not launched. Live Linux/KVM, x86-64 and two-computer UI
workflows were not exercised. Those limits are not covered by compilation or
fixture tests. No migration of an existing user VM was performed.

### Offline v3 candidate verification

Both architecture images passed all seven Docker tests with networking disabled,
including normal-user SFTP and rejection of missing sudo, Python or SFTP support
and a preinstalled working account. ARM64 and AMD64 compressed archives are
85,801,668 and 87,799,791 bytes respectively. These were local candidates;
the v3 lock at publication instead recorded the subsequently published artifacts.
ARM64 local staging passed with its download function forced to throw.

The ARM64 MicroSandbox test used the candidate image with `--net none`. All eight
missing-tool cases failed preflight without package-manager calls. Provisioning,
exec/SSH identity, SFTP/SCP permissions, Git and Git LFS roundtrips, and restart
persistence passed. Traps confirmed no package manager ran across the workflow.
Evidence: `src-tauri/target/verification/working-account/offline-v3/live.log`;
Docker test logs: `/tmp/silo-guest-image-v3-{arm64,amd64}-tests.log`.

The rebuilt candidate bundle passed the additional verification below.
Publication subsequently produced different archive hashes. Desktop and browser
evidence above used the earlier image;
the offline account test does not claim offline desktop installation.

### Packaged offline v3 acceptance

The isolated debug bundle `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Account Verification.app` built successfully. All 13 live test groups passed using the runtime, library and v3 guest image inside that exact bundle, with guest networking disabled and no package-manager calls. Its guest archive SHA-256 and manifest matched the candidate ARM64 lock at the time, not the subsequently published archive. Evidence: `target/verification/working-account/offline-v3-packaged/{live.log,image-verification.json}`. The disposable VM was stopped and removed. No GUI launch or Linux/KVM verification is claimed. The published v3 archives are now available; acceptance of the published ARM64 bytes is recorded below.

### Published v3 acceptance

The actual public ARM64 archive passed all 13 offline live test groups using the
signed packaged runtime. Its verified SHA-256 is
`03f592e602afb0fff724a1f82a5866571356d398b4a3edae1bc15d3a7360efc0`.
Evidence: `target/verification/working-account/offline-v3-published/live.log`.
This validates the published ARM64 guest bytes separately from the earlier
candidate tests; it does not add GUI, AMD64 VM or Linux/KVM acceptance.
