# Silo comprehensive code review, 2026-10-02

Fix recovery and data preservation before cosmetic cleanup. Damaged activity history can block Stop and Quit; a failed full checkpoint can strand a paused VM; an account migration can overwrite workspace files through an existing symlink; and installing an update can discard settings the frontend never delivered.

This report consolidates **26 actionable findings: 2 P1, 20 P2, and 4 P3**, plus **11 improvement opportunities** across reliability, security, performance, maintainability, usability, testing, and delivery. Four findings are additional to the existing same-day current-checkout review: R-23 through R-26. The repeated defects come from ownership and failure handling: unrelated computers share publication ordering, advisory state blocks essential controls, persistence errors do not consistently prevent success, and cleanup lacks an independent transaction or lifetime.

## Scope, revision, and confidence

Reviewed revision: `f9c12601723f25b7e4a5a3bab01b90058eec14aa`, Silo `0.10.0`. Runtime inputs pin MicroSandbox `0.7.6`, revision `09df3d4b9d832adaede1fb9a198cfc660bfab8cd`, and Rust `1.94.0`. This report describes that source snapshot, not later changes.

Follow-up: the [second review pass](SiloUI-CODE-REVIEW-PASS-2-2026-10-02.md) checks the later `a1197ae` cutoff, adds R-27 through R-37, and verifies the R-21 fix. It separately records a native compilation failure on intermediate revision `ac83d94`. The findings and verification below remain historical evidence for `f9c1260`.

The working tree already contained a modified documentation index and two untracked reports: [the initial review](research/codebase-review-2026-10-02.md) and [the current-checkout review](research/codebase-review-current-2026-10-02.md). They were preserved. This document consolidates their detailed findings, independently checks the relevant current source, records this pass's verification separately, and adds new findings. It does not count inherited fixture results as fresh executions. No application fixes, version changes, release notes, commits, or publication were performed.

Four reviewers divided the work across frontend interactions, Rust runtime/security, delivery/guest scripts, and integration review. This is a broad risk-based audit, not a claim that every line received equal scrutiny.

| Area | Reviewed boundaries |
| --- | --- |
| Runtime and data | Lifecycle/cancellation journals, operation admission, shutdown, migration, checkpoint failure recovery, backup/import validation, storage maintenance |
| Security | Credential cache and retirement transactions, host push, SSH identity/lifetime, remote operation framing, desktop proxy/capabilities, package/archive verification, channel separation |
| Frontend | State ordering, local/remote identity, Network/SSH, settings and update persistence, migration recovery, computer-use approval/readiness, directories/logs, navigation and controls |
| Guest and delivery | Working-account migration, computer-use helper, runtime/LFS staging, release publication, APT retention, CI and website/demo tests |
| Optimization and clarity | Hidden polling, archive memory use, bundle size, log computation, checkpoint surveys, misleading readiness/error/retry affordances, documentation drift |

**P1** means essential control or recovery is blocked under the stated trigger. **P2** means correctness, data preservation, credential policy, or availability is impaired. **P3** means a narrower defect or one with a practical workaround. Priority reflects consequence, not measured frequency. No P0 compromise, VM escape, or credential exfiltration was demonstrated.

**Evidence terminology:** a production-function fixture runs real code with synthetic collaborators; an extracted seam runs specified production logic outside its full integration; source confirmation establishes the control flow without exercising its live platform consequence. Diagnostic fixtures assert existing bad behavior: their passing result proves the reproduction, not a fix. Improvement opportunities need further measurement or qualification and are not additional demonstrated incidents.

## Verification performed in this pass

App/release/website/demo checks used Node `24.11.1` through the installed nvm executable. The native Rust suite used `+1.94.0` and explicit synthetic GitHub configuration from the release guide. The standalone credential seam used Rust `1.91.1`; the timeout seam used `1.94.0`; neither needed GitHub configuration. Python was `3.14.0`; these results do not replace the supported guest/CI Python environment. No installed bundle was launched or inspected. No real VM, remote computer, production HOME, production app, or credential store was used.

| Command, from repository root | Result |
| --- | --- |
| `npm --prefix app/SiloUI run typecheck` | Passed |
| `npm --prefix app/SiloUI run lint` | Passed with 12 warnings; O-08 |
| `npm --prefix app/SiloUI test -- --maxWorkers=2` | 197 files; **1,789 passed** |
| `npm --prefix app/SiloUI run build` | Passed; browser assets only, no native bundle/runtime preparation |
| `cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check` | Passed |
| `cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked` | **1,302 passed, 20 ignored** with local socket access; 3 compiler warnings |
| `npm --prefix app/SiloUI run test:release` | **69 passed, 12 skipped**, 81 total |
| `python3 -m unittest discover -s app/SiloUI/scripts -p 'test_*.py'` | **282 passed, 11 skipped**, 293 total |
| `npm --prefix website test -- --maxWorkers=2` | Node tests: **12 passed**; Vitest: **4 passed, 1 failed**; R-14 |
| `npm --prefix demo test` | **13 passed** |
| `npm --prefix website run typecheck`; `npm --prefix demo run typecheck` | Both passed |

The first native run returned 1,264 passes, 38 failures, and 20 ignored because the sandbox denied socket creation. Its exact log was retained. The rerun with local socket access passed; the initial failures are an execution-environment limitation, not 38 product defects. Skipped and ignored cases do not qualify live VM, Linux/APT, container, TLS, or platform-specific behavior. Build success does not establish release readiness.

| Finding evidence | Provenance in this pass |
| --- | --- |
| R-01/R-02 | Silo failure/recovery paths rechecked; pinned upstream checkpoint coordinator/executor read locally. Earlier malformed-history/Paused fixture outputs retained as prior observations; those fixtures were not rerun here. |
| R-03–R-06/R-15 | Existing production-source/component diagnostics rerun: **5/5**, Node 26.10.0. Main suite above separately ran on Node 24. |
| R-17/R-18/R-22 | Existing component diagnostics rerun: **4/4**, Node 26.10.0. |
| R-07/R-19 | Current `SessionSecret` extracted and executed with synthetic values; deterministically interleaved updates erased a new append; failed token persistence changed the active cached value. |
| R-08/R-10/R-12/R-16/R-21 | Current-source confirmation; R-21's drafted barrier regression remains unexecuted. |
| R-09 | Current readiness/ownership source checked; unrelated-listener fixture is prior evidence and was not rerun here. |
| R-11/R-13/R-20 | Actual guest/helper/publication/staging functions exercised again with local fixtures. APT fixture mocked only GPG verification; no live APT client. |
| R-14 | Existing website failure rerun on Node 24. |
| R-23/R-26 | New real-store/component diagnostics: **2/2**, rerun on Node 24.11.1. |
| R-24 | Actual `copy_home` called in production merge order with temporary synthetic homes; external sentinel overwritten. |
| R-25 | Unchanged timeout selector extracted/compiled with Rust 1.94.0; all checkpoint methods returned 600 seconds. Owner completion/retry paths source traced. |

