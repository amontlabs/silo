# Skipped and ignored test audit, 2026-10-02

Audited `codex/r14-website-docs` after merging `codex/integration` at
`88d6289f`; checked the classifications again after the test fix and integration
merge at `4c8e11c1`. The sync at `b8e6fa62` added the notice race below;
integration fixed it in `cf9a38f8`, present at the later sync `f8004588`.
This is a source and fixture audit, not live qualification.
No production code changed.

The requested patterns (`#[ignore`, `it.skip`, `test.skip`, `describe.skip`,
`it.todo`, `.fails(`) found 25 Rust ignores and no Vitest exclusions or expected
failures at the initial snapshot. The `b8e6fa62` sync added one `it.fails` case,
which `cf9a38f8` subsequently re-enabled. The final inventory has no Vitest exclusions.
The wider audit also checked Node's `{ skip: ... }`, Python's
`skipUnless` and `skipTest`. Rust iterator `.skip()` calls are not test exclusions.

## Re-enabled hermetic test

[`PreinstalledImageDesktop.test_v4_missing_runtime_command_falls_back`](../app/SiloUI/scripts/test_desktop_recipe.py)
previously skipped when the host supplied `xauth` outside the fixture directory.
Commit `ffced694` removes that skip and gives this case a PATH containing only
its fake guest tools and seven required host utilities. The recipe still uses
its real `command -v` branch, and the test still asserts that missing `xauth`
triggers full installation and restores the v4 defaults.

A simulated host-with-`xauth` check first failed because the test skipped; the
same check passed after the fix, with one test executed and no skips. All 20
`test_desktop_recipe.py` cases then passed. The commit was folded into integration.

## Rust ignores

All 25 remain intentional: 20 live checks, two artifact qualification checks,
and three subprocess entry points. The two hermetic entry points run through
ordinary parent tests; removing their ignore would terminate the main test
process or run without the parent's temporary fixture. The lifecycle entry
point belongs to a live parent.

