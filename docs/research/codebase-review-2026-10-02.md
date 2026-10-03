# Silo code review, 2026-10-02

Silo has strong defensive code and substantial regression coverage, but two recovery defects can prevent a user from stopping a sandbox or quitting the app. Fix those before spending time on cosmetic cleanup.

This review identifies **16 findings and 9 improvement opportunities** across runtime recovery, security, local and remote state, computer use, storage, release tooling, tests, performance, and documentation. The most persistent design problem is coupling: advisory history blocks lifecycle control, one remote delays other computers, one damaged checkpoint blocks all scheduled reclamation, and unrelated asynchronous responses invalidate each other. The fixes should isolate ownership and failures at the existing seams, without introducing a new orchestration framework.

## Scope and evidence

Reviewed base: `9cbe7b601b28a6d15169961feeba451ba216b1e6`, version `0.10.0`. Source line references below refer to that checkout. Runtime inputs pin MicroSandbox `0.7.6` at `09df3d4b9d832adaede1fb9a198cfc660bfab8cd` and Rust `1.94.0`.

The working tree was clean at the start. Unrelated guest-image, license-notice, build-script, workflow, and guest-image documentation edits appeared during the review. They were preserved. This review does not qualify those concurrent edits as a completed change.

Four reviewers inspected the current implementation and relevant tests. The [September 29 review](codebase-review-2026-09-29.md) and [remediation ledger](../SiloUI-REVIEW-REMEDIATION-PLAN.md) supplied historical context. Their old line numbers and status labels were not treated as proof of current defects.

| Area | Coverage |
| --- | --- |
| Runtime | Process execution and cancellation, operation gates, lifecycle intent recovery, shutdown, update recovery, migration, storage maintenance |
| Checkpoints and transfers | Capture, restore, pending Start, native ownership/dependencies/deletion, export/import staging, archive checks, capacity checks, controller recovery |
| Security and integrations | Tauri capabilities and CSP, OAuth and personal tokens, host push, retirement, secrets transport/storage, management SSH, guest SSH access, remote dispatch, published ports, viewer authentication, ChatGPT package verification, updater boundaries, channels |
| Frontend | Production source and stores, mutation/read ordering, identity routing, setup and backup tracking, network/SSH, navigation, editor drafts, directories, logs, secrets, desktop/computer-use status |
| Guest and tooling | Computer-use helper, desktop supervisor/status, runtime staging, release publication/verification, Debian update helper and maintainer scripts, APT repository generation, CI, website/demo tests |

This is a targeted architectural and failure-path audit, not a claim that every line received equal attention. No installed app bundle, real VM, remote computer, production HOME, or real credential store was exercised. Static accessibility inspection and role-based fixture tests do not establish screen-reader behavior or visual quality on a real desktop.

Evidence levels:

- **Fixture reproduced:** current production functions, controllers, or an explicitly described extracted seam exhibited the problem with deterministic synthetic data.
- **Source confirmed:** the control flow establishes the issue; its live platform consequence was not exercised.
- **Opportunity:** an evidenced cost or boundary weakness needs measurement or a stronger reproduction before treating it as a production defect.

Priorities: **P1** blocks essential control/recovery under the stated trigger; **P2** affects correctness, security cleanup, or availability; **P3** is a smaller efficiency or documentation defect. Priority is not a claim about how frequently the trigger occurs. Every finding remains open in this report; no application fix was made.

## Verification results

Commands ran from the repository root unless noted. JavaScript checks used Node `24.11.1` through `/Users/polarzero/.nvm/versions/node/v24.11.1/bin`; Python was `3.14.0`; Rust checks used `+1.94.0`. Python results therefore do not substitute for CI's Python 3.12 run.

| Check | Result and interpretation |
| --- | --- |
| `npm --prefix app/SiloUI run typecheck` | Passed |
| `npm --prefix app/SiloUI run lint` | Passed with 12 warnings, described under O-08 |
| `npm --prefix app/SiloUI test -- --maxWorkers=2` | 197 files, **1,777 tests passed**; deterministic supplied data |
| `npm --prefix app/SiloUI run test:release` | **68 passed, 12 skipped**, 80 total; skips do not qualify live/opt-in paths |
| `python3 -m unittest discover -s app/SiloUI/scripts -p 'test_*.py'` | **279 passed, 11 skipped**, 290 total; Linux APT/package-lifecycle tools were not available for the skipped checks |
| `cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check` | Passed |
| `cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked` | With synthetic GitHub configuration: **1,305 passed, 20 ignored** on the rerun with local socket access; three compiler warnings |
| `npm --prefix app/SiloUI run build` | Passed; browser assets only, no native bundle or runtime preparation |
| `npm --prefix website run typecheck` | Passed |
| `npm --prefix website test -- --maxWorkers=2` | 12 Node tests passed; Vitest **4 passed, 1 failed**. See R-14 |
| `npm --prefix demo run typecheck` and `npm --prefix demo test` | Passed; **13 demo tests passed** |
| Extra frontend diagnostic fixtures | **5/5 reproduced** the defective behavior described in R-03 through R-06 and R-15 |