Main check logs are ignored evidence under `app/SiloUI/src-tauri/target/verification/comprehensive-review-2026-10-02/`. New frontend fixtures and results are under `frontend-audit-new-2026-10-02/`; guest/tooling diagnostic results under `tooling-review-final-2026-10-02/`; rerun prior frontend diagnostics under `frontend-audit/` and `frontend-audit-2/`. The new credential and checkpoint-timeout source harnesses were copied into the comprehensive-review evidence directory. These files are local diagnostic artifacts, not committed test coverage. Findings below include acceptance criteria so fixes do not depend on retaining local files.

No current dependency advisory scan was completed. This pass did not send dependency metadata to an advisory service. No particular CVE or clean advisory status is claimed.

## Findings


All entries remain open. R-01–R-22 retain the earlier ledger IDs. The evidence provenance table above distinguishes newly executed fixtures from previous observations; “Observed” in a retained entry does not imply a new run in this pass.

| ID | Priority | Finding | Evidence |
| --- | --- | --- | --- |
| R-01 | P1 | Activity-history failure blocks lifecycle control and Quit | Extracted current seam reproduced |
| R-02 | P1 | Failed full capture can leave a VM Paused without ordinary recovery | Current seam reproduced; pinned upstream checked |
| R-03 | P2 | Older lifecycle response replaces another sandbox's newer state | Production-source fixture |
| R-04 | P2 | One slow remote holds back healthy Network and SSH rows | Production-source fixture |
| R-05 | P2 | Cross-computer port saves invalidate one another | Production-source fixture |
| R-06 | P2 | Same-named sandbox errors affect the wrong computer's port | Controller fixture |
| R-07 | P2 | Token-ledger read/modify/write loses concurrent retirement records | Current cache seam reproduced |
| R-08 | P2 | Push-token retirement lacks an independent retry trigger | Source confirmed |
| R-09 | P2 | Remote tunnel readiness accepts an unrelated listener | Extracted current function reproduced |
| R-10 | P2 | Published-port SSH sessions lack controller-crash lifetime ownership | Source confirmed |
| R-11 | P2 | Computer-use Ready can describe a previous boot | Actual guest helper fixture |
| R-12 | P2 | One unresolved checkpoint blocks all scheduled storage reclamation | Source confirmed |
| R-13 | P2 | Retained APT metadata references unretained packages/indexes | Actual publication function fixture |
| R-14 | P2 | Website behavior test fails outside CI coverage | Existing test failed |
| R-15 | P3 | Hidden computer-use views continue polling | React fixture |
| R-16 | P3 | Current documentation contradicts runtime and remediation state | Manifest/docs comparison |
| R-17 | P2 | Late migration response hides completion or failure | Two React fixtures |
| R-18 | P3 | Initial Storage error toast's Retry button does nothing | Actual Retry clicked in fixture |
| R-19 | P2 | Failed personal-token persistence still replaces the active token | Cache seam reproduced; caller traced |
| R-20 | P2 | Go/compiler-recipe changes do not invalidate LFS executable caches | Actual staging function fixture |
| R-21 | P2 | Changing approval back during an active apply can strand the opposite mode | Source confirmed at cutoff |
| R-22 | P2 | Older remote responses lose an explicit approval-mismatch warning | Parser and real-panel fixture rerun |
| R-23 | P2 | Update installation proceeds after settings delivery fails | Real settings-store/UI fixture |
| R-24 | P2 | Home migration follows destination links and overwrites external files | Actual Python helper fixture |
| R-25 | P2 | Remote checkpoint timeout is shorter than supported owner work | Extracted timeout function; owner control flow |
| R-26 | P3 | Failed local status subscription leaves download progress stale | Real store/status-view fixture |

### R-01. Activity history is a prerequisite for Stop

**Location:** [runtime_activity.rs](../app/SiloUI/src-tauri/src/runtime_activity.rs), lines 32–43, 58–65, 130; [lifecycle_recovery.rs](../app/SiloUI/src-tauri/src/runtime/lifecycle_recovery.rs), lines 238–245, 298; [shutdown.rs](../app/SiloUI/src-tauri/src/runtime/shutdown.rs), lines 177–184.

**Trigger:** `sandbox-activity.json` is malformed or unreadable. `begin` reads it before the actual lifecycle intent is persisted or executed. The current extracted function, with a temporary `{broken-json` file, returned `Malformed("Sandbox activity could not be decoded.")` for Stop.

**Consequence:** Start, Stop, and Restart fail before reaching the runtime. Quit uses the same path and cannot stop an otherwise controllable running VM. A history write failure after runtime completion also masks the operation's real outcome. If `finish` fails after cancellation, it prevents intent removal, allowing recovery to revisit an abandoned action.

**Fix:** make history advisory, preserving/quarantining damaged bytes and surfacing a warning. Keep the actual lifecycle intent durable and mandatory. Retire completed/cancelled intents independently of history writes.

**Acceptance:** malformed history plus a running fake VM must still execute Stop. Inject history failure after cancelled Start and verify recovery cannot replay the cancelled intent. The fixture proves the journal failure; no live VM or Quit was exercised.

### R-02. Failed full capture does not settle Paused source state

**Location:** [checkpoints.rs](../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), lines 660–692, 2022–2044, 2202–2205; [lifecycle_recovery.rs](../app/SiloUI/src-tauri/src/runtime/lifecycle_recovery.rs), lines 166–174.

**Trigger:** a full snapshot fails and leaves its source Paused. Silo only attempts resume for cancellation, discards that attempt's error, and otherwise records failure/cleans snapshot references. Quit's special Paused cleanup requires a restore journal, which an ordinary capture does not create.