| Test | Disposition and required boundary |
| --- | --- |
| [`rebuilt_cli_killed_load_is_removed_by_next_launch_recovery`](../app/SiloUI/src-tauri/engine/src/backup_controller/recovery.rs) | Keep: real patched CLI artifact qualification (`SILO_E03_MSB`), FIFO input and worker termination. Ordinary scripted recovery fixtures already cover cleanup; replacing the CLI would remove the qualification. |
| [`real_backup_restore_preserves_root_and_computer_without_original_cache`](../app/SiloUI/src-tauri/engine/src/backup_controller.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`real_checkpoint_export_imports_and_cold_boots_checkpoint_time_disk`](../app/SiloUI/src-tauri/engine/src/backup_controller.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_older_checkpoint_export_imports_and_cold_boots`](../app/SiloUI/src-tauri/engine/src/backup_controller.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_built_in_computer_use_sets_up_and_survives_export_and_import`](../app/SiloUI/src-tauri/engine/src/backup_controller.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_built_in_desktop_boots_repeatedly`](../app/SiloUI/src-tauri/engine/src/backup_controller.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_download_of_the_pinned_arm64_package`](../app/SiloUI/src-tauri/engine/src/chatgpt_app/tests.rs) | Keep: external OpenAI download (about 453 MB) and package extraction. |
| [`live_editor_transport`](../app/SiloUI/src-tauri/engine/src/editor.rs) | Keep: explicitly supplied running VM, isolated editor home and SSH/editor integration. |
| [`github_authenticated_browser_workflow`](../app/SiloUI/src-tauri/engine/src/github_live_tests.rs) | Keep: authorized private GitHub repositories, account credentials and mutations. |
| [`github_authenticated_native_workflow`](../app/SiloUI/src-tauri/engine/src/github_live_tests.rs) | Keep: authorized private GitHub repositories, account credentials and mutations. |
| [`live_bundled_image_import_and_cache_reuse`](../app/SiloUI/src-tauri/engine/src/guest_image.rs) | Keep: real packaged CLI, library and staged guest image import/cache qualification. No VM boots here, but the artifacts are not ordinary unit-test inputs. |
| [`inherited_lock_child`](../app/SiloUI/src-tauri/engine/src/host_push_cache.rs) | Keep: subprocess helper selected by its ordinary parent test; it deliberately exits the process. |
| [`live_checkpoint_restore_and_fork_use_the_runtimes_names`](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_built_in_lifecycle_keeps_the_desktop_and_computer_use`](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_pre_v4_computer_gets_no_mount_no_desktop_and_keeps_its_flows`](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`lifecycle_recovery_crash_child`](../app/SiloUI/src-tauri/engine/src/runtime/lifecycle_recovery.rs) | Keep: subprocess helper selected by the opt-in live lifecycle test. |
| [`lifecycle_recovery_survives_real_worker_exit_without_repeating_restart`](../app/SiloUI/src-tauri/engine/src/runtime/lifecycle_recovery.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_reclaim_preserves_capacity_contents_and_reboots`](../app/SiloUI/src-tauri/engine/src/runtime/storage/tests.rs) | Keep: supplied live VM and baseline workspace checksum; reclaims storage and reboots. |
| [`live_start_sets_up_the_silo_account_of_new_and_older_computers`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`github_guest_bootstrap_and_live_identity`](../app/SiloUI/src-tauri/engine/src/runtime_github_tests.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`github_authenticated_guest_workflow`](../app/SiloUI/src-tauri/engine/src/runtime_github_tests.rs) | Keep: authorized private GitHub repositories, account credentials and mutations. |
| [`live_secret_adapter_uses_refs_and_preserves_boot_for_live_updates`](../app/SiloUI/src-tauri/engine/src/secrets_runtime.rs) | Keep: VM boot, assigned secrets and supplied HTTPS endpoints. |
| [`live_approval_switch_edits_only_the_installed_harnesses`](../app/SiloUI/src-tauri/engine/src/test_support/computer_use_live/approval.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`live_lcu_drives_the_desktop_without_a_model`](../app/SiloUI/src-tauri/engine/src/test_support/computer_use_live/lcu.rs) | Keep: real VM/hypervisor qualification; computer-use cases also need a published ChatGPT app and the appropriate guest image. |
| [`crash_child`](../app/SiloUI/src-tauri/vendor/tauri-plugin-updater/src/atomic_install.rs) | Keep: subprocess helper selected by its ordinary parent test; it deliberately exits the process. |

The host-cache parent is
`inherited_lock_survives_parent_exit_until_git_child_finishes`. The updater
parent is `child_exit_leaves_whole_old_or_new_app`; the package's tests run in
[release-platform.yml](../.github/workflows/release-platform.yml). The live
lifecycle parent is
`lifecycle_recovery_survives_real_worker_exit_without_repeating_restart`.

## Conditional Node skips

| File and cases | Disposition |
| --- | --- |
| [Guest image tests](../app/SiloUI/scripts/build-guest-image.test.mjs): `bundled guest tools and unprivileged SFTP work without network` | Keep: explicitly supplied prebuilt Docker image and container runtime. It verifies the actual image, not a mock of its contents. |
| Same file, `offline image check rejects`: missing sudo; missing Python; SFTP executable unavailable to normal users; preinstalled working account; missing dconf user profile; invalid accessibility poller; autostart entry naming a missing program; leftover package file; tampered staged LCU archive; LCU installed in the image | Keep all ten: each mutates a disposable container of that supplied image and checks real guest validation. Default tests already cover command construction and pull/network restrictions. |
| [ZCode TLS test](../app/SiloUI/scripts/zcode-tls.test.mjs): `ZCode TLS: server launch boundary and provider CA selection` | Keep: requires an explicitly supplied installed third-party bundle. The local TLS fixture is isolated, but replacing the bundle would stop testing its actual provider transport and environment sanitizer. |

## Conditional Python skips

| File and cases | Disposition |
| --- | --- |
| [macOS signing](../app/SiloUI/scripts/test_macos_release.py), eight `MacOSReleaseSigningTests`: exact constraint accepted; local build repairs signature; broad exception rejected; additional library rejected; signed replacement engine rejected; Git exception rejected; non-hardened helper rejected; tampered engine rejected | Keep platform guard: uses real macOS `clang` and `codesign` on disposable fixture bundles. The three signer-environment tests remain cross-platform. |
| [APT repository](../app/SiloUI/scripts/test_apt_repository.py), ten `RepositoryTests`: newest version; cached indexes; three-publication downloads; historical signature; previous repository signature; modified package; modified metadata; partial release; mislabeled package; wrong signing key | Keep tool guard: requires real `gpg`, `gpgv`, `dpkg-deb` and `apt-get`. [CI](../.github/workflows/ci.yml) installs the tools and runs these tests. Five retention fixtures remain hermetic and ungated. |
| [Debian installation](../app/SiloUI/scripts/test_debian_installation.py), both `InstallerTests`: in-app upgrade and opt-out/install/upgrade/running process | Keep explicit disposable-root guard: installs packages and writes system APT paths. Linux host support has since ended, so current CI leaves this host-package lifecycle test disabled. |
| [Directory listing](../app/SiloUI/scripts/test_list_directory.py), `test_real_undecodable_name_when_the_filesystem_allows_it` | Keep filesystem guard: some filesystems reject non-UTF-8 names. The adjacent `test_undecodable_names_are_escaped_and_never_openable` already exercises the same encoding rule hermetically through injected directory entries. |

## Bugs and verification limits

At `b8e6fa62`, [`notices.test.ts`](../app/SiloUI/src/desktop/notices.test.ts) contained
`it.fails("bug: disposed notice subscription forwards events before pending registration completes", ...)`,
introduced by integration commit `ae2c0f51`. It hid a real Silo bug:
[`listenForNotices`](../app/SiloUI/src/desktop/notices.ts) set `stopped` on
disposal, but its event callback never checked that flag. A backend event received
before the pending registration resolved still reached the disposed handler.

Temporarily changing only `it.fails` to `it` produced four passes and one failure:
the handler was called once after disposal (`notices.test.ts:78`). The expected
failure annotation was restored. This cannot be re-enabled by correcting the
test without weakening its disposal contract; production changes were outside
this task. Integration then landed `cf9a38f8`, adding the event-delivery guard
and removing `it.fails`. The focused notice suite now passes all five tests.
No confirmed hidden Silo bug remains open in this inventory.

The ZCode TLS test intentionally reproduces a documented third-party bug and
checks the trust-propagation repair; see the
[TLS investigation](SiloUI-ZCODE-TLS-INVESTIGATION.md). Its negative controls
are not expected-failure annotations and should remain assertions.

The passing desktop fixture suite establishes recipe fallback behavior against
fake guest OS tools. Typecheck, lint, Rust formatting and `git diff --check`
passed. Directory-listing tests passed four cases and skipped the real non-UTF-8
filename case on this filesystem. The two Node suites passed eight fixture
cases and skipped their twelve artifact-dependent cases with the opt-in inputs
explicitly empty.

The optional native host-cache parent check waited over five minutes on the
shared Cargo artifact lock. It was cancelled before compilation or execution;
its helper wiring was checked in source, not verified by a native run here.
No live test, real VM, account workflow, package installation or external
download was run. Artifact qualification remains unverified by this audit.