The first native run produced 1,267 passes and 38 failures where the sandbox prevented socket creation. Its exact output was preserved before rerunning. The rerun passed, so those socket failures are an execution-environment limitation, not 38 application defects. A redundant frontend run started while the first run was still pending also passed all 1,777 tests.

The Homebrew Node entry initially failed before executing checks because its referenced `libsimdjson.29.dylib` was absent. Switching to the installed Node 24 executable resolved this local tooling problem; no system package or library was changed.

Local, ignored evidence is under `app/SiloUI/src-tauri/target/verification/code-review-2026-10-02/`, with frontend diagnostic fixtures under `app/SiloUI/src-tauri/target/verification/frontend-audit/`. These paths are machine-local evidence, not committed documentation attachments. The report includes the triggers and observed outputs needed to recreate meaningful regression tests.

The npm advisory request could not complete in the sandbox. Automatic approval review rejected the expanded-network retry because it would send repository dependency metadata to npm's advisory service. No alternative disclosure path was used. **A current dependency-vulnerability scan remains unverified**, not clean. O-09 describes the repository-level coverage gap.

## Prioritized findings

| ID | Priority | Finding | Evidence |
| --- | --- | --- | --- |
| R-01 | P1 | Activity-history failure blocks lifecycle control and Quit | Fixture reproduced |
| R-02 | P1 | Failed full capture can leave a sandbox Paused without ordinary recovery | Fixture reproduced; pinned upstream source |
| R-03 | P2 | An older lifecycle mutation replaces another sandbox's newer state | Fixture reproduced |
| R-04 | P2 | A slow remote blocks fresh local Network and SSH state | Fixture reproduced |
| R-05 | P2 | Cross-computer port saves discard a successful result | Fixture reproduced |
| R-06 | P2 | A same-named remote sandbox's error disables a healthy local port | Fixture reproduced |
| R-07 | P2 | Concurrent token-ledger updates lose revocation records | Fixture reproduced |
| R-08 | P2 | Failed push-token revocation is not independently retried | Source confirmed |
| R-09 | P2 | Remote tunnel readiness accepts an unrelated listener | Fixture reproduced |
| R-10 | P2 | Published-port SSH tunnels outlive controller crashes | Source confirmed |
| R-11 | P2 | Warm boot reports computer use Ready from an obsolete receipt | Fixture reproduced |
| R-12 | P2 | Unresolved initial checkpoint recovery disables every sandbox's scheduled reclamation | Source confirmed |
| R-13 | P2 | APT preserves old indexes without their package/retention contract | File fixture reproduced; live APT not run |
| R-14 | P2 | Website tests fail while CI only typechecks that project | Existing test failed |
| R-15 | P3 | Hidden computer-use views continue native polling | Fixture reproduced |
| R-16 | P3 | Current documentation contradicts implementation and remediation status | Source confirmed |

### R-01. Advisory history becomes a prerequisite for Stop and Quit

**High confidence.** [Activity persistence](../../app/SiloUI/src-tauri/src/runtime_activity.rs), lines 32–43, 58–65 and 130, rejects unreadable or malformed `sandbox-activity.json`. [Lifecycle recovery](../../app/SiloUI/src-tauri/src/runtime/lifecycle_recovery.rs), lines 298 and 238–245, requires these history operations before control work and before retiring its intent. [Shutdown](../../app/SiloUI/src-tauri/src/runtime/shutdown.rs), line 178, uses that lifecycle path.

**Trigger and consequence:** a truncated, incompatible, malformed, or unreadable history file causes Start, Stop, and Restart to fail before the runtime action. Graceful Quit cannot stop running sandboxes through this path. Ordinary history reads already degrade to a warning, so the app can display a sandbox it then cannot stop. An activity write failure after the runtime action can also replace its actual result. After cancellation, failure in `finish` skips intent removal and leaves the abandoned operation recoverable.

The extracted current activity seam, using temporary state, returned:

```text
Malformed activity journal rejects Stop before lifecycle intent or runtime stop:
Malformed("Sandbox activity could not be decoded.")
```

**Fix:** make history persistence advisory. Preserve/quarantine damaged history and report a warning. Retire completed or cancelled lifecycle intents independently of history writes. Continue requiring successful persistence of the actual lifecycle intent before doing work; that journal protects recovery and is a different responsibility.

**Regression:** malformed temp history followed by Stop must invoke the fake runtime and reach Stopped. Inject a history write failure after cancelled Start and assert relaunch cannot resume that Start. Existing valid-newer/oversized-entry tests do not cover malformed JSON. Evidence: `activity-journal-repro.rs` and `.log`.

### R-02. Full capture failure has an incomplete Paused recovery path

