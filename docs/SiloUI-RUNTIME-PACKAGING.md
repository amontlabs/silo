# SiloUI runtime packaging

Status: MicroSandbox 0.7.6 runtime inputs and sixteen patches are pinned. The full app qualification below used 0.7.2 and has not been repeated in full on 0.7.4 or 0.7.6. Later runtime-specific builds, regressions and disposable-VM checks are recorded under [MicroSandbox 0.7.4 upgrade](#microsandbox-074-upgrade) and [MicroSandbox 0.7.6 upgrade](#microsandbox-076-upgrade); those checks do not requalify every app workflow. The optimized macOS qualification bundle passed disposable migration, checkpoint, fork, restore, restart, and authorized live GitHub policy checks. On Linux ARM64, the final AppImage passed native WebKit smoke, packaged-tool integrity, and dependency checks; the authentic predecessor passed migration, checkpoint/fork/restore, RAM/process replay, same-home lineage exports, cold-cache import/Start with the original source cache absent, relaunch persistence, and saved/native/physical capacity checks. These tests ran in an Ubuntu 24.04 ARM64 Lima guest with nested KVM on Apple Silicon, not bare-metal Linux ARM64. On Linux x86-64, the runtime-7 package passed authentic migration and the same-home export matrix; a separate production fresh-home import/Start passed with source cache paths absent. The x86 desktop package also passed unsaved Mousepad checkpoint/fork/source-restore and stale-X11 recovery. The positive live remote-viewer check passed on the final x86 AppImage SHA-256 `8079943262a6a70e007403aa3900d1fd857080d63a04ea8dc4fb700dc5c7d8b2`, controller executable SHA-256 `96ffb0bad56f0e655d2072a7d9ad7bf987c83961292b5c62d84f5d437d671465`. Pinned-key SSH authenticated and exited 0; the running remote viewer connected, while opening a stopped fork left it stopped. Evidence is in `app/SiloUI/src-tauri/target/verification/x86-final-desktop-20260927/remote-final/`. The later user-authorized x86 runtime-8 AppImage passed payload, manifest, tool-version, protocol-probe, and dependency verification, as well as live port-control proof. Its package SHA-256 is `58a0516b396632390b9637966219ff732b577810fcf18483dcc9cbb1409d804a`; detailed hashes and evidence are in [Linux verification](SiloUI-LINUX-VERIFICATION.md). The native GTK destination chooser and source-only archive export passed; see [Linux verification](SiloUI-LINUX-VERIFICATION.md). Release signing and distribution are publication steps outside this implementation qualification.

## Qualification evidence

On 2026-09-25, `npm --prefix app/SiloUI run runtime:prepare` compiled the pinned release CLI and passed every executable capability and Silo protocol probe, then staged the aarch64 macOS runtime, Git, and guest. The exact GitHub owner resolver source in the network patch matches the tree that passed the upstream 574-test network suite; focused resolver tests passed 4/4 and the CLI probe parser test passed 1/1. The packaged `msb` returned `msb 0.7.2`, and each of its five Silo protocol probes returned `1`. The full native suite passed 492 tests, with 12 ignored and none failing; release-tool tests under Node 24.11.1 passed 36, failed 0, with 5 skipped. A command-lock lifecycle race was reproduced in the storage-history test and fixed; four focused lock tests, the history regression, and the final full suite passed. The migration completion persistence fix then passed its focused module 7/7. The final optimized bundle at `/private/tmp/silo-072-isolated-bundle-target/release/bundle/macos/Silo.app` compiled migration and GitHub restore-policy changes with identifier `org.silo.preview.migration-qualification`; the local bundle verifier and `codesign --verify --deep --strict` passed. The bundled CLI version and all five protocol probes verified; features were `net,ssh,embed-binaries`. Its `silo-ui`, `msb`, and `libkrunfw` SHA-256 values and preserved build log are recorded in `app/SiloUI/src-tauri/target/verification/packaged-macos-checkpoint-fork-restore-20260925/final-combined-package.txt` (untracked local evidence). The 1.8 GiB isolated build target was removed after verification; the small evidence remains.

The isolated bundle's first launch exposed missing Tauri ACL entries for the new migration and checkpoint commands. `build.rs` and `capabilities/preview.json` now explicitly register the seven commands; `command_permissions_tests.rs` checks the generated command registry and main-window allowlist. The focused regression passed. A separate migration regression verifies staged-inspection failures retain a static sanitized cause, validated sandbox name, and numeric exit code without raw command output, paths, or credentials; all six migration tests passed.

The qualification UI exercised interrupted migration, Retry, Show logs, acknowledgment-gated Continue, and restart using only disposable app data. The deliberately invalid fixture remained unavailable after failure; Continue switched to a clean runtime generation and restarted. Original fixture `machines.json` and workspace hashes matched the pre-action manifest at `app/SiloUI/src-tauri/target/verification/migration-qualification-source-before-continue.sha256.json`. The failure UI run preceded the sanitized-cause fix; its visible log was generic. The corrected cause has focused native regression coverage, but the updated failure screen was not separately rechecked visually.

The qualification app migrated a copied stopped `migration-proof` sandbox, displayed it Stopped in the normal overview, and restarted without the migration gate. The journal durably recorded 1/1 converted; after creating a fork, it stayed complete across Quit/relaunch although converted metadata now held 2 VMs. We explicitly started the source, created a full/manual checkpoint, forked it into a stopped sibling, and explicitly started that fork. Distinct active writable layers and source-only/fork-only files demonstrated storage independence. Restore created a full “Before restore” recovery checkpoint; after app restart the source remained stopped pending explicit Start, then restored the checkpoint sentinel and removed both post-checkpoint files. The fork remained stopped. The original copied workspace hash was `f4bc56a62aabe5430dffaf3cb3536c345b80e20df465d053db05fe44d0b87e7e`. Exact UUIDs, checkpoint records, file hashes, writable-layer paths, and results are in `app/SiloUI/src-tauri/target/verification/packaged-macos-checkpoint-fork-restore-20260925/RESULTS.txt` (untracked local evidence).

The installed `/Applications/Silo.app` was restored at its exact executable path. Its normal overview showed `dev` and `hermes` Stopped; no production VM was started. The qualification app was absent and both qualification guests were Stopped before test artifacts were pruned. The original source fixture remains at `/private/tmp/silo-uiq/runtime`; the isolated qualification app-data and the final bundle were removed after hashes and small evidence were preserved. No user app data was changed.

The GitHub restore-target selection passed a focused regression and synthetic signed-runtime full-snapshot test (1/1 each). An authorized live GitHub test passed 1/1 in 117.47 seconds using the connected account and a disposable repository. It captured a full checkpoint while the source had write access, assigned the fork the current read-only host profile before its first execution, then verified clone/read success and immediate issue-mutation/Git-push denial. The real host token did not enter the guest. Test issue, branches, temporary VMs, and child tokens were cleaned up. No unrelated repository was accessed.

Linux x86-64 acceptance ran in a disposable Ubuntu 24.04 container hosted by Ubuntu 26, on an ext4 task disk. KVM API 12 and actual `KVM_CREATE_VM` succeeded, and the ordinary AppImage smoke passed 10/10. The authentic predecessor fixture uses the installed Silo 0.6.3 / MicroSandbox 0.6.17 and Ubuntu 24.04 v2 guest. A HEAD-patched 0.6.17 host utility provisioned the guest's `silo` account; verification confirmed UID 1001, the SFTP server, preserved marker `legacy-source-before-checkpoint` with ownership 1001:1001, and a stopped VM carrying `silo.working-account=1`. The backup snapshot remains verifiable. Its first apt attempt failed because the original disposable VM had networking disabled; that VM alone was reconstructed from the verified 0.6 snapshot with the original workspace and supported `public` network profile, then the existing backup was resumed. Packaged Silo conversion passed 1/1. The production UI displayed it stopped; explicit Start, full checkpoint, stopped-fork/explicit-start, RAM-marker and guest-process survival, source/fork workspace independence, recovery-point creation, and stopped pending-restore across app restart passed. A pending-source recovery-fork bug was fixed and passed a real UI retry. The latest expanded run passed 11 lifecycle assertions through source disk/RAM/process rollback after restart. Linux qualification also fixed the short lifecycle-lock wait, completed-history re-quarantine, missing 4 GiB defaults and 0.7 config canonicalization, temporary snapshot-ancestry retention, snapshot-index reads truncated above 32 KiB, multi-member archive-head selection, snapshot selectors constrained by VM-name limits, and the disk-only snapshot-start flag. The production UI exported a 922,837,009-byte v3 archive; native head/integrity verification passed. The imported VM started explicitly and its workspace marker matched byte-for-byte, with source/fork post-checkpoint files absent. A repeated export originally failed because Silo created each capture in a fresh group without persisting lineage. Silo now stores the lineage group on each managed VM, carries it through import/fork/restore/relaunch, and captures backups in that group. Focused regressions passed; the final ARM64 same-home matrix exported source, imported VM, and fork twice each after one existing import. The ordinary 10/10 x86 AppImage smoke passed on package SHA-256 `f3ed57daaeccc7237780f4b78399222e5805413367671194c9a7d714fd811a6c`; the authentic predecessor migration and seven-export same-home matrix passed separately on runtime-7 package SHA-256 `f741e941c7d215835282ab7159b7ae32cef50bfba51311aceb5593ed9a4b1189`. A separate x86 fresh-home import/Start passed with the original source cache absent and destination VMDK paths verified, as recorded in the Linux acceptance research note. The native GTK folder chooser passed and wrote source-only archive SHA-256 `73791219b0b3fe2ecfed8a323480b96a4fa9c548d0692c214b680abdb432b2e1`. Do not infer that these checks all used one package. The later x86 Mousepad session proof passed on the desktop package recorded in the research note. The earlier x86 controller attempt returned zero raw SSH bytes after 25 seconds, before the final raw-stream flush fix. The positive live remote viewer subsequently passed on AppImage SHA-256 `8079943262a6a70e007403aa3900d1fd857080d63a04ea8dc4fb700dc5c7d8b2`: pinned-key SSH authenticated and exited 0, the running source viewer connected, and opening a stopped fork left it stopped. Compact evidence is in `app/SiloUI/src-tauri/target/verification/x86-final-desktop-20260927/remote-final/`. The final x86 runtime-8 AppImage and live port-control proof passed; see the qualification record below. A backup-history startup fix passed its focused regression. The final rebuilt AppImage, SHA-256 `f3ed57daaeccc7237780f4b78399222e5805413367671194c9a7d714fd811a6c` (211,974,648 bytes), passed the 10/10 ordinary smoke at `/work/evidence/appimage-smoke-final-42b4-extract`. A separate current-0.7.2 utility apply on a disposable v3 guest passed account, workspace, descriptor-path and snapshot verification; the focused 11-test suite covers interruption/resume, but the successful live run did not induce a second interruption. Its compact evidence is `app/SiloUI/src-tauri/target/verification/linux-account-migration-072-20260925.txt` (untracked local evidence). An earlier synthetic 0.7.2 VM staged in the old runtime directory is not predecessor compatibility evidence.

Linux ARM64 qualification completed on a disposable Ubuntu 24.04.4 ARM64 Lima VM on Apple Silicon with nested KVM. The authentic 0.6.17 Ubuntu 24.04 v2 guest migrated through the production UI and passed the 14-assertion checkpoint/fork/restore lifecycle. The final local AppImage is SHA-256 `14c4215d141c49399f44817a28edd0946d05e0a12c5fa86b468c831e8272c448`; its six managed ELF payloads in `usr/libexec/silo/tools` match the prepared hashes. A live AppImage WebKit smoke confirmed its `APPDIR` path, passed dependency preflight with all three tool rows checked, and passed native route, secret-form, autostart, and relaunch-persistence checks. The final local DEB (SHA-256 `127479a7939220c314c12ab83798e03507b2a1854dbf7213c97cb8f18a8694d6`) was installed only in the disposable guest. Its production IPC lineage matrix resumed an existing same-home import and fork without creating another import, exported source/import/fork twice each, and verified persisted groups across relaunch. All three had matching saved/native/raw-image capacity of 1 CPU, 1024 MiB RAM, and 4096 MiB. The imported VM's exact deny-all network profile remains intact and is accepted by backup validation; unrelated custom rules remain rejected. A separate cold-cache proof created one valid source archive through production IPC, imported it into a fresh app-data/HOME, then started the imported VM after both the original source alias and its canonical backing storage were absent. Root and workspace marker contents matched after Start. The destination held its own VMDK base image, raw managed root, and qcow2 overlay, with 10 GiB virtual root capacity. Archive SHA-256: `cbc27a90525091d2cd94fef88fd21da69e29d9ff59af59377c20666aa3637d12`. Compact cold-cache/package evidence is `app/SiloUI/src-tauri/target/verification/arm64-cold-cache-qualification-2026-09-26/` (untracked local evidence); lifecycle and lineage evidence remains in `app/SiloUI/src-tauri/target/verification/arm64-final-qualification-2026-09-26/` (untracked local evidence). These results cover an ARM64 Linux guest with nested KVM, not bare-metal ARM64 Linux. The native GTK chooser passed for a source-only archive export; see [Linux verification](SiloUI-LINUX-VERIFICATION.md). Unsaved graphical editor-buffer survival passed in the later x86 Mousepad checkpoint/fork/source-restore run recorded in the Linux acceptance research note. Release signing and distribution are publication steps outside this implementation qualification.

## Pinned runtime

Silo now pins the official [MicroSandbox v0.7.6 release](https://github.com/superradcompany/microsandbox/releases/tag/v0.7.6) (published 2026-10-01), source commit [`09df3d4b9d832adaede1fb9a198cfc660bfab8cd`](https://github.com/superradcompany/microsandbox/tree/09df3d4b9d832adaede1fb9a198cfc660bfab8cd). The pinned source archive is the commit archive `https://codeload.github.com/superradcompany/microsandbox/tar.gz/09df3d4b9d832adaede1fb9a198cfc660bfab8cd`; its SHA-256 is `3d076d83211e9755b17c0a1b93fadb3f201fcc4073e311ca139636ebebf827bb` (the tag-named archive has a different top-level directory and therefore a different digest, `c8fd2536d789d05ba28d00aa106e6137b0a1cb9da26dd9a5ab1ca0f631853ea9`; the build uses the commit archive). That source pins libkrunfw to [`cf4c22b9f05c680928e6d96a9d198f5845573a87`](https://github.com/superradcompany/libkrunfw/tree/cf4c22b9f05c680928e6d96a9d198f5845573a87), the same gitlink as 0.7.2 and 0.7.4, and the three libkrunfw release assets are byte-identical to the 0.7.2 and 0.7.4 ones; its release workflow names libkrunfw 5.6.1.

| Rust target | `msb` asset and SHA-256 | `agentd` asset and SHA-256 | libkrunfw asset and SHA-256 |
| --- | --- | --- | --- |
| `aarch64-apple-darwin` | `msb-darwin-aarch64` `af1dbfeff907a9b5784084dc01831e4e9bf856a57159fd93dc9bd4b009642644` | `agentd-aarch64` `f858a7b227308b40e06387b99c5f9a99f5fb9f1aeaa70f47080a8998b87dd2ba` | `libkrunfw-darwin-aarch64.dylib` `43e36ee2b1f2a7488c25f34193f657568a7733281d856ad267506ccc02993d59` |
| `aarch64-unknown-linux-gnu` | `msb-linux-aarch64` `ac0c8abce71ea901971807366bd4859d46d8bd0db8f9e8b6f1a54a8761812f93` | `agentd-aarch64` `f858a7b227308b40e06387b99c5f9a99f5fb9f1aeaa70f47080a8998b87dd2ba` | `libkrunfw-linux-aarch64.so` `98d01137190de7022a3132c6f55c245ef43d02d67d5d7e697ee19c303fce8769` |
| `x86_64-unknown-linux-gnu` | `msb-linux-x86_64` `e5baba0cbc6628a39e12e297729dfa1b137f3e7ce13a3fa9cf5fd22e198cab9c` | `agentd-x86_64` `78bb21c3bf16f195068c946ce72419d242fa2b3f1bebd2c339cc8e58dd79847d` | `libkrunfw-linux-x86_64.so` `ce9a749e8471e89aa5e2ad88de0c1581c3384c100bcb107a75bb12739a12d590` |

The release listing publishes SHA-256 values for these assets; for 0.7.6 every `msb`, `agentd` and libkrunfw asset in the table was downloaded on 2026-10-01 and hashed locally, and each digest equals the published one. The `checksums.sha256` listing digest is `168801c1d3bf2bf0df80bf2511e41af43c50e42fb3097b3830b4b1b32614b50a`. Silo applies sixteen ordered patches, each pinned by SHA-256 in `app/SiloUI/runtime-inputs.json`. Preflight validates their exact names, order, path containment, and bytes. The build cache key includes all patch hashes. Earlier macOS and Linux qualification cited above used the 0.7.2 runtime; it qualifies neither 0.7.4, 0.7.6 nor any rebased patch.

Ordered source patch pins (all `-0.7.6.patch`; "Feature" is a Silo-specific capability, "Fix" corrects an upstream defect):

| Patch | SHA-256 | Kind | Purpose |
| --- | --- | --- | --- |
| `microsandbox-silo-network-0.7.6.patch` | `45b890b9c2a095e4b0dde6da6341e856e06efb11307231547c5778407cff0eeb` | Feature | GitHub credential profile in the secrets handler; `ssh serve` and `exec` `--no-start`, managed authorized keys, machine-identity check and stdin-owned lifetime; the five original Silo protocol probes; SSH login directory is the user's home. |
| `microsandbox-restore-policy-0.7.6.patch` | `46294a8e6a2b4913795f268b936b536721a21f33f32f7e3834a06d0480ef9555` | Feature | Restore-time labels, environment defaults, host-sourced secrets and asymmetric network defaults, applied before restored execution. |
| `microsandbox-create-stopped-0.7.6.patch` | `7234d5319f05452e4cfc6355f21cedb0f50589df17bca13cdcaa2ed97816ca2c` | Feature | `create --no-start` and `--progress-json`: persist prepared storage as `Created` without running guest code, with credential-free progress. |
| `microsandbox-adopt-owned-disk-0.7.6.patch` | `0ddba4cb88627548ffbace1c0bd041bb5c86eba99ebe81279fe934d62b75dbcb` | Feature | `adopt-disk`: convert a stopped sandbox's disk-image mount to an owned managed volume. |
| `microsandbox-log-retention-desktop-start-0.7.6.patch` | `2929930c703291efd45d5807f44888b4272e612d27bc270b26b001d88b2712e6` | Feature | Log retention shared with stopped sandboxes and desktop-start log handling (`logging_retention.rs` is byte-identical to `src-tauri/src/log_retention.rs`, checked by a test). |
| `microsandbox-restore-root-capacity-0.7.6.patch` | `703df2e0330f45b653acb7027954ffff9f658ae7f9e86f6865d2ccba8e8608e9` | Feature | Restored managed and flat root disks declare the captured capacity (Silo's export check compares it). Rounds a non-MiB capacity up rather than rejecting it. |
| `microsandbox-portable-image-cache-0.7.6.patch` | `f627298c02524ea7bc5b7aeddc3020cdfaf0a4cd8afa87b56cae5b371ae478c9` | Fix | Rebuild the imported image VMDK against the destination cache; upstream keeps the exporter's absolute extent paths (reproduced, see [upstream bugs](#upstream-defects-confirmed-against-v074)). Compare existing image metadata by parsed content: it is serialized from a label `HashMap`, so two caches of the same image store different bytes and upstream refused the import. |
| `microsandbox-live-public-ports-0.7.6.patch` | `1a127057fbd0594ef6a08e3c7bd73143a4a123f73f3e293226dd84ab5525537f` | Feature | Add and remove public port publications on a running sandbox through the runtime control channel. |
| `microsandbox-secret-values-stdin-0.7.6.patch` | `304ba1069d8d69146f0b5adfde296003e10c7d5965d9eabe224970ab5c8aa790` | Feature | `MSB_SECRET_VALUES_STDIN=1`: secret `env` sources resolve only from a bounded JSON document on stdin; `--silo-secret-values-protocol` probe. |
| `microsandbox-import-stage-id-0.7.6.patch` | `094586917498660eb00a6b85dc177276367d39967205e803da26b4bddf74bfed` | Feature | `snapshot load --stage-id <32 hex>` so Silo can journal and clean exactly the staging paths of an interrupted import. |
| `microsandbox-sftp-user-0.7.6.patch` | `812987f168198e5708b4b6bae1a30e20b65f7c652f4855db5e6e6959cb1e618d` | Fix | Nonroot SFTP sessions run through the guest `sftp-server` under the SSH user; upstream runs them as root (upstream issue 1623). |
| `microsandbox-remove-created-0.7.6.patch` | `18b5dd57f15175fc4eae5824690cb9c5246d919a7f3dacc781af87bfa1a1bdbf` | Fix | `remove` also removes a sandbox whose status is `Created` (prepared, never started). Upstream's removal helper accepts only `Stopped` and `Crashed`, although the handle-level check and `destroy` already treat `Created` as removable; see [Removing a sandbox that never started](#removing-a-sandbox-that-never-started-2026-10-01). |
| `microsandbox-restore-starting-control-0.7.6.patch` | `b268376ed4cbf257f35d6f1d3559fbf154987572ce536544c28f94c25c9c2d5d` | Fix | A checkpoint restore of a sandbox with spare CPU or memory capacity reads its restored targets over a control session before the creator publishes `Running`; 0.7.6's session ownership check rejects a `Starting` sandbox, so every such full restore failed with `runtime session changed before request admission`. The restore alone may use a `Starting` session; see [MicroSandbox 0.7.6 upgrade](#microsandbox-076-upgrade). |
| `microsandbox-runtime-instance-id-0.7.6.patch` | `3a1c70aaebb60e73f1d803c4a4026f1585639307ca12309573e609102dc4b9f1` | Feature | `msb inspect --format json` reports `runtime_instance_id` (`<run id>:<started at>` of the active run, only while the handle's pid still matches it) through `SandboxHandle::runtime_instance_id`, and the `--silo-runtime-instance-protocol` probe announces it. Silo trusts only this identity to launch computer-use setup and to verify a worker for storage reclaim; see [Regression: missing `runtime_instance_id`](#regression-missing-runtime_instance_id-2026-10-02). |
| `microsandbox-checkpoint-fs-state-0.7.6.patch` | `6346dbdb79002a82526130a30c3c8f8cc17b061de4e9976636510f9f7eb2f885` | Fix | The checkpoint integrity check admits up to 8 MiB for a virtio-fs device state (the limit the runtime's restore already uses) instead of 1 MiB for every device, and only for that device type; the sum of all device-state objects is capped at 64 MiB, and a restore refuses a checkpoint that names more virtio-fs devices than the sandbox's configuration can construct before it reads any payload (see [Checkpoint device-state limits](#checkpoint-device-state-limits-2026-10-02)). The passthrough table of the read-only ChatGPT folder (4,479 paths) was 1.18 MiB after boot, so `snapshot create` of a running built-in VM failed with `checkpoint object exceeds 1048576 bytes`. Found by the live lifecycle test (`live_built_in_lifecycle_keeps_the_desktop_and_computer_use`); to be reported upstream. |
| `microsandbox-relay-closed-local-arena-0.7.6.patch` | `efbed4c5ea7b0e79e3745445b5dd54d5dc30590e530aad3a61840735f960ef10` | Fix | A guest bulk frame routed to a host relay client whose local shared arena was closed by that client's disconnect is dropped like any frame for a departed client; upstream treated `LocalShmError::Closed` as transport corruption, ended the shared guest reader and stopped the whole VM (see [Relay client disconnect during a bulk write](#relay-client-disconnect-during-a-bulk-write-2026-10-08)). Sent upstream as its own PR. |

The former ninth patch (`microsandbox-preserve-basic-auth`, an independent Basic Auth substitution policy plus `query_params` normalization) was dropped for 0.7.4 and stays dropped; the rationale is in [MicroSandbox 0.7.4 upgrade](#microsandbox-074-upgrade). Its 2026-09-27 verification (isolated `adopt-disk` on a copied catalog preserving `headers=true`, `basic_auth=true`, `query=false`, `body=false`) applies to the 0.7.2 runtime only.

The `secret-values-stdin` patch keeps secret values out of the runtime's environment (review items B-19/D-45 and B-28). Upstream 0.7.2 and 0.7.4 resolve every secret source of kind `env` from the `msb` process environment, which other processes of the same user can read, and which passes names chosen for secrets (for example `SSLKEYLOGFILE`) to the host runtime; its alternative `store` source kind is declared but unimplemented ("store-backed secret sources are not supported yet"). With `MSB_SECRET_VALUES_STDIN=1`, the patched CLI reads one bounded JSON object of source values from standard input before any thread starts, removes the flag so the sandbox process does not inherit it, and resolves `env` sources only from those values; the device environment is then never consulted, so a missing value fails closed. Without the flag, behaviour is unchanged. Silo sets the flag for every runtime command and sends the GitHub access profile (`SILO_GITHUB`) and the computer's assigned secrets this way, never as environment variables. The patch adds the `--silo-secret-values-protocol` probe, which the build requires. It changes the three places that read an `env` source (the network resolver, live secret rotation in `modify`, and the restore pre-check). On 2026-09-30 the ten-patch macOS CLI built (release, `net,ssh,embed-binaries`); the new `secret_values` unit test, the CLI probe test and the network resolver tests passed, and a smoke test of the built binary accepted a valid document, rejected malformed and oversized ones, and ignored standard input without the flag. It has not been exercised with a live VM on macOS or Linux, and the SDK's `modify` tests were not run.

The v0.7.2 release added, and v0.7.4 and v0.7.6 keep, the supported snapshot/restore surface used by Silo. The build checks `create --mount-owned`, `create --no-start`, `create --progress-json`, `exec --no-stdin`, snapshot creation, `restore --cow-mem` (0.7.6's name for `--forked`), the seven exact Silo protocol probes, and managed SSH. The CLI is built with `net,ssh,embed-binaries` so the verified `agentd` payload is included. Silo no longer carries the 0.6.17-only Imago storage override; the pinned Imago source of v0.7.2 and v0.7.4 preserves logical disk length during discard.

The manifest records the target, versions, release assets, packaged filenames, and staged-input hashes without claiming one cross-platform runtime path. The observed macOS bundle layout is:

```text
Contents/MacOS/msb
Contents/Frameworks/libkrunfw.5.dylib
Contents/Resources/microsandbox/manifest.json
Contents/Resources/THIRD-PARTY-NOTICES.md
Contents/Resources/microsandbox/licenses/*
```

The app, sidecar, and library are signed together. The app and `msb` carry Apple's [Hypervisor entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.hypervisor). Library validation is not disabled. Signing changes Mach-O bytes, so the release hashes prove staged inputs, while the macOS packaged integrity check must validate the app signature. The app bundle declares macOS 14.0; the upstream `msb` Mach-O declares 11.0.

The later launcher must use only these private paths and set `MSB_PATH`, `MSB_LIBKRUNFW_PATH`, and an app-controlled `MSB_HOME`. Upstream resolves both environment paths first in its [runtime configuration](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/config/mod.rs). It must not search `PATH` or a user's global MicroSandbox directory.

MicroSandbox is Apache-2.0. libkrunfw is LGPL-2.1-only and embeds GPL-2.0-only or compatible Linux sources. Exact license texts and source commits are in the bundle. Before external distribution, choose and review a compliant corresponding-source conveyance method. The current preparation is not release legal approval.

The `import-stage-id` patch adds `snapshot load --stage-id <32 lowercase hex digits>`.
Its exclusive snapshot and cache stage roots identify every unpacking and
publication directory before archive bytes are read, including external-base
archives. Silo journals `silo-import-<id>` first, passes that suffix as the stage
ID, and retries cleanup of precisely those paths and that group's indexed
members at launch. Missing paths are idempotent; collisions and symlinks are
refused. Random stages from older runtimes are preserved. Runtime preparation
requires the `--stage-id` capability for both cached and newly built executables.
The ordered patch SHA-256 above participates in the executable cache key and the
packaged manifest; no published release asset hash is substituted for a patched
build hash.

On 2026-09-30, this change ran `npm --prefix app/SiloUI run runtime:prepare`
in the isolated `fix/wd-e03` worktree. The release CLI at
`/Users/polarzero/code/projects/silo-wt/e03/app/SiloUI/src-tauri/binaries/msb-aarch64-apple-darwin`
has SHA-256 `a47001083c14e413614da1cc52ab577c6ff6002e659191567e63a08f3b562637`;
its version, six Silo protocol probes, and `--stage-id` capability passed runtime
preparation. The opt-in native test
`rebuilt_cli_killed_load_is_removed_by_next_launch_recovery` fed it a partial
1 GiB-declared tar disk entry through a FIFO, waited for actual extracted bytes,
and sent SIGTERM to that directly spawned child. Both exact stage roots survived
the killed load. Reloading Silo's durable journal and running production launch
recovery removed both roots, preserved three unrelated stage markers, and
cleared the import identity. No Silo app, VM, keychain, or user runtime was used.
This is fixture-only macOS CLI/recovery evidence, not an installed-app or Linux
qualification. Ordinary tests also cover a crash before spawning load,
preexisting-stage refusal, symlink refusal with journal retention, and the
inherited-worker lock. Logs are local in `/tmp/silo-codex/e03-*.log`.

## MicroSandbox 0.7.4 upgrade

Silo moved from v0.7.2 (`60d4dc8a436fb9365491567ec21d073e924e3c6d`) to v0.7.4 (`e36ffc0a58b48d70e0e4d66d75f1596994e3865a`, tagged 2026-09-29) on 2026-09-30. Upstream changed 320 files, including a rewritten `SandboxBuilder`/`create_sandbox` (builder-based creation), a config layer (`sdk/rust/lib/config/`), a cross-version compatibility layer for saved configurations (#1634, catalog migration `m20260922_000001_migrate_secret_config`), `msb snap` command aliases (#1619), `msb wait` (#1396), secret scanning scoped to request locations (#1666), and `allow_passthrough_for` renamed `allow_placeholder_for` (#1667). libkrunfw is unchanged. Every Silo patch was reapplied to the new source, rebuilt against the new APIs, and re-evaluated.

### Patch decisions

| Patch (0.7.2) | Decision | Evidence |
| --- | --- | --- |
| `silo-network` | Rebased | `crates/cli/lib/commands/ssh.rs` and `sdk/rust/lib/sandbox/ssh.rs` became async upstream; helpers and tests were merged by hand. Still no upstream `--no-start`, `--authorized-keys`, `--expected-machine-id` or GitHub profile. |
| `restore-policy` | Rebased | Applied cleanly. `--env` gained `-e` because a new upstream CLI test requires one short form for every repeated long flag. `allow_passthrough_for` became `allow_placeholder_for`. |
| `create-stopped` | Rebased | `create_sandbox` now takes a builder; the `start` flag is threaded through `create_with_mode`. No upstream create-without-start (`rg 'no_start\|LocalCreated'` finds nothing). |
| `adopt-owned-disk` | Rebased | Import lists and `HashMap` import merged. No upstream equivalent. |
| `log-retention-desktop-start` | Rebased | Applied cleanly on the new logging code; shared-rules byte check still passes. |
| `restore-root-capacity` | Rebased, one change | `apply_snapshot_root_layout` (`sdk/rust/lib/sandbox/builder.rs:2089-2110`) still declares `size_mib: None`. Non-MiB capacities are rounded up instead of rejected, because upstream's admission tests use 4096-byte fixtures that otherwise fail on the capacity error first. |
| `portable-image-cache` | Rebased | Applied cleanly. Defect still present at v0.7.4: `crates/image/lib/stitch/vmdk.rs:20-50`, `sdk/rust/lib/backend/local/snapshot/archive.rs:3711-3758`. |
| `live-public-ports` | Rebased | Applied cleanly; the network, protocol and runtime suites pass. |
| `preserve-basic-auth` | **Dropped** | See below. |
| `secret-values-stdin` | Rebased | Applied cleanly; upstream still reads `env` sources from the process environment (`crates/network/lib/model/config/resolver.rs`, `sdk/rust/lib/sandbox/modify.rs`). |
| `import-stage-id` | Rebased | Applied cleanly; `--stage-id` verified in the built CLI. |
| `sftp-user` | Rebased | Applied cleanly. Upstream SFTP still runs as the root agent (`sdk/rust/lib/sandbox/ssh.rs:1636-1690`); reported upstream as issue 1623. |

`preserve-basic-auth` had two jobs. First, accept saved 0.6.x secret configurations (`injection`, `query_params`, `on_violation`, `entries`). Upstream now does this: `packages/microsandbox-types/rust/lib/compat/v0_5_0/local/secrets.rs` and the catalog migration normalize every one of those spellings (tests `typed_reader_matches_saved_field_conversion` and the `config-0.6.18-*.json` fixtures). Second, keep Basic Auth as an independent scope. Upstream removed that scope on purpose: `legacy_header_scopes_merge_for_http1_and_http2` asserts that a saved `headers:false, basic_auth:true` policy becomes `headers:true`. That widens ordinary-header substitution for such a policy (reproduced against unpatched v0.7.4). Silo never writes such a policy: its secrets use `headers=true` with `basic_auth` true (0.6) or unset (0.7.2), and the one real record inspected was `headers=true, basic_auth=true, query=false, body=false`, which maps unchanged. Carrying a divergent secret-scope model through every future upstream compatibility change would cost more than the residual case, so the patch is dropped. Consequence: a hand-edited or third-party 0.6.x policy with Basic Auth on and ordinary headers off is widened when the catalog is upgraded.

### Upstream defects confirmed against v0.7.4

- Imported image VMDK descriptors keep the exporting machine's absolute cache paths. Reproduced without a VM: the patch's `imported_image_vmdk_uses_destination_cache_after_source_is_removed` test fails on unpatched v0.7.4 (`cold: VMDK does not point to imported extent`) and passes with `portable-image-cache`.
- SFTP through `msb ssh serve` acts as root for nonroot users. Upstream issue 1623 (open) reports it; Silo observed it live on Linux; the code path is `sdk/rust/lib/sandbox/ssh.rs:1636-1690` plus the root agent filesystem handler. Not reproduced in this change because it needs a running guest.
- `msb remove` refuses a sandbox whose status is `Created` (never started), with or without `--force`. Reproduced with the unpatched build of the eleven-patch tree (see [Removing a sandbox that never started](#removing-a-sandbox-that-never-started-2026-10-01)); fixed by `remove-created`.

Not defects: legacy Basic Auth widening (deliberate, tested upstream) and the omitted restored root capacity (default-size metadata that Silo's export check needs; the disk itself is not shrunk).

### Network `strict` and the export profile (2026-10-01)

MicroSandbox's network option `strict` ("require hostname-based policy allows to use inspectable application authority") flipped its default between the two runtimes Silo has shipped: it is `false` in 0.7.2 (`crates/network/lib/model/config/types.rs`, `#[serde(default)]` and `strict: false`) and `true` in 0.7.4. It does not exist in 0.6.17. It only changes a connection that a `domain` or `domain_suffix` allow rule admits (`strict_hostname_allow_is_opaque` in `engine/tcp/proxy.rs`, the TLS bypass in `engine/tls/proxy.rs`); group and CIDR rules, deny rules and default-allowed traffic ignore it, and Silo's profile (`guest/github-network-default.json`) has no hostname rule. GitHub token injection uses `secrets.allowed_hosts` and `require_tls_identity`, not `strict`.

The profile said `false`, so the export check (an exact comparison with the profile) refused every sandbox created by 0.7.4: `<name> has custom network strict settings that Silo cannot carry in an export.` Reproduced by feeding the real `msb inspect --format json` network of a sandbox created with Silo's argument list to the check (`backup::tests::export_accepts_the_network_of_sandboxes_from_every_origin`, failing before the change). Real inspect output of each origin, captured with the bundled 0.7.4 and, for older origins, the 0.7.2 and 0.6.17 binaries against throwaway `MSB_HOME`s, is kept in `src-tauri/src/test_support/msb-inspect/`:

| Sandbox origin | Saved `network.strict` | Why |
| --- | --- | --- |
| Created by 0.7.4 (`msb create`) | `true` | Runtime default; Silo now also passes `--net-strict=true`. |
| Restored, forked (full) or imported (deny-all) by 0.7.4 | `true` | `restore` builds its network from defaults, not from the source: restoring a sandbox saved with `false` gave `true`. `msb restore` has no `--net-strict` option, so Silo relies on the runtime default here. |
| Created by 0.7.2 (also after its `adopt-disk` by 0.7.4) | `false` | 0.7.2 saved its then-default; `adopt-disk` re-saves the typed configuration unchanged. |
| Created by 0.6.17 and migrated (`adopt-disk`) | `true` | The saved configuration has no value; 0.7.4 reads it as `true`, and `adopt-disk` writes `true`. Before `adopt-disk` the value is absent. |

Decisions. The profile and every computer Silo creates use `true`, set explicitly (`--net-strict=true` in `runtime::create_computer`, with a test tying it to the profile). Silo does not rewrite computers that carry `false`: the bundled `msb modify` has no network-strict option, so that would need a new runtime patch to change something nothing can observe. Instead the export and import comparisons (`backup::with_profile_strict`, used for the profile, the deny-all import network and the exported copy) treat a boolean or missing `strict` as the profile's value whenever the policy has no hostname rule, and an export carries the profile's value. That keeps archives made while the default was `false` (0.7.2 era exports record the profile with `false`) importable, and still refuses a non-boolean value or `strict: false` beside a hostname allow rule. The option stays out of the UI. Live check on 2026-10-01, with the bundled 0.7.4 and `strict` `true` and `false` (same guest image, Silo's network profile): `git ls-remote https://github.com/git/git HEAD`, `ssh -T git@github.com` (answering `Permission denied (publickey)`), HTTPS to `example.com` and `pypi.org`, and plain HTTP all behaved identically.

### Verification and pins not updated

- `npm --prefix app/SiloUI run runtime:prepare` (aarch64-apple-darwin, Rust 1.94.0, `net,ssh,embed-binaries`) rebuilt the patched CLI in 6m04s, passed the version, six protocol-probe, `--mount-owned`, `--no-start`, `--progress-json`, `--stage-id`, managed SSH, snapshot-create and forked-restore checks, and staged the runtime. Packaged `msb` SHA-256 `35a70e48d8eb95d68002b33f3fbe8952d20e042a6b0b70c98805f22c82d04585`. Silo's `snapshot create --from-sandbox` is still accepted (upstream keeps it as a documented alias of `--sandbox`), so no caller changed.
- Upstream suites on the rebased tree, with `HOME` and `MSB_HOME` in a temporary directory and no VM: network 606, runtime 410, types 112 + 10, image 256, protocol 63 + 12 + 3 + 4, db 17, migration 33, cli 380 lib + 20 binary + integration, SDK 1146 lib + 63 `snapshot_artifact` + 4 `api_compat`. Suites that boot VMs (most other `sdk/rust/tests`) were not run.
- Silo: `test:release`, preflight, typecheck, lint, the full Vitest run, the Python script suite and the full native suite (`--test-threads=1`, synthetic GitHub configuration) pass. `src-tauri/Cargo.toml` and `Cargo.lock` now pin `microsandbox-image` and `microsandbox-utils` at the new commit.
- Pins updated from the release without a Linux build: the Linux `msb`, both `agentd` and the libkrunfw digests (all downloaded and hashed). The Linux patched CLI is not built or verified here; it needs the `linux-packaging` workflow. No 0.7.4 VM, migration or installed-app qualification was run on any platform.
- Risks to check before a release: (1) upgrading an existing catalog applies upstream's `m20260922_000001_migrate_secret_config`, which rewrites saved secret configurations and refuses to roll back global passthrough defaults (not exercised against a real Silo catalog here); (2) exports made by a Silo bundling 0.7.2 are accepted (`EARLIER_IMPORTABLE_RUNTIME_VERSIONS` lists `0.7.2`; the 0.7.4 runtime loaded and verified a 0.7.2 export, both using `msb-snapshot-tar-zstd-v0.7`); (3) all lifecycle, checkpoint, restore, SSH and public-port behaviour rebuilt on the new builder is covered by unit tests only.

### Removing a sandbox that never started (2026-10-01)

Silo creates every sandbox with `msb create ... --no-start` (`create-stopped`), so a new
sandbox has runtime status `Created` until its first boot. Unpatched MicroSandbox 0.7.4
could not remove one: `msb remove <name>` exits 1 with `error: sandbox still running:
cannot remove sandbox "<name>": status is Created`; `--force` fails the same way and
`msb stop` leaves the status at `Created`. Silo's delete (`preflight_removal` and
`remove_computer_runtime` in `runtime.rs`) treats `Created` as stopped and runs `msb remove
--quiet <name>`, so deleting any never-started sandbox failed. The cause is
`remove_local_persisted_sandbox` (`sdk/rust/lib/sandbox/mod.rs`, v0.7.4 lines 1739 and
1783): both its status check and the recheck under the lifecycle locks accept only
`Stopped | Crashed`, while `SandboxHandle::remove`, which calls it, rejects only
`Starting`, `Running`, `Draining` and `Paused`, and `destroy` already skips its stop step
for `Created`. Upstream's own local create never writes `Created` (its entity calls it
"Cloud-only today"), so the gap is invisible upstream.

**Upstream (checked 2026-10-01).** v0.7.5 (2026-09-30) and `main` (`09df3d4b`, the 0.7.6
version bump) still read `Stopped | Crashed` in both places, and no issue or pull request
covers it. Moving the runtime pin is not part of this change: the pin is deliberate, and
0.7.5 does not contain a fix anyway. The issue and pull request text is drafted, unpublished,
in [the research note](research/microsandbox-remove-created-2026-10-01.md).

**Patch.** A new `microsandbox-remove-created-0.7.4.patch` (since renamed `-0.7.6.patch`), applied last (kind Fix), instead
of an extension of `create-stopped`. It changes one file, `sdk/rust/lib/sandbox/mod.rs`, and
does not depend on any other patch: it applies to unpatched v0.7.4 and to the twelve-patch
tree, and its tests pass on both. That keeps it a small, separately reviewable change that can be
sent upstream on its own, and a future change to either patch does not disturb the other's
pin. It adds `sandbox_status_allows_removal` (`Created`, `Stopped` or `Crashed`) and uses it for
both status checks. Everything else in `remove_local_persisted_sandbox` is unchanged: the
transition guard, the snapshot-lineage guard, the exact-identity check (`SandboxReplaced`), the
lifecycle-lock recheck, and the refusal of `Starting`, `Running`, `Draining` and `Paused`. A
`Created` sandbox has no run record, runtime process or socket, so there is nothing to tear down
beyond what removing a `Stopped` one already does (socket artifacts, `sandboxes/<name>`, and the
catalog row with its cascading rows). A concurrent first start of the same sandbox is excluded
by the same transition guard and lifecycle lock that `start` takes before it moves `Created` to
`Starting`; a create in flight holds both until it returns. No CLI flag or protocol probe is
added (a behaviour change has no flag to probe); the pinned patch hash in the manifest is the
evidence that a runtime has the change.

Tests inside the patch, next to the existing `persisted_removal_*` tests:
`persisted_removal_removes_a_sandbox_that_never_started` (a `Created`, a `Stopped` and a
`Crashed` sandbox, each with a private directory and a label row: directory, sandbox row and
label rows are gone) and `persisted_removal_still_refuses_a_sandbox_that_may_own_a_runtime`
(`Starting`, `Running`, `Draining` and `Paused` are refused with `SandboxStillRunning`, and
their directory and row stay). The first fails without the change with `SandboxStillRunning("cannot
remove sandbox \"never-started\": status is Created")` and passes with it.

**Which Silo flows leave `Created`.** Found by reading `runtime.rs`, `runtime/checkpoints.rs`,
`backup.rs` and the pinned start and restore source; not every path was run, because the
runtime cannot boot a guest here.

- Creating a computer. `create_computer_with_progress` runs the only production `msb create`, with
  `--no-start`. The sandbox stays `Created` until `verify_guest_tools` boots it once through
  `msb exec`, and Silo accepts `Created` or `Stopped` after that check. A failure before the
  first boot completes (the create itself, the status check, a first boot that fails its start
  validation) calls `cleanup_failed_create`, whose `msb remove --force --quiet` failed for a
  `Created` sandbox: the runtime record kept the name, and the managed workspace disk stayed too,
  because it is removed only after the runtime removal succeeds. A start that fails after it
  claims `Starting` ends `Stopped` instead (`start_sandbox`).
- Deleting. Whatever is `Created` when the user deletes it (the same leftovers, and records
  copied by the migration, which accepts `Created`) failed to delete.
- Interrupted imports by a released Silo (0.9.0 and earlier). They leave the sandbox `Created`
  over a partial disk; this is the orphan that `discard_released_import` removes.
- Not these: fork, restore and import with the current code keep a pending-restore record
  without any runtime sandbox until Start, and Start runs `msb restore`, which inserts `Starting`
  (`rollback_failed_startup` ends it `Stopped`, or removes the record). Checkpoints
  (`snapshot create`) do not change a sandbox's status.

**Silo side.** `discard_released_import` no longer has a `Created` special case: it removes the
sandbox through `cleanup_failed_create` and then the disk folder, and an interrupted import
reports "No sandbox was added. Import the file again." The "removed its disk but not its sandbox
record ... the name stays taken ... import under another name" wording and its test are
replaced by `recovery_removes_a_released_import_whose_sandbox_never_started`; the migration
fixture (`runtime_migration/interrupted_tests.rs`) now models the runtime that removes a `Created`
sandbox, and its two released-import shapes expect the same result as a stopped one. New in `runtime.rs`: `deleting_a_sandbox_that_never_started_removes_it_from_the_runtime_and_its_disk`
runs the real delete path (`apply_whole_configuration`) against a fake runtime that removes
`Created`, `Stopped` and `Crashed` sandboxes and refuses the rest, and checks that `msb remove
--quiet <name>` is issued, the record and disk go, and a running sandbox is refused by Silo before
`remove` is issued.

**Verification (macOS ARM64, Rust 1.94.0, no VM, throwaway `MSB_HOME`s under `/tmp`).**

- `npm --prefix app/SiloUI run runtime:prepare` rebuilt the twelve-patch CLI in 5m56s. Packaged
  `msb` SHA-256 `34eb037978dd86d113564b32bb29c81babb6ac58777cbbfcfb0eb19dee6a7d8c`;
  `--version` prints `msb 0.7.4`; all six Silo protocol probes print `1`; `--no-start`,
  `--progress-json`, `--mount-owned`, `snapshot load --stage-id`, the managed SSH flags, snapshot
  creation and forked-restore flags are present; the staged manifest lists the twelve patch hashes,
  the last being `d42071128d10d80c1b950aefbc3c673d1d2bf0115ef7534a9075c50cce83c355`.
- Reproduction with a control build (the eleven-patch tree without this patch, `cargo build
  --release`, SHA-256 `b7dfdcf9d7071eb6683cc7f302b1097bf9fbc8e506ef178b4cde52b3e58f22b7`): `msb create
  <empty rootfs> --name e2e-created --no-start` gives status `Created`; `msb remove e2e-created`
  exits 1 with the message above, `remove --force --quiet` exits 1, `msb stop` leaves `Created`, and
  creating the name again fails with "already exists".
- The same script against the packaged `msb`: `remove` exits 0 (`Removed e2e-created`), so does
  `remove --force --quiet`, `msb list` is empty, the name can be created and removed again, and
  the only database rows left are msb's own maintenance lease.
- Parity with `Stopped`: two sandboxes with a 1 GiB owned workspace disk, one left `Created`, one
  set to `Stopped` in the catalog (a VM cannot boot here). Each removal deleted exactly its
  `sandboxes/<name>/owned-volumes/work_*/disk.raw` and its `sandbox` and `sandbox_labels` rows, and
  left the same lock files; the home was then equal to an empty one apart from msb's own lock and
  database files.
- Refusal: a catalog row `Running` with a run record naming a live process (this script's own
  `sleep`; reconciliation keeps a Running row whose recorded pid is alive). `msb remove` exits 1
  with `sandbox still running: cannot remove sandbox 'e2e-live': still running`, as it does for
  `Starting`, `Draining` and `Paused`; the sandbox directory, sandbox row and run row stay. After
  the process ended, the row reconciled to `Crashed` and `remove` succeeded.
- `cargo +1.94.0 test --locked -p microsandbox --lib persisted_removal` on the twelve-patch tree and,
  separately, on pristine v0.7.4 with only this patch: 5 passed each. With the helper reverted to
  `Stopped | Crashed` the new acceptance test fails with the error above. The full SDK library
  suite on the twelve-patch tree (`cargo test --locked -p microsandbox --lib`, 275 s) passed 1148
  tests with 14 ignored and none failing. The patch is `rustfmt` clean (`cargo fmt --all
  --check` on the pristine tree with the patch). Clippy with `-D warnings` on that tree reports
  pre-existing diagnostics under Rust 1.94.0 (11 in the library tests, one in each of two
  integration tests), none in `sandbox/mod.rs`.
- Silo: the full native suite (`cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked`,
  synthetic GitHub configuration, default threads) passed 1141 tests with 15 ignored in 71.5 s;
  `cargo +1.94.0 fmt --check` is clean; `npm --prefix app/SiloUI run test:release` ran 71 tests
  (65 passed, 6 skipped, none failed; also under Node 24.11.1), the runtime Vitest file passed 13,
  and the Python release-cache and toolchain tests passed 5. These use fixtures and fake runners,
  not the rebuilt runtime. The packaged app (`desktop:build`) was not built.

**Not verified.** No VM was booted, so a running sandbox was not refused by a real runtime (the
refusal above uses a live process as the runtime stand-in). The Linux CLI was not built; the patch
is plain Rust and applies to the same source, but the `linux-packaging` workflow has not run it.
The packaged app, a real interrupted import over a migrated home, and the delete of a sandbox
whose creation failed were not exercised live. A remote computer that runs an older Silo, whose
bundled `msb` lacks this patch, still cannot delete a never-started sandbox.

## MicroSandbox 0.7.6 upgrade

Silo moved from v0.7.4 (`e36ffc0a58b48d70e0e4d66d75f1596994e3865a`) to v0.7.6 (`09df3d4b9d832adaede1fb9a198cfc660bfab8cd`, the release commit of 2026-10-01) on 2026-10-02; v0.7.5 (2026-09-30) was skipped, so this covers both. Upstream changed 291 files. The changes that matter to Silo: the piped-stdin `msb exec` hang is fixed (#1674, upstream issue 1549) and `exec`/`run` gained `--no-stdin` (#1688); `msb branch` became `msb fork` and `restore --forked` became `--cow-mem` (#1708; the old names stay as aliases, and the deprecated `--forked` prints a warning even with `--quiet`); host paths in a creation request are made absolute and pinned when the sandbox is created (the security merge `b3843eb1`); denied HTTP and HTTPS egress can answer `403`, opt in only (#1489, #1694) and the saved network configuration now also carries `http` and `nat64_prefixes`; the published-port accept queue is configurable (#1606) and a guest FIN now reaches the host peer (#1706); placeholders are allowed on trusted secret hosts (#1700); and the control channel gained framed generation-two operations (#1669) together with a stricter session-ownership check. libkrunfw is unchanged (same gitlink `cf4c22b9`, byte-identical release assets, 5.6.1). The Rust toolchain is unchanged: upstream pins none (edition 2024, no `rust-toolchain.toml`), and the tree built, tested and ran with the documented 1.94.0.

### Patch decisions

Each patch was applied to the 0.7.4 source as one commit each, then rebased onto 0.7.6 so that git resolved context moves; the resulting per-patch diffs are the pinned files. None of Silo's patches is in 0.7.6: `rg` finds no `no-start`, `adopt-disk`, `stage-id`, `ports_list`, `MSB_SECRET_VALUES_STDIN`, `expected-machine-id`, `exit-on-stdin-close`, `sftp-server` or `SILO_GITHUB` under `crates`, `sdk/rust` or `packages` at the tag, and `remove_local_persisted_sandbox` still accepts only `Stopped | Crashed` (`sdk/rust/lib/sandbox/mod.rs:1753`, `1797`). Upstream fixed the `Created` removal as #1724, merged after the tag (`74848ac3`, on `main` only), so `remove-created` stays until a release contains it.

| Patch | Decision | Evidence |
| --- | --- | --- |
| `silo-network` | Rebased, hand-merged | The secrets handler was restructured (an ineligible secret no longer carries a substitution record). Silo's block that granted every location to a GitHub placeholder on an allowed host is dropped: the new code lets a verified destination receive its placeholder unchanged, and an unverified one is blocked everywhere, which is the behaviour the block produced. The TLS proxy gained the 403 deny path; both it and Silo's policy-change cancellation are kept. Network suite: 642 passed. |
| `restore-policy` | Rebased, no conflict | Applied cleanly. |
| `create-stopped` | Rebased, hand-merged | The builder now captures host paths and runs creation inside `with_backend`; the `start` flag is threaded through unchanged. A new upstream test calls `create_sandbox` and needed the extra argument. |
| `adopt-owned-disk` | Rebased, no conflict | |
| `log-retention-desktop-start` | Unchanged | Same bytes and digest as 0.7.4. |
| `restore-root-capacity` | Rebased, no conflict | |
| `portable-image-cache` | Unchanged | Same digest. The defect is still present at v0.7.6. |
| `live-public-ports` | Rebased, hand-merged | `publisher.rs` changed substantially: listeners are bound through `bind_listener` with the configured accept queue (live additions now use the same queue size), the guest FIN is propagated by dropping `to_host`, and `ControlOperation` replaced the old request enum, so the control handler needs explicit `ports_list`, `port_add`, `port_probe` and `port_remove` arms. The live-port tests in `publisher.rs` pass. |
| `secret-values-stdin` | Rebased, no conflict | |
| `import-stage-id` | Rebased, no conflict | |
| `sftp-user` | Unchanged | Same digest; upstream issue 1623 is still open. |
| `remove-created` | Rebased, no conflict | Not in 0.7.6 (see above). |
| `restore-starting-control` | **New** | See below. |

### Regression found and fixed: full restore with spare CPU or memory

A full-memory restore of a sandbox created with `--max-cpus` or `--max-memory` above its boot size fails on unpatched 0.7.6 with `error: control client error: runtime session changed before request admission (delivery: NotSent)`, and the restored VM is stopped. Silo creates every sandbox this way, so every checkpoint restore and fork would have failed. Reproduced with the official `msb-darwin-aarch64` release binary (no Silo patch) and with the patched build; the same steps pass with the official and the Silo-patched 0.7.4. Cause: `restore_requested_resources` (`sdk/rust/lib/sandbox/modify.rs`) reads the restored CPU and memory targets inside `create_sandbox_inner`, before the creator publishes the sandbox as `Running`. In 0.7.4 it used an unverified control request. 0.7.6 routes it through `control_session`, whose `current_run` accepts only `Running` and `Draining` sandboxes, so the check always fails (identified with a temporary trace at `owner.rs:221`). The patch adds `control_session_while_starting`, which also accepts `Starting`, and uses it only in that restore step; process-birth, database and run identity checks are unchanged. Its test seeds a `Starting` sandbox, shows that an ordinary session is still refused, and reads a restored CPU target; it fails without the change. Candidate for an upstream issue and pull request; drop the patch when a release fixes it.

### Call sites

- One `msb` argument used by Silo was renamed: a checkpoint Start passed `restore --forked` (the deprecated alias, which prints a warning even with `--quiet`) and now passes `--cow-mem`, with the old test assertions updated. Nothing passes `branch` or `fork`, and `restore` is otherwise called only with `--name`, labels, environment, secrets and network defaults. Snapshot scope names did not change between 0.7.4 and 0.7.6 (`manifest.rs` of `microsandbox-types` is byte-identical): the loaded descriptor spells its scope `file` or `checkpoint`, while the index and `snapshot list` spell it `disk` or `full`. Silo's import comparison accepts both spellings (`backup::descriptor_scope_supported`); it had accepted only the index names, so every checkpoint import was refused until this fix. `snapshot create --from-sandbox` (alias of `--sandbox`), `snapshot save` and `snapshot load` (aliases of `export` and `import`) still work, so no deprecation text reaches parsed output. A real `restore` and `snapshot` run printed none.
- The runtime capability checks (`scripts/microsandbox-runtime.mjs`, cached and fresh build) now require `restore --cow-mem` (the hidden `--forked` no longer appears in help) and `exec --no-stdin`.
- `--no-stdin`: Silo's runner now inserts it before the `--` separator for every `exec` that passes `--no-tty` and not `--tty` (`runtime::runtime_arguments`, applied where the process is spawned). Silo never forwards input to `msb exec`: its only standard input is the secret document, which the runtime consumes at startup, and the interactive terminal uses `--tty`. The flag gives the guest EOF without starting a stdin forwarder, so an open pipe cannot hold the command up. It is applied centrally, and only to the bundled local `msb`: the SSH remote bridge talks to the other computer's own runtime, which may be older and would reject the flag. Unit tests cover the argument rewrite and the arguments that reach a spawned fake `msb`.

### Export profile and archive compatibility

- 0.7.6 saves `http: {"deny_response": false}` and `nat64_prefixes: ["64:ff9b::/96"]` in every sandbox's network configuration. Silo's export check compares the network with the exact profile, so every sandbox created by 0.7.6 failed to export with `custom network http settings`. `backup::with_profile_defaults` (formerly `with_profile_strict`) now treats those two fields at their defaults as absent; any other value is still a custom setting and still blocks. Real `msb inspect` network output of a 0.7.6 create, restore, forked full restore and deny-all restore is kept in `src-tauri/src/test_support/msb-inspect/*-0.7.6.json` and checked with the existing origin matrix.
- Exports made by a Silo bundling 0.7.4 (the shipped release) must stay importable. `EARLIER_IMPORTABLE_RUNTIME_VERSIONS` now lists `0.7.2` and `0.7.4`. Checked with real binaries on 2026-10-02: archives written by the official 0.7.2 and 0.7.4 (`snapshot save --with-parents --with-image`) loaded into a fresh 0.7.6 home (`snapshot load --group --stage-id`), verified (`msb-file-merkle-blake3-v1`), restored, and showed the workspace marker written before the export. Importing archives of two different runtimes into one home fails on the second (`cache target already exists with different content`) on 0.7.4 as well; it is not new.

### Regression: missing `runtime_instance_id` (2026-10-02)

Silo's 0.6.17-era patch added `runtime_instance_id` to `msb inspect --format json`: `<run id>:<started at>` of the active database run, reported only while the handle is `Running` and its pid equals the run's pid (`SandboxHandle::runtime_instance_id`). It was dropped when the patch set was rebuilt (commit `d654e4bc`) and was in neither the 0.7.4 nor the first 0.7.6 patch set, so the upgrade shipped without it and nothing noticed.

Consumers (grep `runtime_instance_id`): `computer_use::running_identity` (the identity a computer-use apply is bound to) and `runtime::storage` (`after_boot` records the verified start; `verified_worker` accepts a reclaim only for that same running instance). Without the field `running_identity` returned `None`, so the after-boot computer-use sync was never launched (a built-in VM stayed "not set up"), and the storage reclaim after a start was silently skipped.

Fix: `microsandbox-runtime-instance-id-0.7.6.patch` (the 0.7.6 form of the old change, plus the `--silo-runtime-instance-protocol` probe). Guards so it cannot regress silently:

- `runtime:prepare` runs `msb --silo-runtime-instance-protocol` with the other six probes, on both a fresh build and a cached one, and refuses a runtime that does not answer `1`. Preflight pins the patch name and order, and its test pins the last patch.
- Silo reports a runtime that lacks the capability instead of skipping: the patch always writes the `runtime_instance_id` entry (`null` while no active run matches), and `InspectedSandbox` records whether the entry was present at all. `runtime::running_instance_id` returns the reported instance; for a running sandbox whose output has no entry it returns `Unavailable` ("The bundled runtime does not report running instances ... computer-use setup and storage reclaim are disabled. Reinstall Silo.") and prints one `eprintln` line per runtime executable. A stopped sandbox, or a capable runtime that has not established the instance yet (`null`), stays a quiet `None`. Unit tests in `runtime/storage/tests.rs` cover these cases.
- The build-time probe states the capability; it does not run a VM. The Silo-side check reads the real `inspect` output of every running sandbox, so a regression is visible at the first start rather than never.

Live check (2026-10-02, bundled `msb` rebuilt with the patch, ad-hoc signed with `Entitlements.plist`, throwaway `/private/tmp` homes, `e2e-*` and `silo-account-proof` sandboxes only): `msb inspect --format json` of a running sandbox printed `"runtime_instance_id": "2:2026-10-02 14:59:38.584408"`. The live built-in computer-use test (`live_built_in_computer_use_sets_up_and_survives_export_and_import`) started a built-in VM through the production Start path and its computer use reached `ready` (the after-boot sync launched, which it never did without the field). That run's second half (import into a second home) was cut short because another session deleted the shared published ChatGPT folder it read, so only the source half counts. The unit suite, including the storage verified-worker tests, passes.

### Checkpoint device-state limits (2026-10-02)

Review finding: with the first form of `checkpoint-fs-state`, a crafted full checkpoint naming up to 4,096 device references (the manifest's `MAX_COMPONENTS`) of about 7 MiB each passed the integrity check, and the runtime's restore (`decode_devices`, `crates/runtime/lib/checkpoint/restore.rs`) then retained every decoded payload, about 28 GiB, before the device inventory was compared with anything. The 8 MiB allowance only matters for virtio-fs state, so the patch now bounds the whole path:

- `crates/image/lib/checkpoint/resolver.rs`: `verify_device_states` keeps 1 MiB per device and 8 MiB for virtio-fs (type 26) only, and refuses the checkpoint once the sum of device-state objects exceeds `MAX_TOTAL_DEVICE_STATE_BYTES` (64 MiB; a reference counts every time it is named). Tested with a 2 MiB state: refused for a block device, accepted for virtio-fs, 32 of them accepted, the 33rd refused as `aggregate`.
- `crates/runtime/lib/checkpoint/restore.rs`: `validate_device_inventory` runs on the manifest alone, right after the closure opens and before any payload is read, and rejects a checkpoint that names more virtio-fs devices than `max_virtio_fs_devices(vm)` (new, in `runner/vm.rs`: the root and runtime-share transports plus one per mount, owned volume, file mount and embedded backend). `decode_devices` charges every payload to the same 64 MiB budget before decoding it, so at most the budget plus one object is ever retained; the error names the device. The check is an upper bound, not an exact match: the runtime's later device restore still rejects a device the VM does not have.
- 64 MiB is generous for legitimate state (the ChatGPT folder's table was 1.18 MiB) and ten times smaller than what the 4,096 x 7 MiB case needed.

Live check: `runtime:prepare` rebuilt the fifteen-patch CLI with the new patch pin; the packaged `msb`, ad-hoc signed with `Entitlements.plist`, in a throwaway `/private/tmp` home with the guest image and an `e2e-fs` sandbox that mounted a read-only host tree of 13,321 entries and listed it, took `snapshot create --full --guest-flush required --integrity` (2.5 s; the virtio-fs state object was 2.8 MiB, above the old 1 MiB limit) and `restore --cow-mem` with the mount passed again; the restored sandbox kept its RAM-only marker file and listed all 13,321 entries. Both sandboxes and the home were removed.

Upstream tests run on pristine `09df3d4b` with only this patch: `cargo +1.94.0 test --locked -p microsandbox-image --lib checkpoint::resolver` (9 passed, including the new aggregate test) and `-p microsandbox-runtime --lib checkpoint::` (94 passed, including `device_inventory_bounds_virtio_fs_devices_before_any_read` and `decoded_device_state_is_bounded_in_total`); `cargo fmt --all` clean.

### Relay client disconnect during a bulk write (2026-10-08)

Closing the Linux desktop viewer ends its `ssh -N -L` (ProxyCommand `msb ssh serve <name> --stdio`), so that relay client disconnects. If the guest was streaming bulk data to it at that moment, the VM stopped. `runtime.log` showed `agent relay: local bulk writer slot=1 failed: Broken pipe`, then `agent relay error: agent relay: local shared-arena output failed: local shared arena connection is closed` and `Vmm is stopping`.

Cause: `route_guest_lane_frame` (`crates/runtime/lib/runner/relay.rs`) clones the client's route, including its local shared-arena producer, under the clients lock and calls `try_prepare` after releasing it. The client's disconnect cleanup closes that producer, so `try_prepare` can return `LocalShmError::Closed`. Only `Full` was tolerated; any other error ended the shared reader, the relay returned an error and `vm.rs` stopped the VM. `closed` is set only by `close()`, and the only `close()` on a client's `local_outbound` is in that client's disconnect cleanup, so `Closed` never stands for a transport-wide failure.

Patch: `relay-closed-local-arena` (kind Fix, applied last) signals that client's disconnect and drops the frame, releasing its lane and client budgets, as the existing no-client and stopped-writer paths already do. Other local-arena errors still fail closed. No wire, launch or persisted-state change. It applies to unpatched v0.7.6 and to the fifteen-patch tree; the same change, with the test adapted to upstream `main`'s `ClientState`, is upstream [superradcompany/microsandbox#1796](https://github.com/superradcompany/microsandbox/pull/1796).

Verification on 2026-10-08 (Linux x86-64, Rust 1.94.0, sixteen-patch tree): the new `guest_bulk_to_closed_local_arena_drops_frame_without_failing_reader` fails without the fix with the error above and passes with it; `cargo test -p microsandbox-runtime --lib runner::relay` passed 88/88. The same day on macOS (aarch64-apple-darwin, Rust 1.94.0): `npm --prefix app/SiloUI run runtime:prepare` built the sixteen-patch CLI and passed every probe; packaged `msb` SHA-256 `095d639f18f01eaf4e7df54dbb258493d6b15306a38de4b310d8821d8bc1b6a0`. `typecheck`, `lint`, Vitest (2636 tests), `cargo +1.94.0 fmt --check`, the native suite with synthetic GitHub configuration (1997 passed, 27 ignored) and `test:release` (128 passed, 12 skipped) passed. A `desktop:build:debug` Silo Dev bundle then ran a throwaway computer (`e2e-relay`) with its desktop. Ten viewer close cycles during continuous full-screen terminal output left the computer running with no relay error, but the same ten cycles on a fifteen-patch runtime (the previous build, without this patch) did not fail either, so viewer closing alone rarely hits the race. A stress loop that reproduces it: the guest serves an endless stream on a TCP port, the host opens the viewer's own SSH forward to that port with four concurrent readers, then kills the `msb ssh serve` ProxyCommand with SIGKILL after 0.5 s. On the fifteen-patch runtime the VM stopped after 307 iterations with the logged sequence above (`local bulk writer` broken pipe, `merger cleanup failed`, `local shared-arena output failed: local shared arena connection is closed`, `Vmm is stopping`). On the sixteen-patch runtime 1000 iterations logged ten `local bulk writer` broken pipes and no `local shared-arena output failed` or `Vmm is stopping`, and the computer stayed running in Silo. No Linux build or Linux live check was run, and the installed production app was not used.

### Verification (2026-10-02)

- `npm --prefix app/SiloUI run runtime:prepare` (aarch64-apple-darwin, Rust 1.94.0, `net,ssh,embed-binaries`, fifteen patches) built the patched CLI, passed the version, seven protocol probes, `--mount-owned`, `--no-start`, `--progress-json`, `--stage-id`, managed SSH, `--no-stdin`, snapshot-create and `--cow-mem` checks, and staged the runtime. Packaged `msb` SHA-256 `95e3367ae84eb7fa86158a46b2ca730f245d35d7d99599da18bf8aea3c93f953`.
- Upstream suites on the thirteen-patch (before `runtime-instance-id` and `checkpoint-fs-state`, added later the same day) tree, with `HOME` and `MSB_HOME` in a temporary directory and no VM: network 642, runtime 411, image 257, types 112, protocol 63, cli 383 + 21, SDK lib 1106 + 63 `snapshot_artifact` + 5 `api_compat`; all passed. Suites that boot VMs were not run.
- Smoke test of the packaged `msb` (copied, ad-hoc re-signed with the hypervisor entitlement, throwaway `MSB_HOME`, fixture guest image `ubuntu-24.04-v3-arm64`, only `e2e-*` sandboxes): `image load`; `create` with Silo's arguments and `--no-start` (status `Created`); `start`; `exec --no-tty --no-stdin`; a never-closed stdin pipe returned immediately with and without `--no-stdin` (the official and the Silo-patched 0.7.4 `exec` hung on an inherited open stdin during a background run of the same steps, until it was terminated); `snapshot create --full` and a disk snapshot; a full restore with Silo's policy, secret and environment arguments (RAM and disk markers survived); a disk restore; a read-only `-v` mount of a canonical `/private/tmp` directory (read worked, write refused, inspect reports the same absolute path); a relative `-v ./rel` stored as an absolute path; `remove` of a never-started sandbox. A sandbox created by the patched 0.7.4 opened in place with 0.7.6 (`Stopped`, `strict` true, started, executed), and its 0.7.4 full snapshot restored with Silo's arguments on 0.7.6.
- Silo: `typecheck`, `lint` (no errors), the Vitest suite, `test:release`, the Python script suite, `cargo +1.94.0 fmt --check` and the full native suite with synthetic GitHub configuration pass; see the commit messages for counts. A debug bundle was built into a separate target directory; the Silo Dev app itself was not launched.
- Not done: no Linux binary was built (the Linux `msb`, both `agentd` digests and the libkrunfw digests are pinned from the downloaded release assets; the `linux-packaging` workflow must build and test it), no installed-app, two-computer or Linux VM qualification, and no upgrade of a real Silo catalog.
- Open observations: a full restore of a sandbox whose source carries a secret fails (`restore virtio device virtio_fs1`) unless the restore passes `--secret` again, identically on unpatched 0.7.6 and on 0.7.4; Silo always passes it. A full restore of a sandbox with a host bind mount refuses without `--volume` rebinding, as before. Silo's pre-v4 sandboxes have none; a built-in computer-use sandbox (v4 or later) has exactly one, the read-only ChatGPT app folder at `/opt/silo/chatgpt`, and every restore (import, transfer, checkpoint restore, fork) must rebind it with the same `-v`, then verify the restored sandbox reports it read-only. See the [integration contract](SiloUI-CHATGPT-APP.md#integration-2026-10-02).

## Pinned Git distribution

Silo packages the required client runtime from the matching tar archive in [dugite-native v2.53.0-4](https://github.com/desktop/dugite-native/releases/tag/v2.53.0-4), commit [`4098283a7ecb8a227b9d43580336c78a06f90e5d`](https://github.com/desktop/dugite-native/tree/4098283a7ecb8a227b9d43580336c78a06f90e5d). Dugite-native is the portable Git distribution maintained for GitHub Desktop. The selected release is its current, GitHub-signed release and supplies upstream SHA-256 values. Its [stated roadmap](https://github.com/desktop/dugite-native/blob/4098283a7ecb8a227b9d43580336c78a06f90e5d/README.md#roadmap) tracks stable Git updates. This makes the pin suitable now, but release work must still monitor new Git and dugite-native security releases. Silo retains [Git 2.53.0](https://github.com/git/git/tree/67ad42147a7acc2af6074753ebd03d904476118f), [Git LFS 3.7.1](https://github.com/git-lfs/git-lfs/tree/b84b33847fe6458f36ef521534dc0eac953cb379), Git's HTTPS transport, templates, and the Linux CA certificate bundle. It excludes Scalar, Git Credential Manager, server programs, and unrelated helpers. Host pushes use only short-lived Silo-controlled credentials.

| Rust target | Dugite-native archive | SHA-256 |
| --- | --- | --- |
| `aarch64-apple-darwin` | `dugite-native-v2.53.0-4098283-macOS-arm64.tar.gz` | `f9dc64635a5b62fbd7ad95db73268bbb8912255ac516d65d37bf7af22fcb8ffe` |
| `aarch64-unknown-linux-gnu` | `dugite-native-v2.53.0-4098283-ubuntu-arm64.tar.gz` | `a161f45af4626bb7e0c688854bd4a9aee47cc514bca404cff0a5e3536ef1c0af` |
| `x86_64-unknown-linux-gnu` | `dugite-native-v2.53.0-4098283-ubuntu-x64.tar.gz` | `cca76aa31ad9e835e771ee7f55b73934777fbd8d16757a10d307ba06de860901` |

Preparation uses the same target selection as MicroSandbox, verifies the complete upstream archive before extraction, rejects unsafe archive paths and unsupported targets, retains the explicit client-runtime allowlist, checks Git, Git LFS, HTTPS transport, templates, certificates, executable modes, and contained symbolic links, then replaces `src-tauri/runtime/git/`. It also materializes target-qualified Tauri sidecars for Git and each helper under the ignored `src-tauri/binaries/` directory. Materializing `git-remote-https` removes the upstream helper symlink before relocation, so no packaged link can point into the staging or build tree. Generated archives, caches, sidecars, and extracted files remain ignored. Node and `tar` are build-time tools only; the Tauri app has no Node runtime and performs no runtime download.

On macOS, Tauri packages the executables next to the app executable and signs each one through its established sidecar path. Linux packages place managed tools under `/usr/libexec/silo/tools`, as described in [Linux payload integrity](#linux-appimage-payload-integrity). Tauri packages templates, licenses, the manifest, and the Linux certificate bundle under the resource directory. The native adapter resolves these fixed packaged paths:

```text
<executable directory>/git
<executable directory>/git-lfs
<executable directory>/git-remote-http
<executable directory>/git-remote-https
<resource directory>/git-support/share/git-core/templates
<resource directory>/git-support/ssl/cacert.pem       Linux only
<resource directory>/git-support/manifest.json
<resource directory>/git-support/licenses/*
```

It must invoke the packaged `git` by absolute path. It must set `GIT_EXEC_PATH` to the executable directory, `GIT_TEMPLATE_DIR` to the private template tree, and the Linux `GIT_SSL_CAINFO` to the private certificate bundle. Its fixed process environment must set `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_SYSTEM=/dev/null`, `GIT_CONFIG_GLOBAL=/dev/null`, a Silo-owned `HOME` and `XDG_CONFIG_HOME`, and a fixed `PATH` containing the executable directory and only required operating-system utility shims. It must never inherit the user's `PATH` or Git configuration. Required credentials must be supplied for one operation by the native adapter, not persisted in the VM.

Every packaged Mach-O executable must pass individual signature verification after packaging. A successful `git --version` or outer `codesign --deep` check alone does not establish this. Tauri does not re-sign executable files copied as resources, so Git, Git LFS, and both HTTPS helper names are configured as external binaries instead. Tauri's [macOS bundler source](https://github.com/tauri-apps/tauri/blob/tauri-bundler-v2.9.4/crates/tauri-bundler/src/bundle/macos/app.rs) adds every external binary to the inner-to-outer signing list. It signs each with the configured identity and hardened-runtime setting before it signs the outer app. The [Dev configuration](../app/SiloUI/src-tauri/tauri.dev.conf.json) uses ad-hoc signing with `hardenedRuntime: false`. Optimized local macOS builds retain hardened runtime and use the [exact-engine VM signing policy](SiloUI-RELEASES.md#local-macos-bundles). Distributable releases use the configured distribution identity and the release guide's signing procedure. Git declares macOS 11.0 and Git LFS declares macOS 12.0. Both are below Silo's declared macOS 14.0 minimum.

The macOS archive contains no private dynamic libraries in the retained runtime. Git links to the system CoreServices and CoreFoundation frameworks plus `libz`, `libiconv`, and `libSystem`. Its HTTPS helper also links to system `libcurl` and `libexpat`. Git LFS links to `libSystem`, `libresolv`, CoreFoundation, and Security. These are platform libraries covered by Silo's macOS minimum.

The Linux archives were built on Ubuntu 22.04 and reference glibc 2.34. Git directly requires glibc and `libz`; Git LFS is static. The HTTPS helper directly requires glibc, `libz`, and `libcurl.so.4`. The Tauri Debian configuration now declares `libc6 (>= 2.34)`, `libcurl4 | libcurl4t64`, and `zlib1g`; the RPM configuration declares `glibc`, `libcurl`, and `zlib`. Tauri's RPM dependency setting cannot express a minimum version, so RPM release validation must separately reject glibc older than 2.34. Tauri's [AppImage bundler source](https://github.com/tauri-apps/tauri/blob/tauri-bundler-v2.9.4/crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs) creates Debian-style data first, placing every external binary in `usr/bin` before linuxdeploy scans the existing ELF files and copies non-baseline shared-library dependencies into the AppDir. A Linux AppImage build must still verify that the produced image contains a usable `libcurl.so.4` chain. MicroSandbox alone can run with glibc 2.28, but bundled Git raises Silo's combined Linux floor to glibc 2.34.

### Linux AppImage payload integrity

The pinned `@tauri-apps/cli` 2.11.4 path copies Debian payloads into the AppDir and invokes linuxdeploy with the GTK plugin and AppImage output. The Tauri v2.11.4 [AppImage bundler source](https://github.com/tauri-apps/tauri/blob/tauri-v2.11.4/crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs) inherits the build process environment when it launches linuxdeploy. linuxdeploy upstream supports `NO_STRIP=1` to skip its ELF stripping step; its maintainers confirm that option in [issue #72](https://github.com/linuxdeploy/linuxdeploy/issues/72). Set it on the Tauri build process only when before/after evidence attributes a packaged payload hash change to stripping. It does not disable RUNPATH changes: upstream linuxdeploy enumerates ELF files in `AppDir/usr/bin` and recursively under `AppDir/usr/lib`, then assigns relative RPATHs to those files ([scanner and rewrite code](https://github.com/linuxdeploy/linuxdeploy/blob/master/src/core/appdir.cpp)). `--exclude-library` filters dependency deployment, not those existing-file rewrites. Tauri's `bundle.linux.appimage.files` copies additional files into the Debian data tree before this scan, so it is not an exclusion switch; a destination outside the scanned directories requires moving the product's runtime lookup there and avoiding the normal `externalBin`/`resources` placement. The Linux package-only overlay now does this for managed ELF tools: preparation copies the five hash-verified external binaries and `libkrunfw.so.5.6.1` to ignored `runtime/linux-package/tools/`; AppImage, Debian, and RPM custom-file mappings install them under `/usr/libexec/silo/tools`. Packaged Linux runtime and dependency checks resolve that shared directory; normal development sidecars, resource manifests, guest image, Git support, help, notices, release metadata, and the user-installed `silo-remote` bridge retain their existing routes. Compare prepared runtime hashes, AppDir payload hashes, and the extracted AppImage against embedded manifests before accepting a package; if hashes still differ, inspect ELF dynamic sections and bytes to identify the transforming step. Never bypass Silo's packaged integrity checks.

Git and dugite-native use GPL-2.0. Git LFS uses MIT plus its recorded component terms. The Linux CA bundle is curl's conversion of Mozilla's CA store and uses MPL-2.0. Exact pinned license texts are packaged under `git-support/licenses/`. External distribution still requires legal review and a compliant corresponding-source offer for GPL components.

## Approved push boundary

The native implementation supports two push routes:

- When computer pushes are enabled, tools in the computer may push through Silo-controlled GitHub access. GitHub credentials remain on the host.
- When computer pushes are disabled, guest pushes stay blocked. The user may select commits and click Push in Silo, which pushes from the host.

App Push never grants standing push permission to the computer. It uses the bundled Git and Git LFS, standard Git transfers, only required committed Git/LFS data, and incremental transfer where the protocol supports it. The [GitHub implementation](SiloUI-GITHUB-IMPLEMENTATION.md#native-implementation) describes credential forwarding and host Push. The current [host adapter](../app/SiloUI/src-tauri/src/host_push.rs) checks authority before pushing the confirmed branch and commit through isolated bundled Git.

## Resource and VM backup UX recommendation (2026-09-08)

Historical recommendation against MicroSandbox 0.6.17, not current export behavior. The later implementation supports checkpoints and exports while a computer runs; see [checkpoint qualification](SiloUI-CHECKPOINTS-PLAN.md) and [export and import testing](SiloUI-DEPENDENCIES-BACKUP-TESTING.md#export-a-computer). The original recommendation follows.

- Onboarding should show genuine compatibility and packaged-runtime checks. There is no measured basis for a universal 16 GiB RAM or 20 GiB free-space gate. Check capacity when creating, starting, backing up, or restoring a selected VM. Show an actionable shortage at that operation; do not add a permanent capacity checklist or invent a minimum when no defensible requirement exists.
- RAM admission must account for the selected VM and host pressure, rather than treating unused RAM as the available budget. [Apple documents memory pressure](https://support.apple.com/guide/activity-monitor/view-memory-usage-actmntr1004/mac) as a combination of free, cached and wired memory and swap activity; Linux documents `MemAvailable` as an estimate accounting for reclaimable memory in [procfs](https://docs.kernel.org/filesystems/proc.html). Performance estimates warrant warnings, not unsupported hard minimums. Runtime overhead and any reserve still require measurement.
- Disk admission should use the selected operation's missing image data, copied/expanded data, archive overhead and temporary storage, checking each affected volume. Account for sparse disks and shared image caching. Do not assume a compression ratio or promise that a successful precheck reserves disk space. Report write failures truthfully and preserve existing backups.
- Ship VM backup through the already bundled MicroSandbox snapshot/archive implementation. It includes tar and zstd internally, so users need no archive commands and Silo needs no second archive engine for this initial managed-VM backup path. The [archive implementation](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/snapshot/archive.rs) supports image inclusion and parent inclusion, sparse disk export, temporary output and transport integrity. Select a self-contained export including the required image and ancestors, and preserve Silo's VM configuration alongside it.
- Recommend managed disks for Silo-created VMs. The pinned [snapshot creation implementation](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/snapshot/create.rs) rejects running/paused VMs, resumable snapshots, flat disks, user-owned disk-image roots and memory-only roots. Gracefully stop a running VM with a clear interruption notice, capture the immutable disk snapshot, then restart it if it was previously running while exporting the captured snapshot. Never describe this as saving running programs or promise interruption-free backups.
- A managed-disk snapshot does not include arbitrary host folders or additional external disks. Keep Silo-owned workspace data inside the managed VM disk for the simple complete-backup path. If a VM uses external storage, identify that exception and explicitly include it through a later supported path or report the backup limitation; never label omitted VM data as a complete backup.
- Before release, prove restore into a fresh compatible VM from the exported artifact with the original VM and image cache unavailable. Verification must cover VM configuration and disk data, failed export, restart failure, and unsupported storage. Source inspection establishes a suitable implementation path; it is not a completed backup/restore test.

## Dependency assessment

This assessment and the proposed checks below describe the 2026-09-08 snapshot, when native Git/Git LFS invocation and real backup operations were absent. The assessment is preserved as decision evidence. Current [dependency probes](../app/SiloUI/src-tauri/src/dependencies.rs) invoke bundled `git --version` and `git-lfs version` in an isolated environment; [host Push](../app/SiloUI/src-tauri/src/host_push.rs) invokes bundled Git for committed-object transfers. Follow [dependencies, export and import testing](SiloUI-DEPENDENCIES-BACKUP-TESTING.md) for current behavior.

| Current row | Finding | Recommendation requiring approval |
| --- | --- | --- |
| `git` | Approved and packaged for future host-side pushes. Native push and preflight invocation are not implemented. | Keep the current row unchanged. Later read-only preflight must run only bundled `git --version`. |
| `git-lfs` | Approved and packaged with Git for future host-side LFS pushes. Native push and preflight invocation are not implemented. | Keep the current row unchanged. Later read-only preflight must run only bundled `git-lfs version`. |
| `tar / gtar` | No current native consumer. MicroSandbox implements snapshot tar handling in Rust in its [snapshot archive module](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/snapshot/archive.rs). Silo does not use the separate upstream installation shim that shells out to tar. | Remove from onboarding now. Select a library when Silo's real backup boundary is implemented. |
| `zstd` | No current native consumer. The same upstream snapshot module performs zstd compression internally. | Remove from onboarding now. Select a library when Silo's real backup boundary is implemented. |

The remaining thresholds also lack a current SiloUI requirement:

- `macOS 26+`: unsupported by the package evidence. The app declares 14.0, but compatibility still needs a real oldest-host test before changing the label.
- `Apple Silicon`: supported. The pinned upstream macOS release has only an arm64 artifact.
- `20 GiB free`: no measured Silo plan supports this fixed minimum. Upstream currently defaults each managed writable root disk to 4 GiB in its [sandbox configuration](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/sandbox/config.rs), but actual image, workspace, and backup budgets vary.
- `16 GiB memory`: no measured Silo plan supports this fixed minimum. Upstream defaults one sandbox to 512 MiB in its [global configuration](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/config/mod.rs).

Decisions pending at that snapshot:

1. May we remove the `tar / gtar` and `zstd` onboarding rows and defer each check to its actual feature?
2. May we replace or defer `macOS 26+`, `20 GiB free`, and `16 GiB memory` after compatibility and capacity tests establish real thresholds?

That preparation left every row, label, layout, and interaction unchanged. The later dependency implementation replaced those fixture checks; the questions above are historical, not outstanding approval requests.

## Proposed real checks

These are the original 2026-09-08 implementation requirements. The native
[dependency module](../app/SiloUI/src-tauri/src/dependencies.rs) now implements
the probes; its unit tests and the dated packaging records below state their
verification boundaries.

1. Resolve the app-private manifest, sidecar, and library by fixed platform layout; validate the manifest schema, pinned target and versions, files, and the macOS bundle signature or Linux staged hashes. Run only packaged `msb --version` with fixed arguments, bounded output, private environment paths, an empty `MSB_HOME`, and fixed per-probe and total timeouts; require its output to match the pinned version.
2. On macOS require macOS at or above the bundle minimum, `arm64`, and Apple's documented [`kern.hv_support`](https://developer.apple.com/documentation/hypervisor) value of `1`. On Linux require the CPU to match a packaged `aarch64` or `x86_64` target, glibc 2.34 or newer for the combined package, and [`KVM_GET_API_VERSION`](https://docs.kernel.org/virt/kvm/api.html#kvm-get-api-version) on `/dev/kvm` to return `12`; close the descriptor without calling `KVM_CREATE_VM` or otherwise creating a VM. MicroSandbox alone supports glibc 2.28 under its [upstream platform requirements](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/docs/cli/overview.mdx), but bundled Git raises Silo's current Linux floor to 2.34.
3. Treat missing, unsupported, permission-denied, timed-out, malformed, and invoke-failure results as unsuccessful; none may pass by fallback or absence.
4. Add read-only bundled `git --version` and `git-lfs version` probes to the remaining native preflight integration. Resolve only the private paths and isolated environment above. These probes prove the selected versions, not HTTPS helper or push usability. Keep deterministic fixtures isolated to tests and explicit fixture launches. Do not add archive-tool, installation, repair, sandbox-creation, legacy handshake, or `msb doctor` checks.
5. Add focused native success and failure tests. Assert the exact complete state and that `Continue` enables only after every required check succeeds. Assert the exact failure detail and remediation, disabled `Continue`, and keyboard Space toggling of the disclosure's `aria-expanded` state and detail visibility.

## MicroSandbox packaging verification record

- `npm --prefix app/SiloUI test -- src/test/microsandbox-runtime.test.ts src/features/onboarding/components/onboarding-preparation.test.tsx src/features/onboarding/onboarding-app.test.tsx src/features/onboarding/onboarding-flow.test.tsx src/features/onboarding/onboarding-recovery.test.tsx src/features/onboarding/onboarding-source.test.tsx`: 6 files and 67 tests passed.
- `npm --prefix app/SiloUI test`: 40 files and 369 tests passed.
- `npm --prefix app/SiloUI run typecheck`: passed.
- `npm --prefix app/SiloUI run lint`: passed.
- A disposable fresh-checkout simulation started with the ignored `src-tauri/binaries/` and `src-tauri/runtime/` inputs absent. `npm run desktop` executed `beforeDevCommand`, preparation restored the target-qualified sidecar, manifest, library, and licenses from the validated cache, and a fail-fast Rust wrapper confirmed those files existed at the first Rust invocation. The wrapper then exited intentionally, so this check did not launch the app or another Vite server.
- `cargo test --offline --manifest-path app/SiloUI/src-tauri/Cargo.toml`: 37 tests passed.
- `npm --prefix app/SiloUI run desktop:build:debug`: passed; built `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app`.
- `codesign --verify --deep --strict --verbose=2 "app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app"`: passed. The app, `msb`, and libkrunfw also passed strict component verification; `msb` has `com.apple.security.hypervisor=true`.
- Isolated packaged `msb --version`: returned `msb 0.6.17` and wrote no files under empty `HOME` and `MSB_HOME` directories.
- Exact bundle executable launched with a fresh `SILO_SETTINGS_DIR`, wrote only its isolated `settings.json`, quit through AppleScript, returned status 0, and left no `silo-preview` process.
- `TAURI_ENV_TARGET_TRIPLE=aarch64-unknown-linux-gnu npm --prefix app/SiloUI run runtime:prepare` and the x86_64 equivalent selected the requested cross-build artifact pairs and matched all pinned hashes. Read-only `debian:bookworm-slim` arm64 and amd64 containers each returned `msb 0.6.17` with staged paths mounted read-only and no files under empty `HOME` or `MSB_HOME`.
- Playwright captured current complete collapsed/expanded and dependency-failure collapsed/expanded states at 1160 by 820 under `src-tauri/target/visual-baseline/`. The before screenshots were overwritten, so this evidence verifies current semantics and styling, not a pixel comparison. Keyboard Space toggled the disclosure. Navigation, shared row styling, Applications controls, and footer controls remained unchanged; only approved dependency content and occupied height changed. This is fixture UI evidence, not native preflight evidence.

That MicroSandbox work did not test Linux Tauri bundling, KVM access, VM startup, or oldest-supported-macOS behavior. Its Linux execution coverage was limited to read-only `--version` under Docker. No VM, installer, repair, sandbox, or `msb doctor` command ran.

## Git packaging verification record

- `npm test -- --run src/test/git-runtime.test.ts src/features/onboarding/components/onboarding-preparation.test.tsx`: 2 files and 19 tests passed. These cover target selection, hashes, unsafe archives and links, missing helpers, modes, relocation, manifest paths, Tauri sidecar signing inputs, Linux dependency declarations, and the unchanged dependency view.
- `npm run typecheck` and `npm run lint`: passed. The debug build also reran the TypeScript and Vite production builds successfully.
- `npm run desktop:build:debug`: passed. The bundle is `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Preview.app`. A stale ignored `target/debug/git/` directory from the earlier resource layout blocked one rebuild and was removed; a fresh build has no file/directory collision.
- `codesign --verify --deep --strict --verbose=2` passed for the app. Individual strict verification passed for `git`, `git-lfs`, `git-remote-http`, and `git-remote-https`. Tauri replaced every upstream signature with the app's ad hoc debug identity and hardened-runtime signature. The packaged helpers are regular files, not staging-path symlinks.
- `node scripts/verify-git-runtime.mjs ".../Contents/Resources/git-support" ".../Contents/MacOS"` passed with an empty isolated home, disabled system and global Git configuration, a private Git/helper path plus one `/bin/sh` shim, and no global Git executable. Packaged Git returned `git version 2.53.0`; packaged Git LFS returned 3.7.1; the HTTPS helper reached its usage path. A disposable bare remote received one standard Git push and its LFS object. The second identical push reported no changes; this is a repeat/no-op check, not a measurement of incremental-transfer efficiency.
- Target-qualified preparation passed for `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`. Disposable Ubuntu 22.04 arm64 and amd64 containers had no global Git, installed only the declared `libcurl4` validation dependency, and ran the exact staged Git, Git LFS, and HTTPS helper. Each pushed Git and LFS data to a local bare remote and completed an unchanged second push. The arm64 artifact reports `git version 2.53.0.dirty`; the manifest requires that exact upstream output.
- ELF symbol inspection confirmed glibc 2.34 for Git and the HTTPS helper on both Linux targets. The helper names `libc.so.6`, `libz.so.1`, and `libcurl.so.4`; Git LFS has no glibc symbol requirement.
- The production UI source did not change. No screenshots were written or overwritten. The existing Git and Git LFS rows remain unchanged.

Tauri's AppImage source path was inspected: it creates Debian-style data first, so the external Git executables are in `usr/bin` before linuxdeploy scans existing ELF files. This coverage does not include a produced Linux Tauri bundle, so it does not prove the final AppImage's libcurl chain. It also excludes an actual HTTPS or GitHub push, real credentials, VM forwarding, KVM, a VM, native preflight commands, oldest supported hosts, notarization, and release-identity signing. It does not implement either approved push route. No user repository changed, and no installer, repair flow, sandbox command, or `msb doctor` ran.


## Git LFS pure SSH publishing server (2026-09-16)

Host-authorized publishing stages a Linux guest helper from
[charmbracelet/git-lfs-transfer](https://github.com/charmbracelet/git-lfs-transfer/tree/971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b),
commit `971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b`. The source archive SHA-256 is
`92d6720202aa5a059c6683df78f1fa47722c0c48ff1dc4ebfc0bc8137d988702`.
There is no stable release asset; the only published binary release is a mutable
2023 nightly. Preparation therefore verifies the pinned source archive and builds
it with Go 1.25 or newer, `CGO_ENABLED=0`, `GOOS=linux`, `-mod=readonly`,
`-trimpath`, and `-buildvcs=false`. The host target selects ARM64 or AMD64 guest
code. Runtime preparation stages `runtime/lfs-transfer/`; Tauri packages it under
`git-support/lfs-transfer/`. The manifest records the source pin, archive hash,
guest architecture, and executable SHA-256. Source and verified builds are cached
under `target/runtime-cache/git-lfs-transfer/`; dependency modules are checked
against upstream `go.sum` and the public Go checksum database.

The package includes the upstream MIT license, compiler runtime license, and
license/notice files for the external modules actually linked for that guest
architecture. The Go compiler is a build dependency, not an application runtime
dependency. The helper is copied to an operation-specific guest temporary directory;
existing computers need no image rebuild.

The upstream server uses a conventional `lfs/objects` tree and does not resolve
`lfs.storage` or linked-worktree configuration itself. Silo asks guest Git LFS for
`LocalMediaDir` and exposes that directory through an operation-specific server
view. Git LFS continues to choose and validate objects through its native protocol.
An absent guest cache uses an empty view, preserving the guest repository state.

Prototype evidence uses bundled Git LFS 3.7.1 and the pinned server over a local
SSH shim. It covers empty content, objects reachable only from earlier commits,
source-pruned objects already present at the destination, and new objects missing
from both locations. The final ordinary `git lfs push` rejects missing required
objects. Pure SSH fetch can exit successfully with server-side objects absent;
fetch success alone does not establish completeness. On two historical 6 MB
versions, a cold source transfer used 12,001,358 protocol bytes; a repeat with an
existing host cache used 275 bytes. Local timings were 0.209 s and 0.157 s. These
measurements establish cache bandwidth savings, not live computer or SSH-network latency.

B-28 source mapping: each general secret keeps its guest name and placeholder,
but the CLI records an opaque `SILO_SECRET_<number>` source. The number is the
big-endian decimal value of the first 128 bits of SHA-256 of the guest name; it
is stable across assignment order, additions and removals. Silo sends values
under these generated names on stdin and refuses duplicate sources. The CLI's
create, modify and restore adapters use the same mapping only when stdin
transport is enabled. `SILO_GITHUB` remains the reserved GitHub protocol source.
The shared reserved-name list guards guest settings, independently of transport.

Primary-source gap verified in the pinned
[CLI network adapter](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/crates/cli/lib/commands/common.rs):
the parser persists the guest name as its environment source and offers no
separate source-name option. This small bundled upstream patch supplies the
mapping at the existing adapter seam; no credential broker or storage service
is added. The source patch retains the upstream license and existing runtime
build, packaging, and protocol-probe checks.

The build and smoke-test record above predates the generated source mapping.
For the mapping revision, the focused upstream secret-values test and Silo's
transport and secret-configuration tests pass. The patched CLI compiled with `net,ssh,embed-binaries`; its five secret-parser
tests and protocol-probe test passed. The patch applied through the production
build helper and every manifest patch digest matched. Live VMs remain unverified.

## Inactive-disk migration (review D-25)

The owned-disk adoption patch accepts Created, Stopped and Crashed sandboxes.
Migration inspects the staged copy first: MicroSandbox reconciles a dead Running
process to Crashed, then Silo admits that inactive state. Active states remain
blocked. Adoption copies and verifies the external disk without starting guest
code or changing lifecycle status. Source files remain intact.

The pinned [MicroSandbox reconciliation source](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/backend/local/sandbox/mod.rs)
checks process ownership before marking a stale run Crashed. Its
`test_reconcile_sandbox_runtime_state_marks_dead_processes_crashed` regression
and the adoption patch's fixture tests exercise this boundary without a computer.
Silo's migration regression requires exactly inspect, adopt-disk and inspect.
These tests do not qualify live migration or a packaged app.

### Verification of D-25 and F-08 patches (2026-09-30)

`npm --prefix app/SiloUI run runtime:prepare` rebuilt the ten-patch runtime
in the isolated `fix/wc-patches` worktree with Rust 1.94.0, Node 24 and Go 1.25.
The macOS ARM64 sidecar SHA-256 is
`99b8b7ed9b2c346a88a7b6b0c8425c9f06a28f02b2b5a2aebeebf2571fdac6cd`.
Its manifest matches every current patch pin. Version, all six Silo protocol
probes and `adopt-disk --help` passed. A disposable catalog/disk fixture with
no guest image exercised the built sidecar: direct Crashed adoption passed;
a stale Running row inspected as Crashed and then adopted successfully.
Both paths retained Crashed status and preserved original, staged and owned
disk bytes. No VM or Silo app was started.

The upstream adoption and dead-process reconciliation tests passed, as did
11 logging tests and five execution-log tests. The shared retention source
and patch passed the Vitest byte-identity and digest checks. Flood regressions
failed with the old shared budget, then passed with independent 125 MiB
execution and console budgets. Silo's migration regression likewise failed
before admitting Crashed, then its 16-test migration suite passed. These are
fixture, build and CLI results, not live migration or packaged-app qualification.

### Adoption from the previous generation (2026-10-01)

Migration no longer stages a copy of each `volumes/<name>/workspace.raw`. It
runs `adopt-disk` without `--source`, so adoption reads the disk the staged
sandbox already names in the previous generation and writes only the owned
copy. The staged copy had stayed in the converted storage as an unused
duplicate. On filesystems without reflinks, such as ext4, it held as much
space as the workspace, and the upgrade briefly needed three copies.

Silo's migration suite asserts the bare `adopt-disk <name>` call, no staged
workspace disk, a copied configuration-ownership marker and an unchanged
previous generation. The duplicate assertion failed with the old copy. On
macOS ARM64, the bundled sidecar (SHA-256
`101118e812ea79127c5ecff900100591b7d8cf569df66db590867643bef12576`) adopted a
`--no-start` fixture's 8 MiB sparse disk without `--source`. The sandbox stayed
Created and the mount became Owned. The owned copy matched byte for byte. The
source's hash, size, mtime and inode were unchanged. No VM or Silo app was
started; this is not live migration or packaged-app qualification.

### The previous generation stays closed (2026-10-01)

`<app data>/runtime` is a pre-upgrade backup, and downgrading is not
supported. Until the migration is `complete` or `not-required`, nothing may run
`msb` against it or open its database, and after "Continue" into a fresh runtime
it holds the only copy of the unconverted sandboxes. A newer `msb` upgrades the
database it opens in place: a live check on 2026-10-01 launched a development
build over a database made by the released 0.6.17 runtime. The old `msb.db-wal`
and `msb.db-shm` changed, `seaql_migrations` gained the three 0.7.4 migrations,
and 0.6.17 then refused the home with "database schema is newer than this msb
binary". A running `msb` could also tear the conversion's copy of the database.

Three launch paths had reached the old home through `runtime_paths`, which
resolved to `runtime/` until `runtime-generation.json` existed. The storage
monitor ran its periodic inspection whenever migration blocked operations (its
branch was inverted). The main window's data source starts polling outside the
migration screen, so `read_network_state` inspected every saved computer every ten
seconds. Quit listed managed computers before stopping them. An empty window with no
frontend still modified the database at launch.

`runtime::runtime_paths` is now the only way to name a runtime, and it refuses
with "Finish the Silo runtime migration before using sandboxes." unless the
migration is `complete` or `not-required`; a record that needs manual repair
refuses too. The staged conversion uses `runtime::migration_runtime_paths`, which
names only `runtime-checkpoints-converted`. Callers that merely look for running
computers (Quit, update checks, update preparation, health checks) treat the refusal as
"nothing is running", so a failed migration cannot stop Quit or an update.
An interrupted export or import must settle before the migration may start
(E-50), so its recovery and the backup service, which keeps its runtime command
for the process, get `runtime::inert_runtime_paths`: the same files, but no
executable, so `msb` cannot start. `runtime_migration/guard_tests.rs` covers every
status, the Continue generation, a fake-`msb` process that must never start, the
inert paths, the staged conversion, and a scan that fails if production code
builds `RuntimePaths` elsewhere or names the previous folder.

Recovery through the inert paths had still written to the previous generation (the
runtime alias, `.silo-backup-worker.lock`, and for an import that owned a loaded
snapshot group, that group's load stage), and a journal that owned an export capture
or an import group failed closed, so the migration could not start while the export
page that could dismiss it was hidden. An interrupted export or import now settles in
two steps, and the first never writes to the previous generation:

1. **First launch of the upgrade: what needs no runtime**
   (`backup_controller/recovery.rs`, `settle_before_migration`). It waits for a
   surviving child's lock read-only, removes the working files and the partial export
   file the operation left elsewhere, keeps a finished export file, and reports an
   import whose settings were saved as complete. When nothing is left for the runtime,
   the journal records its result ("Export/Import interrupted before the upgrade", or
   cancelled) and stays in place for the export and import page.
2. **What only the runtime can remove waits for the converted storage.** An export
   capture (`pending_capture`), a loaded import group or the computer identity of an
   unfinished import (`group`, `id`) are in the previous generation, so the migration
   copies them into the converted generation with everything else. Giving them up
   would waste that space and could make a later import under the same name fail, so
   the journal is kept, pending, with `awaitingUpgrade` set (written only when set,
   and cleared when the result is recorded). The marker is what the migration
   recognises: `wait_for_migration_recovery` and the journal check in `convert_with`
   no longer wait for such a journal, and `quarantine_previous_backup_state` leaves it
   where it is when it selects the converted generation. The migration ends in a
   restart. The next launch finds the migration `complete`, so `recover` runs the
   ordinary `recover_at_paths` with the paths of `runtime::runtime_paths`, which names
   the converted generation: it removes what the operation left from that copy and
   reports as after any relaunch ("Import interrupted", "Export cancelled", "Export
   complete" when a finished file verifies). The pre-upgrade backup keeps its copy of
   what was left, untouched, until it is deleted.

   If that cleanup fails, the journal stays and the failed-recovery behaviour applies
   as for any interrupted operation (relaunch to retry, or dismiss to stop retrying).
   The migration already completed and is never reopened by it. "Continue" (the clean
   generation) keeps its previous behaviour: it isolates every unfinished journal,
   waiting ones included, in the previous folder, and nothing is cleaned there or in
   the clean generation, since the previous folder then holds the only copy of the
   sandboxes that were not converted.

   *Paths.* The journal names no path inside the previous generation: it holds names,
   identities, an import group and an export capture's group and member (the only paths
   are the export file and its folder, which the user chose). Recovery recomputes every
   path from them through `runtime::runtime_paths`, and MicroSandbox resolves the
   snapshot selectors (`group:member`) from its current home, never from a path stored
   in the database. The converted database does still hold the previous generation's
   absolute paths, copied unchanged: `snapshot_index.artifact_path`, and the disk path
   of each sandbox record. Silo selects by group and member, and by sandbox name and
   identity, never by those paths; MicroSandbox's `remove` deletes the sandbox's own
   directory below its home and its record, and leaves the disk image at the configured
   path alone. The tests refuse a runtime command whose storage is not the converted
   generation.

   *A released import.* Silo 0.9.0's import journaled the new sandbox's identity, claimed
   `volumes/<name>` with an `.silo-restore-owner` marker, wrote the disk and ran
   `msb create` over it, and its own recovery ran `msb remove` for it, so a 0.9.0 journal
   can need the runtime. (`pending_capture` and `group` exist only in development
   builds, but a released import's identity needs it too.)
   `discard_released_import` is the same cleanup for the converted storage: it removes
   the folder only when its marker holds the journaled identity (an empty folder, or
   one with another owner, is left as it is, and a saved sandbox with the name keeps it)
   and the runtime's sandbox only when it is Silo's own with that identity
   (`runtime::cleanup_failed_create`). A released import leaves its sandbox `Created`.
   Unpatched MicroSandbox 0.7.4 refuses to remove such a sandbox (`msb remove` fails
   with "status is Created", with and without `--force`), and starting it to change
   that would run guest code over a partial disk. The bundled runtime's
   `remove-created` patch (see [Removing a sandbox that never started](#removing-a-sandbox-that-never-started-2026-10-01))
   removes it like a `Stopped` or `Crashed` one, without running anything, so the
   sandbox and its disk are removed whole and the name is free again.

   An earlier version of this change reported "Silo did not clean up the data it had
   started" and gave the cleanup up. That result no longer exists: no journal is
   abandoned, and nothing else produced the wording.
3. **An unreadable journal does not hold the migration back.** A
   `backup-operation.json` that was read but cannot be used (damaged or empty, not a
   journal, a field this version does not know), or that another version wrote, can be
   settled by nothing, and used to refuse the migration with the only way out being
   "Continue". `convert_with` now renames it to
   `backup-operation.unreadable-<UTC date>.json` in the app data folder (never reading,
   changing or deleting it; a numeric suffix keeps an earlier one), after its last
   refusal and before the copy, and records a result in its place: "Export or import
   record set aside", "An export or import record couldn't be read and was set aside.
   If an export or import was running before the upgrade, run it again." Whatever the
   unknown operation did is copied like any other content. While a migration is
   unfinished, an unreadable journal is left where it is (exports and imports stay
   unavailable) until `convert_with` sets it aside. At any other launch, `install`
   does the same (`set_aside_at_startup`, which shares `set_aside_unreadable_journal`'s
   rename and notice): a journal that cannot be read or that another version wrote
   used to keep exports and imports unavailable until the user removed the file by
   hand, and is now renamed aside, never read or deleted, with the same result in its
   place ("If an export or import was running, run it again."; the wording does not
   mention the upgrade). If the rename fails the file stays and exports and imports
   stay unavailable as before.

   Only a journal that was read and is unusable is set aside, on both paths. The kind
   is a type, not a message: `recovery::try_load` returns `LoadFailure::Unusable` for
   a file it read but could not use, and `LoadFailure::Io` for any `io::Error` from
   opening or reading it (permission denied, a failing disk, a folder in its place),
   and `journal_state` reports the second as `JournalState::Unavailable`. Nothing is
   known about such a file, so it is never renamed: exports and imports stay
   unavailable, and `convert_with` refuses ("The saved export or import record could
   not be read. Relaunch Silo to try again. No data was changed."), without waiting for
   it. The next launch reads it again. Earlier in this change a folder in place of the
   journal was set aside by the migration; it is now refused until it is removed.
4. **The user is shown what the upgrade recorded.** An export and import result present
   when the window opens belongs to an earlier session and is not toasted, so none of
   the results above would have been seen: the screen about the pre-upgrade backup
   comes first after a migration, and a result recorded just before it counts as old.
   The journal therefore carries `unseen` on a result an upgrade produced: one settled
   before the migration (`finish_settlement` with `before_upgrade`), one whose cleanup
   waited for the upgrade (recorded by the launch after it), and the set-aside notice,
   from either path. It is written only when true (an older build then refuses the
   journal until the result is acknowledged or dismissed; a journal written before the
   field counts as seen), it is removed with the journal when the result is dismissed,
   and a new operation replaces both. `read_backup_state` reports it as `resultUnseen`
   for the result in `operation`, and `acknowledge_backup_result(expectedOperationId)`
   (main window only) marks exactly that result seen, keeping the result until it is
   dismissed. The screen about the backup (`migration-backup-notice.tsx`) reads it
   only when it is shown, lists it beside the backup, and **Open Silo** acknowledges it
   before the application opens, so it is not shown twice; if that fails the result
   stays unseen and the application shows it. Where the screen does not appear (no
   backup left, its notice already shown) or the result is recorded later (recovery of
   an operation that waited for the upgrade finishes after the screen opened), the
   application shows an unseen result as an ordinary export and import notification
   that stays until dismissed, and dismissing it removes it like any result, which is
   how it is acknowledged there: the main window may be hidden at launch, so showing
   the notification acknowledges nothing. No other result is marked unseen, so an
   ordinary result from an earlier session stays silent as before. Fixtures:
   `?view=migration&unseen-result=interrupted-import` and
   `?view=app&unseen-result=set-aside` (`UI-PATTERNS.md`).

A live check on 2026-10-01 launched the development build (`Silo Dev`, `org.silo.dev`,
synthetic GitHub configuration) in an isolated home (`HOME=/tmp/sl`) over previous
generations made with the released 0.9.0's `msb` 0.6.17: a saved sandbox `dev`, the
leftovers of an import that stopped after it saved its identity (the sandbox `copy`
created with `msb create` over `volumes/copy` with its marker), and the pending journal
a released Silo writes. Each run converted the sandbox ("Sandbox 1 of 1 converted and
verified", `complete`) and left all 20 entries of the previous generation byte-identical
(a manifest of every entry, size and SHA-256 before and after). With the sandbox
`Created`, as the released import leaves it, the next launch (before the `remove-created`
patch existed) removed `volumes/copy` from the converted generation, kept the sandbox
record the runtime could not remove, and recorded a "name stays taken" result; that
result no longer exists, and this run was not repeated with the patched runtime. With the record `Stopped` (set in the fixture's
database before the run), the next launch removed the record, its sandbox directory and
the disk from the converted generation, and recorded "Import interrupted. No sandbox
was added. Import the file again." although the record named the previous generation's
disk by absolute path. With a journal of another version, the file was set aside
byte-identical and the result recorded. The dev build's `msb` is the pinned 0.7.4 with
the patches, which cannot boot VMs, so nothing started guest code. The released
import's sandboxes were made with `msb create` over a plain root folder, not restored
from a snapshot. The interrupted capture and the loaded import group of a development
build were not made live: no bootable guest image was available to snapshot, so those
shapes are covered by the unit tests only (a fake runtime and a scripted `msb`). By
MicroSandbox's source, its snapshot index stores each member's absolute path, so the
index of a copied home still names the previous generation's artifacts; Silo's cleanup
selects by group and member, which MicroSandbox resolves under its current home, but
that was not exercised live. A first run, before a sandbox's state was considered,
showed that unpatched `msb remove` refuses a `Created` sandbox: the recovery failed and
kept its journal, which first led to a special case that kept the record (replaced by
the `remove-created` patch). At the time of this check the export and import page
treated every result already present when it opens as stale and did not toast it, and
the pre-upgrade backup screen comes first after a migration, so the user would not have
seen any of these results; item 4 above is the change that shows them. What the user
sees (the screen and the notification) is covered by component tests and fixtures only,
not by a live run. This is not a qualification of the migration or of the packaged app.

Outside the app's reach: an editor's saved SSH `ProxyCommand` runs `msb ssh
serve` with the home it was written for (the migration copied those entries
unchanged until [Editor connections after the migration](#editor-connections-after-the-migration-2026-10-01)).
Re-running the live check after the
change left the old database byte-identical through launch, UI polling and Quit,
and 0.6.17 listed the home without error. That check used a fixture with no real
sandbox, so it is not live migration or packaged-app qualification.

### Pre-upgrade backup (2026-10-01)

After a migration that converted every sandbox, `<app data>/runtime` is a
pre-upgrade backup: nothing reads it, downgrading is not supported, and on a
filesystem without reflinks (ext4) it is a second full copy of every sandbox
disk, the image cache and the database. On APFS the copies are clones, so
deleting frees less than the allocated size Silo shows. Silo keeps it for 14
days and then deletes it; the owner can delete it sooner. The migration screen
reports its size and the date it will be deleted, and Settings, General,
Storage lists it with **Show** and **Delete now** until it is gone.
`src-tauri/src/pre_upgrade_backup.rs` owns this.

- **Only after a complete conversion.** The folder is offered, measured,
  revealed and deleted only while `runtime-migration.json` reports `complete` and
  `runtime-generation.json` selects `runtime-checkpoints-converted`
  (`runtime_migration::previous_generation_is_backup`). After "Continue" the
  generation is `runtime-checkpoints-clean` and the same folder holds the only
  copy of the unconverted sandboxes, so none of this applies. Without a
  generation file nothing was migrated.
- **Window.** The 14 days start when Silo first sees the migration complete,
  which is the first launch after the conversion restarts. An install that
  migrated before this feature has no record, so its window starts at its first
  launch with it. The start is stored in a separate `pre-upgrade-backup.json`
  (`version`, `startedAt` in RFC 3339 UTC, `noticeAcknowledged`), not in
  `runtime-migration.json`: that reader rejects unknown fields, so builds
  without this feature would refuse a changed file. They never read the new
  one. A record that is damaged or has another `version` is kept untouched;
  the backup is still offered and can be deleted by hand, but Silo never
  deletes it automatically.
- **Date bounds.** A saved start whose 14-day deadline or UTC conversion exceeds
  the supported date range follows the damaged-record policy above. The reader
  uses the `time` crate's [checked date addition](https://docs.rs/time/0.3.55/time/struct.OffsetDateTime.html#method.checked_add)
  and [checked offset conversion](https://docs.rs/time/0.3.55/time/struct.OffsetDateTime.html#method.checked_to_offset).
  Regressions cover overflow at `9999-12-31`, UTC overflow from a negative
  offset, and a valid deadline at the end of year 9999. All use temporary data;
  no app or VM is launched. The focused native suite passed all 28 tests on
  2026-10-02, compiled directly with Rust 1.94.0 and the shared cached dependencies
  while Cargo waited for its build lock. Formatting, frontend typechecking, and
  lint also passed; this did not build or inspect an app bundle.
- **Schedule.** Checked at launch and then hourly while Silo runs. A backup
  that came due while Silo was closed is deleted at the next launch. Hourly
  rather than daily because the sleep does not count time the computer spent
  asleep, and a check that finds nothing due reads one small file. The date
  shown is the local calendar date of the deletion instant (14 x 24 hours after
  the start); no countdown is shown. A clock set far forward would delete early;
  a clock set back never does.
- **Deleting.** Exactly `<app data>/runtime`, with `remove_dir_all`, which
  unlinks symlinks instead of following them. A symlink or non-directory at
  that path, or the folder Silo currently reads from, is refused. One deletion
  runs at a time; a concurrent or repeated request waits and then finds nothing
  to delete, which succeeds. A failure part-way (permissions, I/O) keeps the
  rest, the record and the Settings row, and reports the cause so the user can
  retry. Before deleting, Silo runs the image-cache repair and refuses if any
  converted image descriptor still names a file below the backup, because a
  sandbox whose image reads from the backup stops booting once it is gone
  (see `runtime/image_cache.rs`).
- **Runtime-home alias.** The previous generation was reached through an alias
  symlink under `~/.silo` (or `~/.silo-dev`) that dangles once the folder is
  gone. After a successful deletion Silo also removes that link, located with
  `runtime::runtime_home_alias` through `runtime_migration::backup_locations`.
  It goes only if it is a symlink whose link text is exactly
  `<app data>/runtime/microsandbox`; a folder, a file, a link to anywhere else,
  the converted generation's alias and every other entry stay, and the link is
  never followed. It is best effort: a failure is logged and does not fail the
  deletion. The hourly check also removes a dangling alias when the folder was
  already deleted by hand, again only while the migration is complete into the
  converted generation.
- **Size** is allocated bytes (`st_blocks`), not apparent size, so a sparse
  disk image counts what it occupies; symlinks are not followed and a file with
  several names counts once. It is a separate command (`measure_pre_upgrade_backup`)
  that runs off the async workers and only where the size is shown, so reading
  the status at launch never walks the folder.
- **Show** reveals the folder with the same `tauri_plugin_opener::reveal_item_in_dir`
  as exports. The folder holds Linux disk images (`upper.ext4`, `workspace.raw`)
  and a database: copyable, not browsable on macOS.
- **Quarantined files.** The migration moves the previous export-folder setting
  into the folder as `before-checkpoints-backup-history.json`, so it is not replayed
  against the new runtime. After a conversion it moves no export journal: one that
  recorded a result stays so the export page shows it until the user dismisses it, one
  that waits for the upgrade stays for the converted generation's recovery (the backup
  keeps its copy of what the operation left), and one that cannot be read is set aside
  in the app data folder as `backup-operation.unreadable-<UTC date>.json`. Only
  "Continue" moves an unfinished journal into the previous folder (as
  `before-checkpoints-backup-operation.json`), which is not a backup then. Earlier
  builds also moved a finished journal there. Nothing reads the moved files back, and
  the history only remembers an export folder and, in older builds, a list of exports
  that no UI showed. The export files themselves are elsewhere and are not touched.

Existing tools considered: a launchd agent or systemd timer would run while Silo
is closed, but deleting needs Silo's own migration state and image-cache check
in the same process, and Silo already runs its other maintenance (storage
reclaim, log retention) from a monitor thread. The new code is date arithmetic,
a guarded `remove_dir_all` and one small file.

Verification: native unit tests cover the date arithmetic across month, year and
leap boundaries, the clean-generation, symlink and in-use refusals, legacy installs
without a record, past-due deletion at launch, idempotent and concurrent deletion,
symlinks inside the backup, a failed deletion keeping its row, damaged and
later-version records, sparse and hard-linked sizes, and the image-descriptor
check. Frontend tests cover the migration screen, the Settings row, the confirmation,
failure and retry against fixtures, and the native JSON contract. None of this
deleted a real migrated install or booted a converted VM after deleting its
backup.

Migration-state reads and async runtime command admission wait on the progress
mutex on Tauri's blocking pool. The
writer retains that mutex while it commits the progress file and synchronizes the
file and directory; a slow commit therefore cannot stall the main thread through
a progress read. A held-lock regression requires an independent future to run
before the writer releases the mutex and checks that the snapshot stays unchanged.
This follows [Tauri's async command execution](https://v2.tauri.app/develop/calling-rust/#async-commands)
and [Tokio's blocking-work boundary](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

### Editor connections after the migration (2026-10-01)

"Open in editor" writes one SSH entry per computer into `<runtime home>/ssh/*.conf`
and prepends `Include "<runtime home>/ssh/*.conf"` to the user's `~/.ssh/config`.
The runtime home is the alias `~/.silo/<hash of the storage path>`, so the previous
and the converted generation have different ones. The migration copies `ssh/`
verbatim, so each copied entry still named the previous home in its `ProxyCommand`
(`MSB_HOME`), `IdentityFile` and `UserKnownHostsFile`, and the user's file included
only the previous home until the user opened the computer from Silo again. An
editor that reconnects by itself (a restored VS Code window) therefore ran `msb
ssh serve` against the pre-upgrade copy: it started the stale sandbox outside the
migration guard, an upgraded `msb` opened that database and changed it in place,
and once the backup was deleted nothing connected.

`editor::refresh_transports` now repairs this at every launch, on every build,
while `runtime_migration::backup_locations` reports a completed conversion into
the converted generation. It never runs for the clean generation chosen with
"Continue", where the previous folder holds the only copy of the unconverted
sandboxes, or while the migration is unfinished. Running at every launch also
repairs installs that migrated before this change, and it is a few small reads:
a file is written only when its content would change.

- **Entries.** In the selected home's `ssh/`, each `*.conf` Silo wrote (it has a
  `ProxyCommand` line; symlinks, other files and invalid sandbox names are left
  alone) gets its `ProxyCommand`, `IdentityFile` and `UserKnownHostsFile`
  regenerated for the converted home, with the current bundled `msb`
  (`rewrite_configs`, shared with the AppImage refresh, and `with_local_entry`).
  The `Host` name is kept, because an editor saved it. `prepare_configuration` now
  keeps such an earlier `silo-<hash>-<sandbox>` name on the `Host` line when it
  rewrites an entry, so "Open in editor" does not undo this for the saved name.
- **`Include`.** Added with `install_include`, with all its rules (links from
  dotfile managers followed when this account owns them, atomic replacement, the
  exact line reported when the file cannot be changed), but only when the user's
  file still includes the previous home: that line shows they use editor
  connections, and a user who removed it keeps their file as it is. When the line
  cannot be added, the application shell shows a notice with the line and a Copy
  button (`EditorIncludeNotice`, fed by `read_editor_include_notice`). It stays,
  across launches, until the line is in the user's file or the user dismisses it;
  the dismissal saves that line as `editorIncludeNoticeDismissed`, so a different
  needed line shows it again. A toast or system notification would be lost when
  the window is focused, notifications are off or the page has not loaded yet.
  Other failures to add it still notify.
- **The previous `Include` stays.** The new line goes first and `ssh` keeps the
  first value it finds for `ProxyCommand` and `UserKnownHostsFile`, so the
  repointed entries win while the backup exists. A glob that matches nothing is
  harmless once the backup and its alias are deleted, and `IdentityFile` is the
  one cumulative option: a missing second file is skipped. Removing the line would
  mean editing the user's file beyond one prepended line.
- **The previous generation is never written.** Its `ssh/*.conf` stay as copied.

Verification (2026-10-01): unit tests cover repointing real entries written by
`prepare` and copied like the migration does, with `ssh -G` showing the
`MSB_HOME`, identity and known-hosts file before and after; the previous home
unchanged byte for byte; repeating; an `Include` that is present, absent or in a
stow-linked or read-only config; entries that are not Silo's; the clean and
unfinished generations; and the earlier alias surviving "Open in editor". A live
check in an isolated `HOME` built the previous generation as v0.9.0 writes it
with a real sandbox made by the released 0.6.17 `msb`, converted it with the real
`convert_with` and the 0.7.4 `msb`, and then ran an editor-style `ssh`:
before, `ssh -G` resolved `MSB_HOME` to the previous home, the connection
reached the previous copy and changed its database files; after, it resolved to
the converted home, authenticated with the converted home's key and reached the
running converted sandbox, the previous home stayed byte-identical, and it kept
working with the previous home deleted. The sandbox had a hand-written
rootfs, not a Silo guest, so the session itself failed in the guest at the missing
working account. No editor, packaged app or Silo guest was used.

### SFTP working-account identity

The `sftp-user` patch runs nonroot SFTP sessions through the guest's bundled OpenSSH
`/usr/lib/openssh/sftp-server` and the existing identity-aware exec stream.
The pinned upstream's filesystem RPC handler runs as root even when the SSH
session authenticates as `silo`; a real Linux Zed extension upload exposed
root-owned upload directories and a subsequent rename permission failure.
Root sessions retain the original handler. A missing guest helper fails without
falling back to root. The guest image already requires this executable.

[OpenSSH's subsystem manual](https://man.openbsd.org/sftp-server) documents its
stdin/stdout protocol. The [pinned SDK SSH handler](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/sandbox/ssh.rs)
shows the original root-agent SFTP path. Live evidence and qualification limits
are recorded in [the Linux verification session](research/linux-verification-2026-09-30.md).

The same patch sets SSH command `USER` and `LOGNAME` to the effective guest user,
after client environment requests. The offline Linux account regression found
these unset even though UID/GID and HOME were correct.
