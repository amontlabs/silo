# Test dependencies, export and import in the real app

The native app at `app/SiloUI` uses real computer state and operations. It has no fixture launch flags, fake operation
results or settings-directory overrides.

## Build and launch

From the repository root:

```sh
npm --prefix app/SiloUI test
npm --prefix app/SiloUI run typecheck
npm --prefix app/SiloUI run lint
cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked
RUSTUP_TOOLCHAIN=1.94.0 npm --prefix app/SiloUI run desktop:build:debug
codesign --verify --deep --strict \
  'app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Dev.app'
open 'app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Dev.app'
```

Before launching, inspect any running instance and verify its executable path
and ownership. Use the exact rebuilt Dev bundle above; its executable is
`Silo Dev.app/Contents/MacOS/silo-ui` and its identifier is `org.silo.dev`.
Do not interrupt the user's app or computers. Quitting a test-owned Dev instance
stops its local computers, so use only disposable state for this walkthrough.

The first build downloads pinned build inputs and compiles the patched runtime.
It requires Rust 1.94.0. Users of the finished app need none of those build tools.
The debug build already uses the development channel, with separate app data,
credentials, runtime aliases and remote bridge names. See
[build channels](SiloUI-BUILD-CHANNELS.md). Use deterministic frontend fixtures
for UI-only checks and disposable Dev computers for native checks. Historical bundle
paths in the verification records below identify those earlier runs.

## Dependencies

On a fresh app identity, open onboarding's Dependencies step:

1. System and Bundled tools show checking states. Continue stays disabled.
2. Expand both groups. A supported Apple-silicon Mac shows macOS/hypervisor and
   packaged MicroSandbox, Git and Git LFS results. Normal captions use one line;
   error details may wrap.
3. Continue enables only when every required check succeeds. Keyboard Space and
   Enter operate the existing disclosures; focus and light/dark appearance stay
   unchanged.
4. A failed check shows its concrete error and Retry checks. Retrying resets the
   results while new read-only probes run. No install or repair happens here.

macOS checks the declared macOS 14 floor, arm64 target and `kern.hv_support`.
Linux checks a supported GNU/Linux target, glibc 2.34 or later, access to
`/dev/kvm` and KVM API version 12. It never creates a computer to probe support.
Both validate bundled files and bounded version commands. Failed, unsupported,
unreadable and timed-out results cannot pass.