**High confidence in the integration defect; no live failure injection.** [Capture cleanup](../../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), lines 660–692, attempts resume only for `RuntimeError::Cancelled`, discards its result, and otherwise records failure/cleans snapshot references without settling the source guest. Lines 2198–2201 release Paused guests during Quit only when a `restore_journal` exists. Ordinary capture has no such journal. [Lifecycle recovery](../../app/SiloUI/src-tauri/src/runtime/lifecycle_recovery.rs), lines 166–174, accepts Paused as neither ordinary Start nor Stop.

**Trigger:** pinned MicroSandbox can deliberately retain suspension after a full snapshot error whose recovery could not resume/rebind or thaw the source. Its [executor](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/runner/control/executor.rs#L485-L500) retains Quiesced for `keep_paused`; its [coordinator](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/checkpoint/coordinator.rs#L950-L969) sets that condition for source recovery failures. Ordinary resume is insufficient for recovery-owned suspension.

**Consequence:** capture fails, but the sandbox remains suspended. Start, Stop, Retry checkpoint, and graceful Quit do not provide an ordinary escape. The fixture supplied Paused after a timeout and observed only `snapshot`; cancellation plus failed resume observed `snapshot, resume`, with Paused remaining in both cases. This does **not** prove that killing a CLI always pauses a real guest.

**Fix:** inspect the source after every failed full capture under cancellation masking. Respect upstream recovery ownership and use the existing Restore resume/force-stop policy where applicable, or persist an explicit actionable recovery state. Preserve disks and diagnostics. Clarify reusable capture-recovery behavior upstream instead of relying on success-path assumptions.

**Regression:** full capture fails into Paused; resume fails; assert a fallback settles to Stopped or an actionable state that Quit can handle. Cover generic failure, timeout with observed Paused, and cancellation with failed resume. Existing successful-cancellation-resume tests cover a narrower case. Evidence: `checkpoint-paused-repro.rs` and `.log`.

### R-03. A lifecycle response can undo another sandbox's displayed completion

**High confidence.** [Production source](../../app/SiloUI/src/desktop/production-source.ts), lines 680–689 and 1094–1108, merges mutation enrichment but replaces runtime rows wholesale. It bypasses the per-row settlement protection used for reads at lines 924–940. [Runtime state construction](../../app/SiloUI/src-tauri/src/runtime.rs), lines 4314–4325, explicitly emits settling rows for other active sandboxes.

**Trigger:** Start A and B concurrently. B's response publishes Running. A's older response then includes B's cached Stopped row with `settling: true`. The reproduction deferred the next refresh and observed B revert to Stopped.

**Consequence:** the UI offers Start for an already running sandbox and hides its live features until another read succeeds. Backend guards still protect mutations; VM data loss was not demonstrated.

**Fix:** merge mutation results using the same per-row observation/settlement rules as reads. Apply the mutation's own target separately and preserve settled rows when an unrelated result supplies placeholders. Add backend observation revisions only if the existing row ownership cannot express the order reliably.

**Regression:** two Starts resolving in reverse order must leave both Running. Repeat with a completed Stop and an unrelated late response. Frontend diagnostic fixture reproduced this scenario.

### R-04. One remote delays fresh Network and SSH results for other computers

**High confidence.** [Production source](../../app/SiloUI/src/desktop/production-source.ts), lines 439–472 and 478–502, waits for all owners before publishing SSH results, and for the remote Network aggregate before publishing the successful local result. [Remote requests](../../app/SiloUI/src-tauri/src/remote.rs), lines 800–821, can wait 600 seconds.

**Trigger:** a connected but unresponsive remote remains pending while local SSH changes from enabled to disabled and a local port moves from 3000 to 4100. The fixture observed both old local values until that remote resolved. Other refresh attempts share the pending request.

**Consequence:** first-load views remain loading; established views retain outdated addresses/settings without marking the healthy local rows stale. This is the remaining Network/SSH scope of earlier H-05, despite the fixed computer-state refresh path.

**Fix:** publish owners independently. Bound each owner's wait and revisions; preserve or mark unavailable only that owner's rows. Reuse `readComputer`'s existing policy.

**Regression:** indefinitely defer one remote and assert fresh local and another remote publish promptly. Its late result must not overwrite newer mutations.

### R-05. A global network revision invalidates unrelated successful saves

**High confidence.** [Network mutation publication](../../app/SiloUI/src/desktop/production-source.ts), lines 521–529, uses a global `networkRevision`. By contrast, SSH saves already track owners separately at lines 1561–1575.

**Trigger:** begin a slow local port save, navigate to a remote sandbox's Ports section, and complete a remote save first. Separate forms permit this even though each form has its own busy state. The remote save invalidates the local response. The fixture received successful local port 4100 but retained port 3000.

**Consequence:** success feedback disagrees with the displayed/copyable endpoint. A discarded result does not itself schedule a confirming read.

**Fix:** track revisions per computer and merge disjoint owners independently. A discarded same-owner response should arrange confirmation when needed.

**Regression:** local/remote overlapping saves in both completion orders retain both results. Same-owner overlapping changes still preserve the newest intent.

### R-06. Presentation names are used as network error identity

**High confidence.** [Network state/controller](../../app/SiloUI/src/features/application/components/network-ports-state.ts), lines 21–24 and 85–95, converts ownership into `name: message`, then matches errors with `startsWith(name)`. [Network page](../../app/SiloUI/src/features/application/pages/network-page.tsx), line 36, supplies this collection; [port actions](../../app/SiloUI/src/features/application/components/network-ports.tsx), line 67, condition Open on the resulting state.

**Trigger:** local `dev` is Reachable; remote `dev` has a network error. The fixture observed the local port become Unknown despite its healthy own row. The cited render condition then hides Open; that UI consequence follows from source rather than a rendered fixture assertion.

**Fix:** retain structured target identity in the error data, or pass the selected row's error directly. Format names only when presenting an alert, including the computer when names collide.

**Regression:** two same-named sandboxes on separate computers with one failing owner must preserve the healthy owner's Reachable state and Open action.

### R-07. Token retirement lacks an atomic ledger transaction

**High confidence; locking seam reproduced.** [GitHub token ledger](../../app/SiloUI/src-tauri/src/github.rs), lines 1128–1135 and 1177–1199, reads a cloned ledger and later replaces it. `SessionSecret`, lines 312–348, locks individual reads and writes, not the entire read-modify-write. The worker holds `OPERATION`, but failed-revocation handling in `HostPushCredential::drop`, lines 1999–2023, does not.

**Trigger:** two failed push-token revocations append concurrently, or a destructor appends while the worker performs network revocations from an older snapshot.

**Consequence:** a new token or another workspace's entry disappears from the cached and durable ledger. Silo loses the record needed to retry revocation; scoped write authority remains until GitHub expires/revokes it. Token exfiltration was not demonstrated. The extracted exact `SessionSecret` fixture reproduced synthetic token A disappearing when B wrote its earlier snapshot.

**Fix:** give the retirement ledger atomic updates. Take a candidate snapshot before network requests, then remove only successfully revoked entries from the current ledger in a locked transaction. Append through that same transaction. Avoid solving this by holding a global network-operation lock in destructors.

**Regression:** pause retirement after reading, append a failed push token, and finish retirement. The new token and unrelated entries must survive. Barrier two simultaneous appends as a second case. Evidence: `token-ledger-race-repro.rs`; no live Keychain or GitHub use.

### R-08. Push-token retirement is coupled to unrelated grant renewal

**High confidence from source; no live GitHub reproduction.** [GitHub](../../app/SiloUI/src-tauri/src/github.rs), lines 1136–1139, remembers a failed retirement by setting `grants_issued`. Lines 1408–1413 call retirement only within access application, while lines 1225–1246 decide whether access is due without considering pending retirement. The independent worker sweep, lines 2637–2647, only handles workspaces absent from policy.

**Trigger:** host push completes but token revocation fails while the sandbox's ordinary GitHub grants remain verified and unexpired. Connectivity later recovers.

**Consequence:** retirement is not promptly retried. It waits for grant renewal or a policy change. If access was disabled and reconciled during the push, the configured workspace can have empty grants and remain successful, leaving no session retry trigger.

**Fix:** schedule retirement independently, with its own retry deadline/wakeup. Reuse the worker; do not introduce a second general scheduler or require VM credential reattachment.

**Regression:** a successful configured workspace with unexpired grants and a pending retired token must retry revocation after the retry clock advances, without applying grants. Repeat with disabled access and empty grants.

### R-09. Remote tunnel readiness does not establish listener ownership

**High confidence for false readiness; hostile timing not exercised against OpenSSH.** [Remote tunnels](../../app/SiloUI/src-tauri/src/remote_network.rs), lines 267–297, reserve a TCP port, release it, spawn SSH, and accept any successful TCP connection while the child lives. Browser opening later uses that address at lines 484–513.

**Trigger:** a competing local process binds during the reservation gap while SSH is connecting/authenticating. The exact extracted `open_tunnel` fixture returned success with a separately bound loopback listener and unrelated `/bin/sleep` child.

**Consequence:** Silo can publish a false endpoint and send service traffic to the wrong local process. A later poll can detect SSH's eventual exit, but the initial readiness assertion is already wrong. No direct Silo credential disclosure was established.

**Fix:** obtain a supported authenticated OpenSSH confirmation of successful forwarding, or keep a Silo-owned listener and forward through a private Unix socket. Research the supported OpenSSH mechanism before adding relay infrastructure.

**Regression:** a competing listener plus an SSH fixture that remains alive and later fails forwarding must never produce Ready. Evidence: `tunnel-readiness-repro.rs`; its known child was cleaned up by `Tunnel::drop`.

### R-10. Crash cleanup depends on destructors that cannot run after a crash

**High confidence from process ownership; no live Silo crash test.** [Remote published-port tunnels](../../app/SiloUI/src-tauri/src/remote_network.rs), lines 272–276, use null stdin/stdout/stderr. Cleanup is `Tunnel::drop`, lines 29–32, or explicit `close_all`, lines 525–528. [SSH construction](../../app/SiloUI/src-tauri/src/remote.rs), lines 617–642, creates long-lived `ssh -N` sessions. [Desktop viewer transport](../../app/SiloUI/src-tauri/src/desktop_viewer.rs), lines 21–92, already has a parent-lifetime mechanism that this path lacks.

**Trigger and consequence:** a controller crashes while a healthy forward is connected. SSH retains the local port and remote connection. Relaunch has no registry entry for that process and cannot close it or reliably reuse a selected port. ProxyCommand descendants also need explicit group ownership.

**Fix:** reuse the desktop tunnel's lifetime pipe/watchdog and owned process-group behavior. Do not rely on a Rust child handle or destructor to enforce parent lifetime. [Rust Child documentation](https://doc.rust-lang.org/std/process/struct.Child.html) and the [OpenSSH manual](https://man.openbsd.org/ssh.1) establish those process/session semantics.

**Regression:** an isolated parent opens a fixture forward; terminate only that verified test parent; assert its listener and owned descendants disappear within a bound.

### R-11. A persistent computer-use receipt is presented as current readiness

**High confidence; warm-boot fixture reproduced.** [Guest computer use](../../app/SiloUI/src-tauri/guest/silo-computer-use.py), lines 549–555, returns early for a matching ready receipt when the LCU path merely exists. Even `boot=True` skips current LCU status, session checks, repair, and doctor. Lines 260–292 project that receipt. [Host projection](../../app/SiloUI/src-tauri/src/computer_use.rs), lines 1129–1135, promotes the result to Ready after approval reconciliation.

**Trigger:** first boot/setup succeeds; a later desktop boot fails, or runtime readiness is lost while the pinned pair/mode stays unchanged. The existing `test_an_up_to_date_vm_runs_nothing` explicitly requires no work on warm boot.

The current helper with the existing temporary guest fixture produced:

```json
{"initialState":"ready","currentDesktopSession":"failed","bootSyncState":"ready","bootSyncReadiness":"ready","commandsAfterBoot":[]}
```

**Consequence:** Ready describes past installation instead of a usable present session. Boot repair logic never runs on this common cached path. The user discovers failure through the agent rather than the displayed state.

**Fix:** keep installation idempotent but revalidate session/readiness after boot. Separate installed/registered state from current readiness, including a boot identity or verification timestamp. A lightweight probe can precede doctor; reinstall is unnecessary.

**Regression:** ready receipt followed by failed new-boot session must run bounded repair/checks or report unavailable/failed, while preserving the skip-install behavior. Also cover a healthy warm boot. Evidence: `computer-use-warm-boot-repro.json`.

### R-12. Checkpoint recovery gates storage maintenance for the whole computer

**High confidence from source.** [Storage monitor](../../app/SiloUI/src-tauri/src/runtime/storage.rs), lines 684–700, runs scheduled reclamation only after global checkpoint recovery returns success. [Recovery](../../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), lines 2784–2804, exits on one invalid record, journal error, or interrupted capture whose data cannot be removed.

**Trigger and consequence:** an unresolved checkpoint during the monitor's initial recovery keeps scheduled reclamation disabled until recovery succeeds. Healthy sandboxes receive no scheduled reclamation during that period. The monitor retries each minute and emits stderr without explaining this dependency in the affected storage UI. Once `recovered` becomes true, a later damaged record does not disable periodic reclamation through this branch.

**Fix:** collect per-sandbox recovery errors, continue independent entries, and schedule healthy guests. Retry failed cleanup separately. Preserve unknown dependencies and snapshot data; permissive deletion is not needed.

**Regression:** one malformed record and one healthy due-for-trim Running sandbox in temp state. One maintenance tick must preserve/report the bad record and trim the healthy sandbox.

### R-13. Preserved APT indexes can reference deleted packages

**High confidence in file publication behavior; live cached-client APT not exercised.** [Package download](../../app/SiloUI/scripts/download-apt-releases.py), lines 28–34, fetches only two releases. [Repository generation](../../app/SiloUI/scripts/apt-repository.py), lines 45–75, emits those packages and declares 14-day metadata validity. Lines 90–119 preserve only the immediately previous InRelease's indexes, not referenced pool packages or the full earlier retention chain.

**Trigger:** publish versions 0.1/0.2, then 0.2/0.3. The preserved old Packages index still advertises 0.1, but its `.deb` is absent. After additional publications within metadata validity, even an older still-valid index hash can disappear because preservation follows only the latest previous manifest.

The file fixture ran the actual preservation function with the GPG verification seam mocked: the prior index survived byte-for-byte, while its advertised package did not exist. This verifies the retention inconsistency, not signature validation or a live client. Existing `test_cached_indexes_survive_republication` checks only the retained index bytes and uses old packages also present in the new site.

**Consequence:** an older candidate or cached/in-progress repository view can encounter missing files despite preserved metadata. Silo's normal update helper performs `apt-get update` first, so this does not establish that every in-app update fails. APT's [repository format](https://wiki.debian.org/DebianRepository/Format) links package Filename records to pool objects; preserving one alone is insufficient.

**Fix:** define retention by publication validity/time and keep the matching immutable pool objects and hash indexes, or use a maintained repository publisher with that contract. Measure the existing 900 MiB Pages budget before choosing retention or another storage destination. Keep candidate selection limited to the desired current releases even if older pool objects remain available.

**Regression:** publish three versions and several metadata refreshes, then use an isolated APT client with cached metadata and explicitly selected older version. It must download bytes successfully while that metadata remains supported. Test expiration/cleanup separately. Evidence: `apt-retention-repro.json`.

### R-14. Website behavior tests are broken and absent from CI

**High confidence; existing suite failed.** [Website test](../../website/src/demo/read-only-demo.test.tsx), lines 40–53, expects overview buttons named `SSH controls for dev/personal`. The current production UI moved these interactions, so `npm --prefix website test -- --maxWorkers=2` fails at line 46 with:

```text
Unable to find an accessible element with the role "button" and name "SSH controls for dev"
```

[CI](../../.github/workflows/ci.yml), lines 52–59, installs/typechecks website and demo but does not execute their test scripts. The 12 website Node tests and other four website Vitest tests passed locally, as did the demo's 13 tests.

**Consequence:** the public demo's behavioral contract can drift while CI remains green. The failure itself does not establish that the new product interaction is wrong; it establishes that the asserted demo behavior and current implementation disagree.

**Fix:** update the demo and test around the intended current SSH interaction, then run `npm test` for website/demo in CI. Keep the demo's guarantee that it cannot perform native operations.

**Regression:** navigate to local and remote sandbox SSH details through the current UI, inspect the controls, and assert all mutating/native actions remain disabled.

### R-15. Hidden computer-use views still poll native and remote state

**High confidence; fixture reproduced.** [Computer-use section](../../app/SiloUI/src/desktop/computer-use-panel.tsx), lines 185–200, has an unconditional interval. [Detail-page call site](../../app/SiloUI/src/features/application/pages/computer-detail-page.tsx), line 481, supplies no activity flag; [application sections](../../app/SiloUI/src/features/application/application-app.tsx), lines 351–365, remain mounted while hidden.

**Trigger:** inspect a desktop sandbox, navigate to Settings/GitHub, or hide the main document. A hidden-section/hidden-document fixture observed the initial request plus three more reads over 15 seconds. [Native desktop state](../../app/SiloUI/src-tauri/src/desktop.rs), lines 325–326 and 365–393, inspects runtime state and executes guest commands. Related app-status/viewer timers need the same ownership check.

**Consequence:** background subprocess work and SSH traffic continue after the main source correctly pauses. At a five-second interval this is up to 720 periodic reads per hour for a mounted panel, before accounting for other timers; an in-flight read suppresses overlapping ticks. CPU and battery cost were not measured.

**Fix:** pass the existing active-page flag and gate periodic reads on document visibility. Refresh once on return. User-requested setup/download work should continue and settle its result while hidden.

**Regression:** hiding the section/document stops timed reads; showing it triggers one fresh read; explicit setup still completes.

### R-16. Current docs carry contradictory status and version claims

**High confidence.** The [remediation ledger](../SiloUI-REVIEW-REMEDIATION-PLAN.md), line 10, still states that nothing has been implemented, while its entries contain landed fixes. The [documentation index](../README.md), line 24, says the current bundled MicroSandbox is 0.7.2, while [runtime inputs](../../app/SiloUI/runtime-inputs.json) pin 0.7.6. The September review's owner-decision introduction also uses the old not-implemented wording without a prominent current-status qualification.

**Consequence:** a contributor can reopen closed defects or reason from the wrong engine's behavior. The remediation source remains useful as history, but its opening paragraph describes the wrong current status.

**Fix:** identify the initial snapshot and link to the evolving ledger/status; remove contradictory present-tense claims. Update the index's current pin reference. Preserve the dated review rather than rewriting its original findings as if they described today's app.

**Verification:** compare these paragraphs with the manifest and ledger. This review adds a new index entry but leaves those historical documents for a separately scoped correction.

## Improvement opportunities

These are separate from confirmed correctness defects. Implement the smallest change that addresses a measured cost or concrete seam, and preserve existing behavior.

### O-01. Bound guest helper output before it reaches memory

[Guest command execution](../../app/SiloUI/src-tauri/guest/silo-computer-use.py), lines 302–317, captures all stdout in memory, then truncates only when writing the log. A timeout bounds duration but does not bound output volume. LCU installation/setup and third-party tools pass through this helper.

Use a bounded spool or incremental drain with a retained diagnostic tail. Test a fixture command producing output above the bound and one timing out. No guest memory exhaustion was observed, so this is boundary hardening rather than a measured leak. Also review descendant cleanup for timed-out installer processes using the same process-ownership policy as other managed work.

### O-02. Stream large build inputs and hash cached files incrementally

[Runtime preparation](../../app/SiloUI/scripts/prepare-microsandbox-runtime.mjs), lines 22–25, 32–35 and 48–51, buffers downloads with `arrayBuffer`. [Guest staging](https://github.com/amontlabs/silo/blob/91c8145fdc37/app/SiloUI/scripts/guest-image.mjs) reads the entire cached image just to hash/check it, and retains complete bytes during staging. [Image lock](../../app/SiloUI/guest-image/image-lock.json) records **414,843,188 bytes for ARM64 and 423,476,714 bytes for AMD64**, about 396/404 MiB, before transient buffers or other build work.

Use maintained Node stream primitives, incremental SHA-256, pinned-size enforcement, temporary files, atomic rename, and a bounded download deadline. Keep the existing digest/size guarantees. Benchmark cold and cached `runtime:prepare` peak RSS and elapsed time; no peak-memory benchmark was run in this review. Refactor the build-input seam only after preserving its offline staging tests.

### O-03. Measure startup before splitting the browser bundle

The app browser build emitted one **1,071.65 kB minified JavaScript chunk**, **310.97 kB gzip**, plus 96.16 kB CSS. Vite warned about the chunk exceeding 500 kB. These are build sizes, not measured launch latency.

Measure main-window, status-panel, and detached-viewer startup on supported minimum hardware. Lazy-load heavy optional routes/viewer code only where it lowers the relevant startup path. Avoid a broad chunking rewrite or hiding the warning as a substitute for measurement.

### O-04. Checkpoint usage reads survey every sandbox

[Checkpoint usage](../../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), lines 2483–2556 and the survey near line 2212, inventories snapshots, loads configured records, and inspects all runtime sandboxes to answer one sandbox's usage/dependency query.

Benchmark a fake runner with many sandboxes and count inspections per selected Storage/Checkpoints read. Return the selected sandbox's byte totals promptly, and cache/share the lineage survey or compute dependency blockers separately if measured latency warrants it. Invalidation must cover captures, forks, restores, deletion, and imports; a generic cache without those ownership rules would add risk.

### O-05. Active log histories still pay full-list costs

[Log history](../../app/SiloUI/src/features/application/model/use-log-history.ts), lines 176–199, accumulates loaded pages; its cache byte limit at lines 82–89 applies to inactive stores. Lines 229–233 reconstruct chronological results on source updates. [Logs page](../../app/SiloUI/src/features/application/pages/logs-page.tsx), line 54, eagerly formats and joins loaded lines for Copy despite virtualized rendering.

Measure an actively paged 50,000-record view while ordinary state refreshes occur. Memoize ordering independently of workspace presentation and generate the clipboard string when Copy is invoked. Preserve full access through paging/export if active-history memory is bounded. This review did not measure a real rendering stall.

### O-06. Keep export preflight reads inside their stated boundary

[Export controller](../../app/SiloUI/src-tauri/src/backup_controller.rs), near line 819, calls `export_source` before acquiring its export gate. [Checkpoint export resolution](../../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), near line 872, can call `ensure_snapshot_group`, which migrates/saves lineage at lines 218–255.

Move lineage migration into the gated worker and make source preflight read-only. A nominal query should not change a legacy record outside serialization. This is a source-identified race boundary; no lost-update/data-loss scenario was reproduced. Add a legacy-lineage fixture with concurrent preflight and checkpoint work before elevating its priority.

### O-07. Pass notification identity explicitly

[Port notifications](../../app/SiloUI/src/features/application/components/network-ports-state.ts), lines 58–65, extract the sandbox from `id.split(":")[1]`. An encoded remote target contains colons, so this becomes `silo-remote`, loses `noticeSandbox`, and misidentifies the operation. [Port form](../../app/SiloUI/src/features/application/components/network-ports.tsx), lines 36–38, also uses encoded `draft.workspace` in progress text.

Pass target, sandbox name, and computer explicitly into the operation helper. IDs identify records; they should not be parsed to construct user-facing copy. Verify remote add/edit/remove notices name the correct sandbox and computer.

### O-08. Consolidate invariants where the observed defects recur

The production source has 1,927 lines; runtime.rs 11,868; GitHub 4,572; backup controller 5,193; checkpoints 6,062, including substantial colocated tests. Size alone is not a defect. The actionable problem is duplicated ownership logic: ordinary reads protect row settlement, mutations do not; SSH saves protect each owner, Network uses one revision; retirement has individually locked reads/writes but no atomic transaction.

Use the reproduced defects to define deep modules for owner-scoped publication, token-ledger mutation, and capture recovery. Keep their invariants private and test behavior through real seams. Do not introduce a generic framework merely to reduce line counts. Separate large test modules when that improves navigation without changing synchronization.

Lint's 12 warnings comprise nine mixed component/helper export warnings, one state update in an effect, one missing dependency warning in the storage panel, and one ref-during-render warning in push feedback. Native tests emit two unused-variable warnings and one platform-specific dead-code warning. Resolve them deliberately; the lint run passing does not mean warning-free code. None was promoted to a functional defect without a reproduction.

### O-09. Add dependency advisory coverage to the existing tooling

[Dependabot](../../.github/dependabot.yml) configures only GitHub Actions. The inspected CI/release workflows run tests and pin-validation but do not run npm/Cargo vulnerability checks. The guest image has a pinned package inventory, but pinning proves repeatability, not absence of newly disclosed vulnerabilities.

Configure maintained dependency-update/advisory tooling for the npm lockfiles and Cargo manifest/lock, including the vendor updater patch and pinned runtime/guest inputs in the review policy. Establish how an upstream security update is qualified across supported targets. Avoid replacing the bundled tools or custom patch blindly; their integration guarantees need regression checks.

No specific vulnerable dependency is asserted here. The current npm advisory scan remains blocked pending authorization to disclose names/versions to that service; a Cargo/guest advisory scan was not completed. Treat this as missing security evidence, not as a clean dependency bill of health.

## Existing protections checked and preserved

Current source addresses several serious findings from the earlier review. Do not reopen them merely because the old report describes them in present tense:

- Host push binds repository/branch/commit to the requested destination and checks write grants; scoped credentials and retirement paths exist. R-07/R-08 concern cleanup correctness after revocation failure.
- Management SSH keys use the restricted bridge policy. Remote action targets use computer/sandbox identity rather than assuming a bare display name.
- Disabled access covers personal-token behavior; removed sandbox grant state is cleaned up. Secret values use the intended reference/stdin boundary instead of host environment injection.
- Viewer authentication, capability separation, navigation guards, signature validation, package size/hash verification, extraction/publication checks, and channel separation have explicit controls. No new concrete bypass was established in those inspected paths.
- Import staging, runtime identity journaling, snapshot ownership/deletion dependency guards, free-space/extraction limits, temporary-boot cleanup, and cancellation handling have substantial tests. R-01/R-02 concern narrower unhandled failure states.
- Lifecycle/destructive confirmations, sandbox detail keys, editor draft persistence, ordinary stale-read guards, host-push backoff, and directory/log paging are implemented. R-03 through R-06 concern paths that bypass or misuse those existing policies.

The broad management-key loopback scope and public-suffix secret-domain wildcards are documented prior owner choices, not new findings. Editor handoff still expands trust as documented. A suspected VS Code folder-setting override was checked against the [upstream credential provider](https://github.com/microsoft/vscode/blob/main/extensions/github/src/credentialProvider.ts): its null-resource configuration lookup does not substantiate that proposed bypass. This does not qualify every editor extension or a hostile guest interaction.

## What remains unverified

- Actual full-checkpoint failure recovery and paused-guest handling in a signed Dev bundle with disposable VMs.
- Crash lifetime of real published-port SSH sessions, authentication delays, ProxyCommand descendants, and two-computer reconnect behavior.
- GitHub revocation races/retries against disposable repositories and App installations; real Keychain/Secret Service outage handling.
- Three-generation APT publication and cached-client downloads on Linux, package lifecycle, and all-target release dry runs.
- Screen readers, keyboard/focus behavior across native windows, minimum-size layouts, motion, and visual affordances on macOS/Linux.
- Peak memory, idle CPU/battery/SSH traffic, large-inventory latency, large-log interaction, and startup on supported minimum hardware.
- Current npm, Cargo, bundled runtime, vendor patch, and guest OS advisory status.

Frontend fixtures prove supplied state behavior; compilation proves compilation. Passing tests do not establish VM health, installed-app behavior, exploitability on a real OS, or release readiness.

## Recommended sequence

1. Fix R-01 and R-02 with failing behavior tests at the lifecycle/capture seams. Then qualify their exact failure/recovery scenarios in Silo Dev using disposable state.
2. Fix R-07/R-08 as one retirement-policy change, and R-09/R-10 as one tunnel ownership/lifetime change. Keep scope within established workers/transports and research supported upstream behavior where needed.
3. Fix R-03 through R-06 using owner-scoped publication and structured identity. Add the reproduced reversed-completion and same-name scenarios to the ordinary frontend suite.
4. Fix R-11/R-12/R-15, then repair/enforce website tests and correct current documentation. Qualify APT retention before the next repository publication change.
5. Measure O-01 through O-05; implement only changes that improve a stated memory, latency, or work budget. Complete dependency advisory checks once authorized.

**Next action:** add the failing Stop-with-malformed-history regression for R-01, then make advisory history unable to block lifecycle intent execution or retirement.