This is a supported upstream failure state: the pinned [MicroSandbox executor](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/runner/control/executor.rs#L485-L500) retains suspension on `keep_paused`; its [checkpoint coordinator](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/checkpoint/coordinator.rs#L950-L969) sets this after failed source resume/thaw. Both exact-revision files were inspected locally; the executor was also fetched from the primary source. Recovery-owned suspension cannot simply be released by ordinary resume.

**Observed:** a failed full-capture fixture ended Paused with commands `["snapshot"]`; cancellation with failed resume ended Paused with `["snapshot", "resume"]`. The actual capture function was unchanged; runner/storage dependencies were synthetic.

**Consequence:** ordinary Start/Stop and checkpoint retry reject Paused, leaving no normal escape through those controls or graceful Quit.

**Fix:** inspect source state after every failed full capture with cancellation masked. Apply the established recovery policy, including an explicit recoverable state or carefully justified force-stop fallback, while preserving disks/diagnostics. Fix reusable recovery semantics upstream where appropriate.

**Acceptance:** generic error, timeout, and cancellation into Paused must end in an actionable recovery state; failed resume must not disappear. Then qualify the exact failure in Silo Dev with disposable VMs. This report does not claim every killed snapshot process pauses a real guest.

### R-03. A lifecycle result overwrites another sandbox's newer result

**Location:** [production-source.ts](../app/SiloUI/src/desktop/production-source.ts), `workspaceAction` response parsing/publication at lines 1092–1107; compare the ordinary local `readSnapshots` row guards at lines 924–939.

**Trigger:** a lifecycle operation for sandbox A captures a whole-computer response while B is still in its older state; B completes and publishes first; A's older response arrives last. The diagnostic fixture reproduces B's newer state being replaced.

**Consequence:** the UI can show a completed Stop/Start as undone and offer actions based on old state. Backend changes need not be lost for this to mislead the user. A later refresh can repair it, but the mutation response should preserve the same invariant as normal reads.

**Fix:** merge the target operation's result while preserving newer unrelated rows, using per-owner/per-sandbox settlement metadata. Do not discard all of A's successful result with another global revision.

**Acceptance:** reversed response order for simultaneous actions on different sandboxes preserves both completions; stale same-sandbox results cannot override newer intent.

### R-04. Network and SSH refresh wait for the slowest computer

**Location:** [production-source.ts](../app/SiloUI/src/desktop/production-source.ts), `refreshSshAccess` at lines 439–471 waits on `Promise.allSettled`; `refreshNetwork` at lines 478–501 reads local state and then waits on a remote `Promise.all` aggregate before publishing it.

**Trigger:** one remote stays pending while local settings change. The functions wait for every owner's result before publishing; the shared in-flight request also suppresses another refresh. A fixture changes local SSH from enabled to disabled and a port from 3000 to 4100 while deferring a remote, and observes both old values until it resolves.

**Consequence:** a healthy computer's addresses and access controls remain stale, or first load stays incomplete because of an unrelated remote.

**Fix:** publish each owner's result as it settles, with owner-specific timeouts, generations, and unavailable state. Reuse the existing independent computer-state refresh policy.

**Acceptance:** an indefinitely deferred remote cannot prevent fresh local/other-remote publication; its eventual old response cannot reverse newer saves.

### R-05. Port saves on different computers cancel each other's publication

**Location:** [production-source.ts](../app/SiloUI/src/desktop/production-source.ts), network mutation publication at lines 521–529; compare owner-scoped SSH saves at lines 1561–1575.

**Trigger:** start a slow local port save, navigate to a remote port form, and finish its save first. A single `networkRevision` marks the unrelated local response stale. The fixture receives local port 4100 successfully but retains 3000 in the store.

**Consequence:** successful feedback disagrees with the displayed/copyable endpoint. The discarded result does not itself guarantee a confirming refresh.

**Fix:** track network mutation revisions by computer and merge independent owners. Arrange an authoritative follow-up read when discarding a same-owner result leaves uncertain state.

**Acceptance:** local/remote saves in either completion order retain both changes; reversed same-owner responses preserve the latest intent.

### R-06. Network error identity is derived from a display name

**Location:** [network-ports-state.ts](../app/SiloUI/src/features/application/components/network-ports-state.ts), lines 21–24, 85–95; [network-ports.tsx](../app/SiloUI/src/features/application/components/network-ports.tsx), line 67; [network-page.tsx](../app/SiloUI/src/features/application/pages/network-page.tsx), line 36.

**Trigger:** local `dev` is healthy while remote `dev` has an error. The controller formats errors as `name: message` and later matches their prefixes. The same-name fixture turns the local port from Reachable to Unknown; the rendering condition then removes Open.

**Consequence:** one computer's failure disables another computer's valid endpoint.

**Fix:** preserve computer/sandbox identifiers with each error, or pass the selected row's error directly. Format display names only at the presentation boundary, adding computer names when ambiguous.

**Acceptance:** two `dev` sandboxes on different computers, with only one failing, preserve the healthy row's Reachable state and Open control.

### R-07. Retirement-ledger updates are not atomic transactions

**Location:** [github.rs](../app/SiloUI/src-tauri/src/github.rs), `SessionSecret` at lines 312–348, `remember_token` at 1128–1135, `retire_unused` at 1177–1199, and `HostPushCredential::drop` at 1999–2023.

**Trigger:** two push-token revocations fail concurrently, or a destructor adds a token while the worker retires an earlier ledger snapshot. Cache reads and writes are individually locked, but the read/modify/write transaction is not. The worker's operation lock does not cover every destructor caller.

**Consequence:** replacing a cloned ledger loses a newly appended token or another workspace's record, removing the only retry record for scoped write authority. The cache primitive's deterministic interleaving reproduces the lost entry. No token theft was demonstrated.

**Fix:** add an atomic ledger update operation. Snapshot candidates before network calls, then remove only successfully revoked candidates from the current ledger in a locked transaction. Append through the same transaction. Do not hold the general operation lock across destructor network calls.

**Acceptance:** append a failed token after retirement reads but before it commits; the new token and unrelated workspaces survive. Barrier two simultaneous appends as a separate case. Test durable-write failure without losing in-memory pending records.

### R-08. Host-push revocation retries depend on unrelated grant renewal

**Location:** [github.rs](../app/SiloUI/src-tauri/src/github.rs), lines 1136–1139, 1225–1246, 1408–1413, 1999–2023, 2100–2117, 2637–2647.

**Trigger:** a push finishes while token revocation fails, then connectivity recovers while the workspace's normal grants remain current. Remembering a token sets `grants_issued`; it does not independently make retirement due. The worker's extra ledger sweep covers removed workspaces, not every configured owner with pending retirement.

**Consequence:** authority lasts longer than the intended end-of-push boundary. Retirement waits for grant renewal/policy change; disabled access with empty grants can lack a session retry trigger. A related crash window exists because a scoped host-push token is recorded only after failed destructor revocation: a crash before Drop leaves no retirement entry. GitHub expiry/revocation still bounds these credentials; this is not permanent authority. These are source findings, not live GitHub incident reproductions.

**Fix:** persist issued push credentials before using them and schedule retirement independently in the existing worker, with an explicit retry deadline. Track active use so concurrent cleanup cannot revoke a token during its push. Keep personal tokens outside App-token automatic revocation policy.

**Acceptance:** pending retirement retries when its own clock advances even with valid unchanged grants or disabled access. Relaunch after a synthetic crash between issuance/use and Drop must discover the pending token. No VM credential reattachment should be required just to retire a host credential.

### R-09. A listening TCP port does not prove the SSH forward is ready

**Location:** [remote_network.rs](../app/SiloUI/src-tauri/src/remote_network.rs), lines 267–297, 484–513.

**Trigger:** Silo reserves a loopback port, releases it, starts SSH, then accepts any successful connection to that port while the child remains alive. Another local process can bind in the release/start gap while SSH is still authenticating. The synthetic seam pairs an unrelated listener with a live child; the acceptance test does not identify the listener's owner.

**Consequence:** Silo can present/open the wrong local endpoint. SSH's later exit can correct subsequent state, but the first readiness claim is false. No credential disclosure or hostile timing against real OpenSSH was demonstrated.

**Fix:** confirm successful forwarding through a supported OpenSSH mechanism, or retain a Silo-owned listener and use a private Unix-socket transport. Evaluate the [OpenSSH controls and forwarding behavior](https://man.openbsd.org/ssh.1) before adding custom relay infrastructure; TCP accept alone is insufficient.

**Acceptance:** an unrelated listener plus a child that remains alive before failing forwarding must never produce Ready. Include delayed authentication and selected-port collisions.

### R-10. Published-port SSH tunnels can survive a controller crash

**Location:** [remote_network.rs](../app/SiloUI/src-tauri/src/remote_network.rs), lines 29–32, 272–276, 525–528; [remote.rs](../app/SiloUI/src-tauri/src/remote.rs), lines 617–642; compare [desktop_viewer.rs](../app/SiloUI/src-tauri/src/desktop_viewer.rs), lines 21–92.

**Trigger:** Silo exits abnormally with a healthy `ssh -N` forward. This path gives the child null streams and relies on explicit close or a Rust destructor. Neither runs after a controller crash. [Rust's Child documentation](https://doc.rust-lang.org/std/process/struct.Child.html) also makes process cleanup an explicit owner responsibility.

**Consequence:** a forward can retain a local port and remote connection after the app is gone; the relaunched process has no live-registry entry for it. SSH ProxyCommand descendants need the same lifetime ownership.

**Fix:** reuse the already implemented parent-lifetime pipe and owned process-group policy from desktop tunnels. Keep PID/group ownership checks; do not replace this with broad process-name cleanup.

**Acceptance:** terminate only a verified test controller that owns a fixture forward; its listener and descendants must disappear within a bound. Real OpenSSH crash lifetime and ProxyCommand behavior remain unqualified here.

### R-11. Computer-use Ready survives a failed new boot without a probe

**Location:** [silo-computer-use.py](../app/SiloUI/src-tauri/guest/silo-computer-use.py), lines 202–234, 520–543; [computer_use.rs](../app/SiloUI/src-tauri/src/computer_use.rs), lines 1094–1096.

**Trigger:** setup once writes a matching Ready receipt and the LCU executable still exists, but the next desktop session fails. `update` returns early even for `boot=True`, before current LCU/session checks, repair, or doctor.

**Observed:** the actual helper's temporary guest fixture reported `initialState: ready`, `currentSession: failed`, `bootState: ready`, `bootReadiness: ready`, and `commandsAfterBoot: []`. This was reproduced again after the merge using `apply('ask', boot=True)` instead of the former `sync` entry point; the per-run approval outcome was also `applied`.

**Consequence:** the UI promises present readiness based on a historical installation. The user discovers the failure only through an agent or interaction.

**Fix:** separate installed/registered state from current readiness. Preserve skip-install behavior, but revalidate at boot using bounded probes/repair and a boot identity or verification timestamp.

**Acceptance:** a ready receipt followed by a failed boot must probe/repair or report failure. A healthy warm boot must avoid reinstalling unchanged components.

### R-12. One checkpoint recovery failure disables all scheduled trim

**Location:** [storage.rs](../app/SiloUI/src-tauri/src/runtime/storage.rs), lines 684–700; [checkpoints.rs](../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), lines 2788–2808.

**Trigger:** during initial monitor recovery, one checkpoint record is unreadable or interrupted capture cleanup cannot finish. `recover_interrupted` returns early; the monitor never sets its single `recovered` flag, so it never calls periodic reclamation.

**Consequence:** healthy sandboxes lose scheduled reclamation until the unrelated checkpoint problem is resolved. The monitor retries each minute and prints an error, without explaining the storage-wide dependency in the affected UI. A failure appearing after `recovered=true` does not trigger this particular branch.

**Fix:** return owner-specific recovery results, continue independent entries, and let healthy owners receive maintenance. Preserve unknown dependencies/data and retry unresolved cleanup separately.

**Acceptance:** one damaged checkpoint and one healthy due-for-trim VM must preserve/report the damaged state while trimming the healthy VM during a deterministic maintenance tick.

### R-13. APT retention preserves indexes without their package objects

**Location:** [download-apt-releases.py](../app/SiloUI/scripts/download-apt-releases.py), lines 28–34; [apt-repository.py](../app/SiloUI/scripts/apt-repository.py), lines 45–75, 90–119.

**Trigger:** publish releases 0.1/0.2, then 0.2/0.3, then 0.3/0.4 within the 14-day metadata validity period. The downloader retains two releases; preservation copies indexes from the immediately previous manifest, without their referenced pool packages or the earlier hash-index retention chain.

**Observed:** the actual preservation function retained the previous index, omitted a package it advertised, and dropped a hash index from two publications earlier. Only the GPG verification seam was mocked; this is a file-publication reproduction, not a signature test or live APT result.

**Consequence:** clients with cached/unexpired metadata or a selected older candidate can hit missing files. The ordinary in-app update helper refreshes APT first, so this does not establish every update fails. See the primary [APT index-generation contract](https://manpages.debian.org/bookworm/apt-utils/apt-ftparchive.1.en.html).

**Fix:** define retention by time/metadata validity and retain the corresponding immutable package/index objects. Candidate indexes can still advertise only current supported releases. Check the repository's 900 MiB budget before selecting a publisher or storage policy.

**Acceptance:** an isolated Linux APT client holding old supported metadata can fetch its advertised package after several publications. Test expiration and cleanup separately.

### R-14. Website behavioral drift is invisible to CI

**Location:** [read-only-demo.test.tsx](../website/src/demo/read-only-demo.test.tsx), lines 40–53; [ci.yml](../.github/workflows/ci.yml), lines 52–61.

**Observed:** the existing website test fails at line 46 because it cannot find a button named `SSH controls for dev`. The shared production UI moved that interaction. The workflow installs and typechecks website/demo but does not run their test scripts.

**Consequence:** public-demo behavior can diverge from its stated contract while CI remains green. This establishes test/implementation disagreement, not that the new UI design is wrong.

**Fix:** update the demo test around intended current navigation, then include website/demo tests in CI. Keep the read-only demo incapable of native operations.

**Acceptance:** navigate to local/remote SSH details using the current UI, verify intended controls, and assert that every mutating/native action stays disabled.

### R-15. Hidden computer-use views continue native polling

**Location:** [computer-use-panel.tsx](../app/SiloUI/src/desktop/computer-use-panel.tsx), lines 196–211; [computer-detail-page.tsx](../app/SiloUI/src/features/application/pages/computer-detail-page.tsx), line 481; [application-app.tsx](../app/SiloUI/src/features/application/application-app.tsx), lines 351–365.

**Trigger:** open a desktop sandbox, then navigate away or hide the document. Sections remain mounted and this interval receives no active-page/visibility condition. The fixture observed the initial read plus three further reads over 15 seconds while hidden.

**Consequence:** guest subprocess reads and remote traffic continue after the principal source stops polling. Five-second polling allows 720 periodic reads/hour for this panel if each finishes before the next tick. CPU/battery cost was not measured, and in-flight suppression prevents overlapping ticks.

**Fix:** pass the existing active-page state, pause periodic reads when the document is hidden, and refresh once on return. Explicit setup/download operations should still settle while hidden.

**Acceptance:** inactive/hidden views make no periodic reads, reactivation makes one fresh read, and explicit work still completes. Apply the same ownership review to nearby viewer/app-status timers.

### R-16. Current documentation contradicts current implementation

**Location:** [documentation index](README.md), Runtime row; [remediation plan](SiloUI-REVIEW-REMEDIATION-PLAN.md), line 10; [runtime inputs](../app/SiloUI/runtime-inputs.json).

The index says the current bundled engine is `0.7.2`, while the manifest pins `0.7.6`. The remediation plan's introduction says nothing has been implemented although its ledger lists landed changes. Dated historical claims need an explicit snapshot/current-status boundary.

**Consequence:** contributors can reason about the wrong runtime or reopen fixed work. This affects debugging and release decisions, not merely wording.

**Fix/acceptance:** update current-version/status pointers, identify historical snapshot statements, and reconcile introductions with the live ledger. Preserve the original historical findings. This review only adds its index entry; it does not silently revise those separate records.

### R-17. A late migration Retry response hides terminal state

**Location:** [runtime-migration-boundary.tsx](../app/SiloUI/src/desktop/runtime-migration-boundary.tsx), lines 75–90; [runtime_migration.rs](../app/SiloUI/src-tauri/src/runtime_migration.rs), lines 968–992.

**Trigger:** Retry captures a Running snapshot, spawns conversion, and returns that earlier snapshot. Conversion can emit completion/failure before its command response settles in the UI. Event reads and command responses both call `setState` without a common ordering guard.

**Observed:** two real-component fixtures first publish Complete or Failed from the event path, then resolve the older Retry response as Running. Both terminal states regress.

**Consequence:** a completed app disappears behind “Updating your sandboxes,” or a failed attempt loses its error and Retry actions. This boundary has no periodic recovery read, so another application event or relaunch is needed to correct it.

**Fix:** use revisioned migration snapshots or a shared publication generation across reads/mutations, with an authoritative follow-up read after Retry. The initial-read `eventSeen` guard alone does not order later reads or command responses.

**Acceptance:** delayed Retry-Running after event-Complete and event-Failed must preserve the terminal state and its actions. Also reverse two event-read responses. Native timing was not exercised live.

### R-18. Storage's first failure notification offers an inert Retry

**Location:** [workspace-storage-panel.tsx](../app/SiloUI/src/features/application/pages/workspace-storage-panel.tsx), lines 36–53; [its existing test](../app/SiloUI/src/features/application/pages/workspace-storage-panel.test.tsx), lines 60–70.

**Trigger:** the initial storage read fails. The mount effect captures `load` from the first render, where `busy=true`; the failure toast retains that callback. Clicking Retry returns immediately on the captured busy guard even after loading has finished.

**Observed:** clicking the actual toast Retry makes no additional read. Clicking toolbar Refresh succeeds. The existing test named around retry exercises Refresh and only checks that Retry buttons exist, leaving the advertised recovery action untested.

**Fix:** supply a stable retry handler using current busy/disabled/request ownership. Adding the render-local `load` function blindly to effect dependencies would create a new refresh-loop risk.

**Acceptance:** first read rejects; the actual toast Retry invokes the second read and displays its measurements. Repeated clicks during that request must not overlap. P3 reflects the working toolbar workaround.

### R-19. Failed personal-token storage still changes the active credential

**Location:** [github.rs](../app/SiloUI/src-tauri/src/github.rs), `SessionSecret::write`, lines 326–348; [github_personal_token.rs](../app/SiloUI/src-tauri/src/github_personal_token.rs), `value`, `check`, and `save_github_personal_token`, especially lines 269–284 and 323–334.

**Trigger:** account A's token is connected; the user saves a validated replacement B; the system credential store rejects the write. `SessionSecret::write` publishes B in memory before returning the error. The caller's `?` exits before publishing B's account/status or applying the normal change/revision path.

**Consequence:** the failed operation can change which token host consumers use while the UI still describes A. A subsequent check can report B as `saved:true` despite no successful persistence; relaunch reads the prior stored token. The shared cache intentionally preserves failed OAuth rotations, but the personal-token UI does not implement that unsaved-session policy or a corresponding flush path.

**Evidence:** the current cache primitive's failed-write behavior is deterministic; the personal-token caller's early return and status projection establish the integration mismatch. No real Keychain failure or live GitHub identity change was attempted.

**Fix:** make explicit personal-token replacement transactional, or deliberately support a session-only credential state with accurate account, unsaved status, revision propagation, retry, and relaunch semantics. Preserve OAuth's separate requirement to retain a freshly rotated credential after persistence fails.

**Acceptance:** deny storage when replacing A with B and assert the chosen contract across credential reads, UI account, saved flag, guest reconciliation, Retry, and relaunch. A failed save must not silently split those views of identity.

### R-20. Compiler updates do not reach a cached LFS transfer executable

**Location:** [lfs-transfer-runtime.mjs](../app/SiloUI/scripts/lfs-transfer-runtime.mjs), lines 54–78, 126–130; [prepare-release-runtime action](../.github/actions/prepare-release-runtime/action.yml), lines 42–45, 101; compare [microsandbox-runtime.mjs](../app/SiloUI/scripts/microsandbox-runtime.mjs), lines 113–121.

**Trigger:** change Go, requested `GOTOOLCHAIN`, or build flags without changing the LFS upstream source pin. The staged/shared artifact is accepted by source commit/hash, architecture, and its own binary hash. Neither its manifest nor acceptance includes compiler/recipe identity. The outer cache permits fallback restoration; its key also omits Go identity.

**Observed:** actual `stageLfsTransferRuntime` accepted a synthetic old compiled cache after the requested toolchain changed, without checking the compiler or fetching/building source.

**Consequence:** a compiler/standard-library fix or recipe change can fail to reach a package despite successful preparation. No specific vulnerable compiler version is asserted.

**Fix:** define and validate effective compiler version and build recipe in the artifact identity and manifest before using either cache. Respect [Go toolchain selection](https://go.dev/doc/toolchain); a host `go` binary alone is not necessarily the effective compiler. The existing MicroSandbox cache already incorporates compiler/features.

**Acceptance:** unchanged identity reuses output; compiler or flags changing forces rebuild even with a valid fallback cache. Retain source/binary digest validation.

### R-21. Changing approval back can leave an opposite-mode apply uncorrected

**Location:** [computer_use.rs](../app/SiloUI/src-tauri/src/computer_use.rs), `needs_apply` at lines 143–146, `set_approval` at 247–257, completion recording at 263–272 and 815–816, and scheduling at 1196–1198. Introduced by the reviewed computer-use merge.

**Trigger:** Ask was applied successfully. Change to Auto and let its helper begin, then change back to Ask before Auto completes. Saving Ask preserves the previous completed Ask attempt; `needs_apply()` returns false, so the second change schedules no worker. It does not account for the in-flight Auto attempt. Auto then completes and records itself as the last applied mode.

**Consequence:** desired Ask and applied Auto diverge, with Pending shown but no executor left to converge them until another trigger such as boot or manual setup. Removing the synchronous approval command's gate permits this interleaving intentionally; the background executor must account for it. Approval is a convenience setting for root-capable guest agents, not a sandbox security boundary. This finding concerns honoring the user's latest choice.

**Evidence:** source-confirmed state transition, including the absence of a completion-time recheck. A regression using the existing guest/barrier test seam was drafted in local evidence but was not executed or added to the suite. Do not count it among reproduced cases.

**Fix:** schedule when in-flight work can invalidate the last completed match, or recheck desired mode after each helper completion and converge through the existing serialized executor. Preserve latest intent without launching overlapping guest applies.

**Acceptance:** seed completed Ask, hold the Auto helper, request Ask, release Auto, and assert a subsequent Ask apply completes. End with desired Ask, applied Ask, no pending entry, and no overlapping helpers. Existing tests for queued changes without prior applied history do not cover this case.

### R-22. Older remotes lose the specific approval-mismatch warning

**Location:** [linux-desktop-state.ts](../app/SiloUI/src/desktop/linux-desktop-state.ts), lines 20–22; [computer-use-panel.tsx](../app/SiloUI/src/desktop/computer-use-panel.tsx), lines 123–126. Introduced by the reviewed computer-use merge.

**Trigger:** an older remote reports `approval: ask` and `appliedApproval: auto`, but has no new `approvalApply` field. The parser defaults missing or invalid values to `applied`. The panel recognizes the conflicting modes but only displays the specific warning for pending/failed/partial apply states. The preceding native implementation can produce this response shape.

**Observed:** an older-shape failed response passed through the real parser and panel loses the warning that agents can still act without asking, while the off switch remains visible. Generic setup-failure feedback can remain; this is not a claim that all failure feedback disappears.

**Fix:** keep known desired/applied mismatch warnings independent of the new enum. Represent legacy/unknown application state truthfully instead of treating omitted or malformed data as success.

**Acceptance:** test older remote ask/auto and auto/ask combinations, plus current and malformed apply-state fields. The UI must explain known disagreement without pretending approval is an isolation boundary. The parser/panel diagnostic reproduces the ask/auto case at the cutoff.

### R-23. Update installation proceeds after settings delivery fails

**P2; production-component fixture reproduced.** Location: [production-surface.tsx](../app/SiloUI/src/desktop/production-surface.tsx), lines 61–69; [settings-store.ts](../app/SiloUI/src/features/preferences/settings-store.ts), lines 179–193; [updates.rs](../app/SiloUI/src-tauri/src/updates.rs), lines 761–776; [settings.rs](../app/SiloUI/src-tauri/src/settings.rs), `flush_for_update`, lines 2017–2026.

**Trigger:** a preference or onboarding draft remains queued in the frontend because its native delivery failed. Install awaits `settingsStore.flush()`, then invokes the native installer. The real store catches errors and, when pending writes still have a transport error, returns normally. Awaiting it is not proof of persistence.

**Observed:** a fixture rendering the real ProductionSurface, UpdatesProvider, and SettingsStore queued theme Dark over stored Light. Delivery failed twice, including the install-time flush. The store retained `saveError` and the optimistic value, but native `install(false)` was called. Only native transport and unrelated heavy UI were replaced.

**Consequence:** a successful native installation/restart can discard undelivered preferences or onboarding drafts. Native `flush_for_update` saves only state already delivered to Rust; restart skips the ordinary exit-flush handshake. No real update was installed and no credential loss is claimed.

**Fix:** give the store an explicit flush-success contract or check its resulting `saveError`/write-protection state before installing. Reuse the invariant already enforced by Quit in [desktop/settings.ts](../app/SiloUI/src/desktop/settings.ts), lines 82–88. The existing update test mocks `flush` rejecting, which the real implementation does not do; correct that seam.

**Acceptance:** with the actual settings store, failed delivery keeps edits and prevents any install call. Restore delivery, retry, verify durable state, and install exactly once. Cover preferences and onboarding drafts.

### R-24. Working-account migration overwrites files through destination symlinks

**P2; actual helper reproduced.** Location: [working-account.py](../app/SiloUI/src-tauri/guest/working-account.py), lines 62–67 and merge order at 151–155.

**Trigger:** root's legacy home contains `.config` linked to a workspace directory; the old desktop home contains a real `.config` directory with a same-named file. Copying root first preserves the link in the new home. Copying the desktop home next traverses that destination link. The pre-copy loop removes a destination link only when the source is also a link.

**Observed:** calling the actual helper twice in production order changed the external `editor.json` sentinel from `existing-workspace-settings` to `legacy-desktop-settings`; the destination remained a symlink. All paths were temporary synthetic homes. This requires neither an attacker nor a race; retries with existing links have the same conflict.

**Consequence:** migration silently modifies files outside the new home while appearing to preserve/copy user configuration. Guest root execution broadens the writable target set, but this is a guest data-integrity bug, not evidence of host escape or privilege escalation. Python's [copytree contract](https://docs.python.org/3.12/library/shutil.html#shutil.copytree) preserves links in the source tree and permits overwrites when merging existing directories; `symlinks=True` does not promise safe destination traversal. The fixture establishes the actual traversal.

**Fix:** define a conflict-preserving migration policy and copy without following destination symlinks, including intermediate path components. Preflight conflicting files/directories/links; preserve both sources and expose unresolved conflicts. Do not “fix” this by recursively deleting links or their targets without a preservation policy.

**Acceptance:** two-home merges and interrupted retries with file and directory link conflicts leave external sentinel bytes unchanged. Test nested links, dangling links, source-link versus destination-directory conflicts, ordinary files, credential/binary byte preservation, and safe retry after a reported conflict. Validate on the supported guest Python version before migration qualification.

### R-25. Remote checkpoint deadlines expire before supported owner work finishes

**P2; timeout seam reproduced and owner flow confirmed.** Location: [remote.rs](../app/SiloUI/src-tauri/src/remote.rs), lines 800–821, 943–949, 1037, 1057–1064; [checkpoints.rs](../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), lines 628, 1916, 2010; [remote/operations.rs](../app/SiloUI/src-tauri/src/remote/operations.rs), lines 206–212.

**Trigger:** a valid remote checkpoint capture takes more than ten minutes. The controller assigns create/fork/restore a generic 600-second request limit; capture and restore's recovery capture allow 900 seconds, and restore describes an expected complete duration up to 3,600 seconds. Queue and transport time consume the controller budget too.

**Observed:** the unchanged timeout selector returns 600 seconds for all three checkpoint methods. At timeout the controller kills its SSH process and reports unknown outcome. The owner condition guards admission, not work already started, so admitted work continues and stores its eventual result.

**Consequence:** supported long operations generate avoidable failures and lose their original result connection. Only connection-loss retries reuse an operation ID; this timeout is a different failure category and the caller receives no handle to reattach. This is not permanent state loss or inevitable duplicate execution: the frontend refreshes on error and blocks further checkpoint actions while observed owner state is running.

**Fix:** return/retain the existing owner operation ID and observe completion across connection deadlines, or specify a complete-operation budget covering queueing and every stage. Reuse the existing operation registry; an unrelated new scheduler is unnecessary. Simply raising 600 to 900 still leaves multi-stage restores outside the contract.

**Acceptance:** with a fake clock, a 601-second capture stays observable and produces one final result; reconnect attaches to the same operation. Expiration before admission still prevents unstarted work. Qualify a slow disposable two-computer checkpoint separately; this review did not run a ten-minute VM operation.

### R-26. A failed local status subscription leaves download progress stale

**P3; real store/status view reproduced.** Location: [computer-use-bridge.tsx](../app/SiloUI/src/desktop/computer-use-bridge.ts), lines 104–120; persistent settings containers in [application-app.tsx](../app/SiloUI/src/features/application/application-app.tsx), lines 402–406.

**Trigger:** registering the local ChatGPT-app download status listener rejects once. The rejection is swallowed and one status read runs. Local status deliberately skips periodic polling, leaving no listener, resubscription, or fallback refresh.

**Observed:** the fixture's first read returned Downloading, then its backend became Ready. After 60 seconds and focus/visibility events, the real view still showed Downloading: one read, one failed registration, no visible error, and no recovery button. Switching settings tabs does not necessarily remount the still-mounted store.

**Consequence:** completed background setup can appear stuck until the store remounts or the app restarts. This is separate from R-15's unnecessary polling of hidden sandbox views.

**Fix:** track listener registration as fallible state. Retry it with bounded backoff, or use visible fallback polling while events are unavailable; provide an explicit recovery action. Keep a successful subscription event-driven.

**Acceptance:** reject registration once, advance the fixture backend to Ready, recover events or fallback reads, and update the view without relaunch. Assert disposed/hidden views do not acquire permanent extra timers.

## Improvement opportunities


These are evidenced costs, missing guarantees, or investigation targets. They are not additional demonstrated production incidents. Optimize against a measured budget; do not use cleanup as a substitute for the recovery fixes above.

### O-01. Bound guest helper output and descendant lifetime

[silo-computer-use.py](../app/SiloUI/src-tauri/guest/silo-computer-use.py), lines 251–257, captures complete output before truncating the log tail. A timeout bounds elapsed time, not bytes or inherited pipes. Use incremental drain/bounded spool and retain a diagnostic tail; check process-group cleanup for timed-out installer descendants. Test a deliberately noisy child and a child leaving a descendant. No guest OOM was observed.

### O-02. Stream large build inputs instead of holding complete archives

[prepare-microsandbox-runtime.mjs](../app/SiloUI/scripts/prepare-microsandbox-runtime.mjs), lines 22–51, buffers fetch bodies; [guest-image.mjs](https://github.com/amontlabs/silo/blob/91c8145fdc37/app/SiloUI/scripts/guest-image.mjs), lines 24–33, reads full cached images for verification. The image lock records 414,843,188/423,476,714-byte archives. Use maintained stream primitives, incremental hashes, pinned-size limits, deadlines, temporary files, and atomic publication. Measure peak RSS and cold/warm preparation time before and after; no peak-memory failure was reproduced.

### O-03. Measure startup before splitting the main bundle

The verified browser build emits 1,072.98 kB minified JavaScript, 311.36 kB gzip, and 96.16 kB CSS. Vite warns above 500 kB. This proves artifact size, not launch slowness. Measure cold main-window, status-panel, and viewer startup on supported minimum hardware; defer optional routes/viewer code only where the measurement improves. Do not hide the warning or add arbitrary chunks as the success criterion.

### O-04. Checkpoint usage performs a computer-wide survey

[checkpoints.rs](../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), `survey` near line 2216 and usage at 2487–2560, inventories native snapshots, saved references, and runtime lineage for one sandbox's usage/deletion state. Count calls and measure latency with 1, 10, and 100 synthetic sandboxes. If material, separate prompt selected-owner byte totals from dependency surveying or share a correctly invalidated survey. Captures, restore, forks, import, and deletion must all invalidate it.

### O-05. Log virtualization does not bound computation or active history

[use-log-history.ts](../app/SiloUI/src/features/application/model/use-log-history.ts), lines 82–89, limits inactive cache bytes; active paging can keep growing. Lines 229–233 rebuild chronological rows when workspace presentation changes. [logs-page.tsx](../app/SiloUI/src/features/application/pages/logs-page.tsx), line 54, eagerly builds the full clipboard string. Profile a 50,000-record view during ordinary refreshes. Generate clipboard content on demand and isolate log ordering from unrelated workspace updates. Preserve access through paging/export if active memory is bounded.

### O-06. Export preflight can mutate lineage before acquiring its gate

[backup_controller.rs](../app/SiloUI/src-tauri/src/backup_controller.rs), near line 819, calls `export_source` before export work owns its gate. [checkpoints.rs](../app/SiloUI/src-tauri/src/runtime/checkpoints.rs), near line 872, can reach `ensure_snapshot_group`, which saves migrated lineage. Move mutation into the gated worker and keep preflight read-only. First reproduce concurrent legacy-lineage preflight/checkpoint work; no lost-update or data-loss outcome was established here.

### O-07. Notification text should receive domain identity explicitly

[network-ports-state.ts](../app/SiloUI/src/features/application/components/network-ports-state.ts), lines 58–65, extracts the sandbox using `id.split(":")[1]`; a remote target becomes `silo-remote`. [network-ports.tsx](../app/SiloUI/src/features/application/components/network-ports.tsx), lines 36–38, can use an encoded target in progress copy. Pass computer, sandbox ID, and display name as explicit fields. Verify remote add/edit/remove feedback names the right location and links to it. This is a narrow clarity improvement, not a reason to redesign notifications.

### O-08. Consolidate invariants at proven seams and resolve warnings

Large files include runtime.rs (12,006 lines), backup controller (5,213), GitHub (4,572), checkpoints (6,066), and production-source.ts (1,927), including substantial colocated tests. Size alone is not a defect. The actionable duplication is visible in the findings: read/mutation publication policies differ, two tunnel paths have different ownership guarantees, and credential kinds share caching semantics they do not all support.

Build small domain modules around owner-scoped state publication, token-ledger transactions, and owned tunnel lifetime after adding behavior regressions. Existing paired call sites provide concrete adapters. Keep capture recovery policy explicit and keep normal lifecycle history separate from its durable intent. Do not introduce a generic orchestration framework merely to shorten files.

Lint reports nine mixed helper/component export warnings, a state update in an effect, a missing effect dependency in Storage, and a ref read during render. R-18 proves one affected behavior; the others are review leads, not automatic defects. Native compilation emits two unused-variable warnings and one platform-specific dead-code warning. Resolve deliberately and keep tests about externally observable behavior rather than helper implementation shape.

### O-09. Dependency advisory status is missing security evidence

[Dependabot](../.github/dependabot.yml) covers GitHub Actions only. Inspected workflows do not provide npm/Cargo advisory checks or a guest-image advisory policy. Pins and signatures establish identity/reproducibility, not the absence of newly disclosed vulnerabilities.

Use maintained advisory/update tooling for npm lockfiles and Cargo, with an explicit review policy for the vendored updater, bundled runtime, guest OS, and Go helper. Record an all-target qualification path for upstream security updates. No current advisory scan was completed and no particular CVE is claimed. This review did not send dependency metadata to a remote advisory service. A clean test suite is not a dependency-security assessment.

### O-10. Host Git's deadline does not cover final pipe draining

[host_push.rs](../app/SiloUI/src-tauri/src/host_push.rs), lines 517–553, stops the group on failure, then joins output-reader threads without a deadline. If a successfully exited child leaves a descendant holding a pipe, the join outlives the main process deadline. This is an execution-seam concern; a realistic trigger using the packaged Git/LFS helpers was not established, so it is not promoted to a production defect.

Keep the deadline/cancellation policy through output draining and process ownership cleanup. Qualify a bounded fixture with a successful parent and inherited-pipe descendant, then test the real helpers before choosing a shared process abstraction. Preserve the runtime's intentionally detached VM processes; their lifetime policy is different.

### O-11. Authenticate every release artifact independently of editable draft checksums

[publish-release.py](../app/SiloUI/scripts/publish-release.py), lines 94–116, verifies hashes downloaded alongside draft assets. The signature verifier covers AppImages and the macOS update archive. [verify-release-metadata.py](../app/SiloUI/scripts/verify-release-metadata.py), lines 28–53, does not inspect the DMG and checks Debian packages only for version and architecture. A narrow fixture passed arbitrary text named as the DMG through that metadata verifier; it did not run the complete publishing workflow.

Sign the complete asset manifest at build time, or use maintained build-provenance attestations, then verify every DMG/Debian asset before publication and APT signing. Test a substituted package with recomputed draft checksums. Treat this as defense in depth against packaging mistakes or asset substitution, not a proven privilege escalation: a release-write actor can already publish directly. The macOS packager creates both macOS outputs from the same verified app, reducing accidental divergence. Do not claim signed updater verification is absent; it is present for the updater artifacts.

## Security controls and useful structure to preserve

The review found concrete security-relevant consistency defects, principally credential retirement, personal-token identity after failed storage, and forwarding ownership. It did not establish a new bypass of viewer authentication, guest webview capability separation, host-push repository/branch/commit binding, archive path/size checks, verified ChatGPT package extraction, or production/Dev channel isolation.

Preserve those controls while fixing failures. The host-push path clears inherited Git configuration, disables hooks/external protocols, checks objects, and passes credentials through a pipe; see [host_push.rs](../app/SiloUI/src-tauri/src/host_push.rs), lines 380–503, and the [Git configuration reference](https://git-scm.com/docs/git-config). Archive import checks lengths, manifest limits, hashes, free-space budget, and extracted paths. The package extractor uses descriptor-relative operations and ownership checks. These are meaningful boundaries with representative tests, not a security certification.

The existing operation registry, per-VM gates, owner-scoped SSH saves, cached stores, and fake runner seams are sufficient building blocks for most fixes. Prefer repairing their contracts to introducing new queues, persistence layers, or universal process managers. The known working desktop-tunnel lifetime policy provides the second concrete adapter for a small shared tunnel owner, if consolidation is justified.

Static frontend review found semantic controls, keyboard targets, error roles, and focus-restoration mechanisms. No user research or accessibility session was conducted. The supported usability findings are specific: stale or contradictory status, misleading Save/Install success, an inert Retry, a lost terminal migration state, and errors attached to the wrong computer. Fix those before undertaking a visual redesign.

## Qualification gaps

| Missing evidence | Required qualification |
| --- | --- |
| Real checkpoint failure/recovery | Exact Dev bundle; disposable VM; source pause/resume/thaw failures, cancellation, timeout, recovery and graceful Quit; preserve disks and logs |
| Migration data safety | Supported guest Python; two old homes, link conflicts, interrupted retries, user-state checksums before/after |
| Multi-computer correctness | Delayed/unreachable owner, duplicate sandbox names, concurrent saves/actions, long checkpoint, reconnect to same operation |
| Credential persistence/revocation | Isolated credential-store denial and restart; synthetic API failures/retry clocks; separately authorized live GitHub qualification |
| Process ownership | Crash only a verified fixture controller; assert its SSH listener/descendants disappear and unrelated processes remain untouched |
| Linux delivery/APT | Genuine signed metadata, cached older indexes across multiple publications, package reachability, clean install/upgrade and recovery on both architectures |
| Performance | Peak RSS during runtime preparation; cold startup and interaction latency on minimum hardware; 50,000-log and 100-sandbox fixtures |
| Accessibility | Keyboard-only workflows, screen reader, focus after modal errors, zoom/contrast and reduced-motion checks in the packaged app |
| Dependency health | Maintained current advisory scan and triage for npm/Cargo, vendor patch, guest OS, bundled runtime and Go helper |

The [Rust Child contract](https://doc.rust-lang.org/std/process/struct.Child.html) leaves child cleanup to its owner. The [OpenSSH forwarding controls](https://man.openbsd.org/ssh.1) and [ExitOnForwardFailure semantics](https://man.openbsd.org/ssh_config#ExitOnForwardFailure) should guide tunnel fixes; a successful TCP connection alone does not identify its listener. Research supported upstream facilities before introducing custom forwarding infrastructure.

## Remediation sequence and completion criteria

| Order | Work | Completion evidence |
| --- | --- | --- |
| 1 | R-01/R-02 lifecycle control and failed-capture recovery; R-24 migration preservation | Failing behavior regressions first, minimal fixes, then disposable-VM failure/relaunch verification |
| 2 | R-23 persistence before install; R-07/R-08/R-19 credential transactions/retries; R-21 latest approval convergence | Real store/cache seams under failed writes and controlled interleavings; state/account/relaunch agreement |
| 3 | R-03–R-06/R-17/R-22/R-25 owner identity, ordering and observation | Reversed responses, unreachable owners, older protocol payloads and reconnecting long operations |
| 4 | R-09–R-13/R-20 process lifetime, readiness, storage isolation and delivery | Listener collisions/crash ownership, fresh boot probes, healthy-owner maintenance, three-publication APT fixture, compiler cache invalidation |
| 5 | R-14–R-16/R-18/R-26 feedback, retry and verification coverage | Website/demo tests in CI, working recovery controls, hidden polling suspended, truthful docs/status |
| 6 | O-01–O-11 measured optimization and hardening | Publish before/after measurements or explicit invariant tests; preserve existing security and recovery contracts |

Review spec compliance and repository standards separately for each fix. A targeted regression proves the repaired behavior; a broad suite detects collateral damage; neither proves live platform readiness. User-visible fixes require individual patch changesets. This report is documentation only and needs no changeset.

**Next action:** add R-01's failing malformed-history Stop regression and make activity persistence advisory without weakening the authoritative lifecycle-intent journal.