References: [Apple Hypervisor](https://developer.apple.com/documentation/hypervisor),
[KVM API](https://docs.kernel.org/virt/kvm/api.html#kvm-get-api-version),
[Tauri sidecars](https://v2.tauri.app/develop/sidecar/).

## Selected export and import paths

The native pickers return paths through a string-based frontend contract. A
selection must round-trip without changing its filename: spaces, Unicode and
leading dashes are preserved. Non-UTF-8 names fail before archive inspection or
remembering an export folder. Rename the affected file or folder and select it
again. Rust's [`Path::to_str`](https://doc.rust-lang.org/std/path/struct.Path.html#method.to_str)
reports this boundary; `to_string_lossy` replaces invalid bytes and can name a
different file. Native path-conversion regressions exercise synthetic Unix
paths, not a live desktop picker.

## Computer configuration

Use disposable computers for this walkthrough.

1. Configure a computer with distinct runtime and workspace storage sizes. Save it.
   Creation produces a stopped computer with separate root and `/workspace` disks.
2. Start it, then stop it. The displayed state must follow the runtime result.
   A failure must remain visible; it must not become Stopped or Running by default.
3. During onboarding, set a Git name/email and finish. These are saved to the
   computer's environment and verified before completion. Application preferences
   persist after quitting and reopening Silo.
4. CPU/RAM limits come from the device. Unknown measurements fail explicitly.
   Startup memory pressure uses the approved advisory; it is not an invented
   universal minimum. Disk failures report the real operation error.

GitHub authentication and host Push are implemented separately; see
[GitHub implementation](SiloUI-GITHUB-IMPLEMENTATION.md). Repository selection
controls access, and tools inside the computer clone repositories through that access.
This walkthrough's storage checks do not verify GitHub authorization or pushing.

## Export a computer

The Backup page was removed. Each local computer is exported on its own, and
progress and results appear as a background notification.

1. Open a disposable local computer's menu (in the computer list or on its page)
   and choose **Export…**, then choose a destination folder through the native
   folder picker. Remote computers offer no Export.
2. Export starts as soon as the folder is chosen. A running computer keeps
   running. The notification shows the capture and verification phases.
3. Success, **Exported**, must name a real `.silo-backup` export file in the
   chosen folder, with its size. **Show in Finder** (**Show in folder** on Linux)
   reveals it.
4. Start another export and choose **Cancel** in the notification while it is
   capturing, then confirm **Stop**. The incomplete file is removed; earlier
   export files stay intact.
5. An unavailable or full destination must show an error with **Retry**, not
   success. Unknown required space is omitted rather than estimated.

## Export a checkpoint

1. Open the computer's page, then its **Checkpoints** tab. Create a checkpoint if
   none exists.
2. Choose **Export…** from a checkpoint's menu and pick a folder. The
   notification reads **Exporting checkpoint “name”**, then **Checkpoint
   exported**. The export file contains that checkpoint's disks, not the current
   state and not the rest of the checkpoint history.

## Import an export file

1. In the computer list choose **Add → Import computer…** (or **File → Import
   Computer…**) and select the export file through the native picker.
2. Silo checks the file before asking anything. An invalid or damaged file shows
   **This export cannot be imported** with its reason and creates nothing.
3. The review shows the file's size and computer. The suggested name is
   `<source>-imported`; a name you type is retained. An existing name must be
   rejected without changing that computer.
4. Choose **Import**. The notification ends with **Imported name**, "Stopped and
   verified.", and **Open**. The result is a new stopped computer. Start it and
   confirm both root and `/workspace` files survived. Configured storage sizes
   and Git identity must also survive. Running programs, memory and checkpoint
   history are not imported.
5. The source computer and earlier export files remain intact. A failed or
   cancelled import removes only its newly created computer and disk, never
   another computer with the same name.

## Automated live regression

The ignored Rust test
`real_backup_restore_preserves_root_and_computer_without_original_cache` uses
production create, identity, export and import functions with disposable paths.
`real_checkpoint_export_imports_and_cold_boots_checkpoint_time_disk` covers
checkpoint export the same way.
It requires prepared guest-image resources, the built app and a working
hypervisor. Configure native tests through the [release guide's local setup](SiloUI-RELEASES.md#local-setup).
Use the bundled Dev runtime,
which Tauri signs with the Hypervisor entitlement; the raw build-cache executable
cannot boot a computer on macOS:

```sh
SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures \
SILO_TEST_MSB="$PWD/app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Dev.app/Contents/MacOS/msb" \
SILO_TEST_LIBKRUNFW="$PWD/app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Dev.app/Contents/Frameworks/libkrunfw.5.dylib" \
cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked \
  real_backup_restore_preserves_root_and_computer_without_original_cache \
  -- --ignored --nocapture
```

These variables exist only in the compiled test harness, not the application.
The test writes distinct root/workspace markers, exports, and deletes the
isolated original runtime/cache/volumes. It imports into different runtime
homes, first empty and then already containing an independently created computer.
It verifies that import saves a pending computer without creating or starting a
runtime computer. It then uses the app's explicit Start path, verifies files and identity,
and checks that the existing computer and its cached disks remain intact. It cleans up its own computers. It never uses existing user computer data.

## Coverage limits

The original checks recorded below used macOS. Unit tests cover failure mapping,
archive traversal/integrity, cancellation, name collisions and resource handling.
Later Linux runs are recorded in [Linux verification](SiloUI-LINUX-VERIFICATION.md)
and [Linux acceptance](research/silo-linux-acceptance-2026-09-25.md), with exact
packages, runtime versions and device limits. Those dated runs do not qualify the
current HEAD or every platform; verify the intended package and workflow before release.
An ad-hoc debug signature is not notarization or release-signing verification.

## Interrupted import cleanup limit

Silo saves the new import's native checkpoint group before invoking `snapshot load`.
Recovery removes indexed members of that group, including an import interrupted
before computer settings were saved. A failed cleanup keeps that ownership journal
for another launch. Other groups and completed imports remain intact.

The current [import-stage patch](../app/SiloUI/patches/microsandbox-import-stage-id-0.7.6.patch)
adds `snapshot load --stage-id`. Silo journals `silo-import-<id>` before load
starts, then passes its suffix as the stage ID. The loader uses
`snapshots/.msb-snapshot-load-<id>` and `cache/tmp/snapshot-load-<id>` under the
selected runtime home. [Launch recovery](../app/SiloUI/src-tauri/engine/src/backup_controller/recovery.rs)
removes only that journaled group's indexed members and its two stage roots,
including after a crash before computer identity allocation. Cleanup respects the
existing worker lock, rejects symlinked stage paths, and retains the journal on failure.

The older [MicroSandbox 0.7.2 archive loader](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/backend/local/snapshot/archive/batch.rs#L263)
used random `.msb-snapshot-import-*` and `snapshot-import-*` directories without
operation ownership. Recovery still preserves those unattributed legacy stages
and other operations' stages. It does not use an age or prefix sweep. See the
[import cleanup design and evidence](SiloUI-REVIEW-DESIGN-NOTES.md#e-03-checkpoint-deletion-with-fork-dependencies-and-native-snapshot-cleanup)
for the recorded patch qualification and its limits.

## Recorded evidence, 2026-09-08

This record predates the per-computer Export and Import flow: it describes the
removed Backup page, whose backups and restores are now exports and imports.

The production `Silo.app` built successfully and passed
`codesign --verify --deep --strict`. In its native window, accessibility checks
observed `macOS 26.5 · Apple silicon`, `Apple Hypervisor available`,
`Bundled msb 0.6.17 · libkrunfw 5.6.1`, `Bundled Git 2.53.0`, and
`Bundled Git LFS 3.7.1`; Continue was enabled. Both disclosures expanded and their
normal captions remained one line. The window screenshot is generated evidence
at `app/SiloUI/src-tauri/target/ui-evidence/dependencies.png`.
The app was closed through **Silo > Quit Silo**; a subsequent process query found
no `silo-ui` or `silo-preview` process. No test mode was used.

The frontend suite passed 412 tests in 46 files. The production web build,
typecheck and lint passed. The native suite passed 92 tests with the real-VM test run separately; Rust
format checks passed. Live VM evidence is recorded after the end-to-end run
below.

A separate generated copy had its real runtime manifest removed and was signed
again. It still opened Dependencies, reported the exact missing file, marked
MicroSandbox unsuccessful and disabled Continue. Retry repeated the real check
and remained failed. Screenshot: `src-tauri/target/ui-evidence/dependencies-missing-runtime.png`.
No production files or ordinary app settings were changed for that failure test.

The final signed runtime passed the expanded portable regression in 41.52 seconds,
without a preparatory image pull. Both creations remained stopped. After backup,
the test removed both originals and their entire cache, restored with image
pulling disabled into a different empty home, then repeated restore in a third
home containing an independently created VM. Restored root/workspace files,
Git identity and separate configured filesystems survived. Existing cached VMDKs
remained byte-identical and the existing VM still booted. All disposable test VMs
were cleaned up. A real two-VM archive is retained as ignored evidence at
`src-tauri/target/ui-evidence/verified-root-workspace.silo-backup`.

The native archive picker opened the real two-VM archive. `Sandbox to restore`
listed both sources; selecting `silo-proof-second` changed the suggested name to
`silo-proof-second-restored`. The compact production selector is captured at
`src-tauri/target/ui-evidence/restore-selector.jpg`. The native destination picker
also returned the selected folder and enabled Review backup. These used the same
production code in a separate `org.silo.verification` application identity, with
real disposable VM data and no fixture mode.

The final desktop walkthrough restored `silo-proof-backup-restored` alongside an
existing real VM. The result card showed **Restore complete**. Sandboxes showed
**Stopped**, then **Running** after Start, then **Stopped** after Stop. Creating
a backup of that restored VM showed **Backup complete** and produced
`Silo-Backup-1788886343.silo-backup`. After Quit and relaunch, Recent backups still
listed that archive with its source and destination. Screenshots:
`src-tauri/target/ui-evidence/restore-complete.jpg`, `backup-complete.jpg`, and
`backup-history.jpg`. The verification app was closed through its Quit menu.

The walkthrough also caught and fixed two desktop boundaries: native dialogs
must run off the main thread, and Rust enum fields must serialize using the same
names expected by TypeScript. Shared native/frontend contract tests cover idle,
running and completed backup payloads. Failed state reads retain an explicit
operation error rather than hiding the outcome.

## Continue and the setup queue

1. Reopen a saved draft on Computers. Before submission, the footer says
   **Not started**, and the draft determines the operation count. Merely opening
   the app does not create its computers.
2. Click Continue. The exact displayed configuration enters the queue and starts.
   Returning and clicking Continue again must not duplicate an identical job.
3. During creation, the Computers panel shows the actual computer and operation, an
   elapsed timer and native activity. Navigation remains responsive.
4. Continue from GitHub submits the selected identities after computer creation.
   Review shows status on each computer and the Git author card, with unsubmitted
   work labelled **Not started**. There is no separate Setup operations list.
   Finish persists completion only after prerequisites succeed.
5. A failed request keeps its error and Retry. The failed computer must not retain an
   In progress badge. Retry uses the latest submitted draft. Quit waits for
   submitted native work before completing the settings shutdown handshake.

To check optional GitHub setup, leave GitHub disconnected and continue to Review.
Saved repository choices remain in the draft but are excluded from submission
until GitHub is connected. Git author choices still apply. Click Finish: setup
must complete without a repository-setup error. With GitHub connected, setup
saves the selected/all repository policy and waits for each computer to acknowledge
that access before marking completion. A failed or replaced policy keeps setup
incomplete. The [production setup adapter](../app/SiloUI/src/desktop/production-source.ts)
and its [fixture tests](../app/SiloUI/src/desktop/production-setup.test.ts) cover this
acknowledgement; they do not prove live GitHub access.

On Review, each verified computer must show **Complete**, matching its Computers
row. Git author shows **Complete** only after saving and verification succeed;
pending and failed work retain their actual status. Errors may wrap, while
normal card captions remain one line. Use Edit to return to each source step
and confirm the same status after returning to Review.

Native verification of the inline Review status used the rebuilt production
`src-tauri/target/debug/bundle/macos/Silo.app`. Review showed dev, playgrounds,
and personal as Complete; GitHub access was not connected and Git author was
Not started. Clicking Finish applied the saved identity and opened the main
Sandboxes screen without error; all three VMs remained Stopped. The app was
left open. Screenshots are in the ignored `src-tauri/target/ui-evidence/`
directory: `review-inline-validation.jpg` and `review-finish-disconnected.jpg`.

## Live activity

1. Open onboarding's Computers step and expand Live activity. After restarting
   Silo, it displays the latest recorded attempt; it does not rerun setup.
2. Continue with existing computers. Return to Computers: activity must include
   timestamped verification for each computer and an explicit completed outcome.
3. When adding a computer, activity must identify disk preparation, image resolution,
   download, image checks/preparation, configuration saving and verification.
   Downloaded bytes are real; a total is shown only when every layer size is
   known. Cached images need not produce a download. A quiet create reports its
   last stage and elapsed time every five seconds.
4. An actual failure must end with an error, affected computer and recovery guidance.
   Warnings represent nonfatal conditions. Copy activity must copy the same safe
   text shown on screen, without raw registry URLs, credentials or device paths.
5. Quit and relaunch. The recorded outcome and diagnostics remain available.
   A recorded unfinished attempt is labelled interrupted, never completed.
   A history read/write failure is explicit and does not invent history.

Do not interrupt a user's computer operation merely to produce a screenshot. Native
tests cover interrupted/corrupt histories, missing storage, safe failure
categories, streamed output and exit-event delivery. Tests use temporary storage;
there is no production fixture switch or debug activity feed.

Verified on macOS with the rebuilt production bundle: Continue verified all
three existing VMs, activity showed timestamped start/verification/completion,
and the same attempt reappeared after a normal quit/relaunch. Onboarding remains
open with Live activity expanded. Screenshots: `activity-completed.jpg` and
`activity-retained.jpg` under the ignored `src-tauri/target/ui-evidence/` folder.
No user VM was started. A separate disposable runtime check downloaded a fresh
BusyBox image and created a stopped sandbox successfully, reporting 266,867 then
1,900,727 bytes against a 1,900,727-byte total; temporary storage was removed.

Validation: the full frontend suite passed 442 tests before two final boundary
regressions were added; the final affected bridge/panel suite passed all 30
tests. All 37 native runtime tests passed, including the local socket test.
Three tests extracted verbatim from the patched upstream encoder passed.
Typecheck, lint, desktop bundling and signature verification passed. Linux
native execution was not tested on this macOS host.

## Device Git and jj identity

### Automatic startup and notifications

Use the existing completion screen or Settings controls; there is no test mode
or extra production control for these features.

1. With onboarding complete, enable Start computers at launch and select a local
   computer. Quit Silo normally and reopen it. Only selected local computers should start;
   already-running computers must not restart. Opening/closing the status panel,
   focusing the main window or refreshing state must not start them again.
2. Disable automatic startup and relaunch. Stopped computers must stay stopped.
   Incomplete onboarding must never trigger automatic startup. Missing or remote
   selections must not create a replacement computer or be reported as started.
3. Enable notifications and the desired categories through the existing UI.
   When the OS already grants permission, genuine state changes and failures
   should produce the corresponding notifications. Opening Silo must not announce
   every computer's initial state; repeated reads must not duplicate an alert.
4. Disable all notifications or an individual category and repeat the relevant
   event. That category must remain silent. Denied OS permission must not trigger
   an automatic permission prompt. Use the existing Enable notifications control
   to request permission deliberately.
5. Failure tests use isolated runtime and delivery seams for startup, actions,
   health reads and backup results. Do not corrupt user data to manufacture an
   alert. Notification text must not include raw command output, credentials or
   archive paths. A notification delivery failure must not undo a successful
   computer operation or backup.

Native verification on 2026-09-09 used the normal production debug bundle,
`src-tauri/target/debug/bundle/macos/Silo.app`, with saved startup selections dev
and playgrounds. The first attempt exposed a Created/Stopped mapping bug; a
failing regression and fix added support for newly prepared Created VMs. The
rebuilt app automatically started dev and playgrounds; personal stayed Stopped.
The existing Stop dev and Stop playgrounds controls then returned both to Stopped.
The final app remains open and startup preferences remain unchanged.

Notification policy, initial-state suppression, health transitions and OS
authorization were tested natively; the real startup failure exercised the
action-failure hook. Visible OS notification delivery was not verified: automatic
approval review rejected Notification Center inspection because it could expose
unrelated private notifications. Linux delivery was not exercised on a Linux
desktop. Native tests passed, including a Unix-socket test rerun outside the
restricted sandbox, plus the final four Created/Stopped startup regressions.
TypeScript build, lint, native bundle build and strict signature verification
passed. The build log is `/tmp/silo-startup-notifications-build.log`.

### Resume, retry and completion

1. Apply the Git author on GitHub, then open Review and confirm **Complete**.
   Quit and reopen Silo before finishing onboarding. The same author must regain
   **Complete** after a read-only check of the saved computer configuration. Opening
   onboarding must not create a computer or rewrite its identity.
2. Change an author without submitting it. Review must not claim that the new
   value is complete. Continue applies and verifies that value normally.
3. Regression tests cover failed computer submissions, failed identity submissions
   and failed final settings saves. **Retry** must repeat the failed submission,
   not replace it with computer configuration. Do not damage a real installation
   or change device permissions to manufacture these failures manually.
4. Leave GitHub disconnected and click **Finish**. After the settings save
   succeeds, the existing **Setup complete** screen must remain visible with
   login and notification options. **Open Silo** enters the main app.
5. Quit and reopen after successful completion. Silo must open the main app
   directly. Failed completion must leave onboarding open with its error.

Dependency checks retain their existing Retry checks and reinstall guidance;
this flow does not download replacement app components.

### Dependency recovery

Missing or damaged bundled files explain how to reinstall from the original
download or package manager while keeping app data. Device failures give the
specific OS, architecture or Linux KVM requirement. Timeouts and bridge failures
offer Retry checks, then reopening Silo, without claiming reinstall is needed.
The main app exposes the same checks through its existing System issue or load
error display. Retry checks becomes disabled Checking… while running, retains
the error instructions and removes the issue only when checks pass. There is no
Repair action, simulated repair progress or simulated success banner.

Native verification on 2026-09-09 used a temporary copy of the production debug
bundle with only its bundled msb removed. The load error displayed missing-file
and reinstall guidance plus Retry checks. Restoring that file in the copy and
clicking Retry checks returned to the real sandbox list without restarting the
app; all three VMs stayed Stopped. The normal bundle was not damaged and no VM
configuration changed. The temporary app was quit and removed. Regression tests
cover retained guidance during checking and clearing it after success. Eight
native dependency tests cover platform/error mapping; Linux was not tested live.

Native verification on 2026-09-09 used the rebuilt production debug bundle at
`src-tauri/target/debug/bundle/macos/Silo.app`. Before the fix, quitting and
reopening changed Git author from Complete to Not started. After the fix,
reopening restored Complete without submission. Finish with GitHub disconnected
showed Setup complete, the login/notification controls and Open Silo. Open Silo
entered Sandboxes; quitting and reopening entered Sandboxes directly. All three
VMs remained Stopped. The final instance was left open. Retry failure paths were
covered by component/source regression tests, not manufactured against live data.
The 461-test frontend suite, focused native identity regression, TypeScript,
lint, debug bundle build and strict signature verification passed. Build/test
logs for this run are `/tmp/silo-onboarding-fixes-build.log` and
`/tmp/silo-onboarding-fixes-tests.log`.

Open GitHub in onboarding: untouched author fields should use the configured
device Git name/email, or a complete jj pair if Git has none. Without either,
fields remain empty and manual entry remains available. Edit one computer's
identity and navigate away/back: it must retain that edit. Reset uses the
detected device pair. Continue applies/verifies Git and jj identity variables in
stopped computers; the device configuration is read-only.

Native verification replaced the old example draft with the detected Git author
and applied it successfully to all three existing stopped sandboxes. Review
showed Git author Complete; GitHub remained disconnected. No VM was started and
onboarding remains open. Eight native identity tests, 68 focused frontend tests,
and the final 20 bridge/recovery tests passed; typecheck, lint, bundle build and
signature verification passed. jj fallback uses isolated regression tests;
the real-app walkthrough used Git.

The isolated native walkthrough observed actual creation progress, responsive
Continue, identity work waiting behind creation, and disabled Finish. The live
image download timed out; Retry then returned a Docker registry connection
error. Both outcomes were displayed without success. Queue ordering, duplicate
submission, retry, stale-result handling and shutdown use deterministic deferred
bridge regressions; no fixture launch mode was added to the application.

Final queue verification: 425 frontend tests in 49 files passed with
`npm --prefix app/SiloUI test -- --maxWorkers=1 --testTimeout=30000`;
typecheck and lint passed. Parallel runs hit the existing five-second UI-test
deadlines, so the final run used one worker and a larger runner deadline without
changing assertions or test configuration. The 28 focused native runtime tests
and 15 settings lifecycle tests passed. The production desktop bundle built and
passed signature verification.

After reopening the rebuilt production app, the existing three-VM draft showed
**Not started · Continue to start this step**, **Continue to create sandboxes**,
and **0 of 6 operations complete**. The user's draft was not submitted during
verification. Screenshot: `src-tauri/target/ui-evidence/setup-draft-idle.jpg`.

## Export folder picker execution

The export picker reads its saved folder inside its existing blocking worker.
Backup-state reads hold the view mutex while reading the recovery journal, so the
picker can wait for filesystem I/O even though the destination itself is cached.
The blocking worker covers that lock wait as well as the native dialog and path
validation, following [Tauri's async command execution](https://v2.tauri.app/develop/calling-rust/#async-commands)
and [Tokio's blocking-work boundary](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).
The destination persistence and picker path regressions cover the retained data
behavior; they do not exercise a live native dialog.
