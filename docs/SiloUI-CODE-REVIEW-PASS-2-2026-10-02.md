# Silo second code review pass 2026-10-02

This pass found **11 additional defects present at the final cutoff: 9 P2 and 2 P3**. The most consequential affect private-key redaction, GitHub access choices, and checkpoint import semantics. It also reproduced a native test compilation failure on an intermediate revision that was subsequently removed from the checkout; that evidence is retained separately as historical R-38.

The [first comprehensive report](SiloUI-CODE-REVIEW-2026-10-02.md) remains the record for R-01 through R-26 and its improvement opportunities. This report adds R-27 through R-37 rather than counting those earlier findings again. Three subagents independently reviewed data preservation, native security/integrations, and frontend state; the primary reviewer checked their claims, updater ownership, upstream contracts, and verification. Each finding below gives a concrete trigger, evidence, consequence, recommended correction, and a test that would reject an incomplete fix.

## Revision and scope

The final source cutoff is **`a1197ae86037a6f01e04f4a2172836a13b2a1b2b`**, Silo `0.10.0`. The checkout changed during this pass, so results are tied to explicit revisions:

| Revision | Role in this report |
| --- | --- |
| `f9c1260` | First-report baseline; R-21 barrier regression reproduced the old failure |
| `ac83d94` | Intermediate checkout with live-verification changes; four native test compilation errors reproduced directly |
| `a1197ae` | Final checkout after removal of that live-verification merge; retains the approval convergence fix and all 11 new findings |

Relevant frontend, log, editor, application-selection, notification, and updater code was unchanged across these revisions. The checkpoint argument and export-admission diagnostics were extracted and executed again at `ac83d94`; their production logic remains unchanged at the final cutoff. No checkout movement was performed by this review.

MicroSandbox remains pinned to `0.7.6`, commit `09df3d4b9d832adaede1fb9a198cfc660bfab8cd`, with Silo's patches. Upstream behavior below was checked against the exact locally cached source and the bundled patches, not inferred from the latest release.

| Review area | Boundaries examined in this pass |
| --- | --- |
| Data preservation | Checkpoint export/import, native snapshot scope, deferred Start, runtime configuration reconstruction, space admission, restore cleanup, generation migration |
| Native security and integration | Log decoding/filtering/export, SSH editor repair, application selection, remote completion/failure notifications, update restart, single-instance ownership |
| Frontend | GitHub optimistic saves and Retry, onboarding hydration and recovery, policy serialization, duplicate names across computers, secrets editing, checkpoint actions, directory/log pagination, transfers |
| Verification | Focused component fixtures, extracted Rust functions, first-pass approval regression, source revision comparison, current native test compilation |

**P2** means a concrete correctness, credential-policy, availability, or verification defect. **P3** means a narrower defect with a practical workaround. Priority reflects the stated consequence, not an estimated incident rate. No P0/P1 issue, host escape, or automatic credential exfiltration was established in this pass.

An executed diagnostic that asserts the current bad behavior passes when it reproduces the defect. Such a pass is evidence of the defect, not evidence of a fix. Source-confirmed platform consequences are explicitly distinguished from executed guest or desktop behavior.

No production app, installed bundle, real sandbox, remote computer, production HOME, or credential store was used. All runtime inputs were synthetic. This review changed no application source, release notes, dependencies, version, or release state. Diagnostic files are ignored local evidence under `app/SiloUI/src-tauri/target/verification/`; they are not committed regression coverage.

## Findings at a glance

| ID | Priority | Finding | Evidence |
| --- | --- | --- | --- |
| R-27 | P2 | Private-key body lines survive redaction across log records | Real extracted query/redaction functions, generated disposable RSA key |
| R-28 | P2 | Importing a full checkpoint does not select the promised disk-only restore | Executed argument builder and exact upstream control flow |
| R-29 | P2 | Checkpoint Start omits the built-in GitHub environment default | Disk/full argument fixtures; post-restore policy path traced |
| R-30 | P2 | GitHub Retry resends the previously saved policy instead of rejected intent | Real React component diagnostic |
| R-31 | P2 | Onboarding ignores existing policies that arrive after its first render | Real component diagnostic; native replacement semantics traced |
| R-32 | P2 | Onboarding changes an existing token policy to OAuth | Real component diagnostic; native default traced |
| R-33 | P2 | Remote notification routes select the wrong same-named sandbox | Native route traced; real app route/action diagnostic |
| R-34 | P2 | Unrelated runtime storage can block a small export | Exact estimator with controlled free space |
| R-35 | P2 | Failed Debian reexec resumes work without the single-instance claim | Source-confirmed; no Linux session exercised |
| R-36 | P3 | Editor repair reports success after configuration writes fail | Extracted production repair with an actual write failure |
| R-37 | P3 | Linux accepts browser executables that its launch path rejects | Source-confirmed; no Linux desktop exercised |

## Detailed findings

### R-27 Private-key body lines survive redaction across records

**P2.** Locations: [runtime_activity.rs](../app/SiloUI/src-tauri/engine/src/runtime_activity.rs), lines 384–421; [runtime_logs.rs](../app/SiloUI/src-tauri/engine/src/runtime_logs.rs), lines 124–136, 280–306, 384, 803–807, and 995; [log_export.rs](../app/SiloUI/src-tauri/engine/src/log_export.rs), lines 147–148.

**Trigger and cause.** A command or console emits a PEM private key over multiple physical lines or execution-log records. `log_text` initializes `in_pem = false` for each call. The retained-log reader calls it separately for each decoded record. The BEGIN and END markers are hidden, but the body records have no marker and are returned unchanged. Search and cached-page reads use the same stateless filter. Export writes those entries without another redaction pass.

**Observed.** The diagnostic exercised unchanged extracted production read, scan, cache, decode, filter, and redaction functions against temporary `kernel.log` and `exec.log` files. All seven body lines from a newly generated disposable RSA key survived both query paths; the console query's serialized result retained them too. Passing the complete PEM block in one call was the redacted control. No real key was read or printed.

**Consequence.** Material that Silo explicitly tries to hide remains visible, searchable, copyable, and exportable when it crosses the normal record boundary. A user sharing an export can disclose it. The existing arbitrary-sensitive-output warning still matters; this finding concerns failure of the specifically implemented PEM rule, not a claim that logs can be made universally secret-free. Historical remediation D-37 and the single-string unit test at `runtime_activity.rs:428–447` do not cover the production boundary.

**Correction.** Classify sensitive blocks while scanning each stream/session, and preserve that classification in the index used by paging, context, and export. Redacting only the currently requested page cannot identify a block whose BEGIN record lies on an earlier page. Keep session boundaries explicit so interleaved commands do not corrupt one another's classification.

**Acceptance.** A disposable key split across console lines and execution records must leave no body bytes in search, page results, following output, surrounding context, or exported JSON. Include interleaved sessions, pagination starting inside the block, rotation, and an unterminated block. Keep ordinary multiline output readable.

### R-28 Full-checkpoint imports omit the disk-only restore selection

**P2.** Locations: [checkpoints.rs](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs), lines 321–329, 920–924, and 1166–1210; [backup.rs](../app/SiloUI/src-tauri/engine/src/backup.rs), lines 712–748 and the full-descriptor acceptance test at 6085–6087; [bundled help](../app/SiloUI/docs/silo-help.html), line 20.

**Trigger and cause.** Export a full checkpoint, import that export, then Start the imported sandbox. Import records `state: "disk"`, and availability deliberately accepts a native full snapshot for that desired state. Export/load preserves the full artifact. Start adds CPU/memory arguments for the disk branch but never sends `--disk-only`.

The pinned upstream [configuration defaults to Full restoration](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/sdk/rust/lib/sandbox/config.rs#L1141). Its [CLI selects DiskOnly only when the flag is present](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/restore.rs#L195-L207). The [native creation path](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/sdk/rust/lib/backend/local/sandbox/create.rs#L389-L415) materializes captured execution for Full and disk layers for DiskOnly. The bundled patches do not reverse those defaults.

**Observed and limit.** The exact production argument block was extracted and executed for both pending scopes. Neither includes `--disk-only`. Selection of native memory restoration is source-confirmed; no live RAM restore was performed.

**Consequence.** A full checkpoint follows the memory-resume path, or fails its compatibility requirements, where Silo promises import without the captured memory session. The disk-only label in Silo does not enforce that contract. Current host network/secret policy is still reconstructed; this finding does not establish a credential-policy bypass.

**Correction.** Retain the artifact's native scope separately from the desired restoration mode. Pass `--disk-only` when recovering disks from a native full checkpoint. Do not blindly add it to every restore: the pinned runtime rejects it for actual disk/file captures.

**Acceptance.** Import a disposable full checkpoint with a running process and a RAM-only sentinel. Start must change the boot ID, preserve checkpoint-time disk files, and leave the captured process/session absent. Test actual disk snapshots separately. The ignored `real_checkpoint_export_imports_and_cold_boots_checkpoint_time_disk` test in [backup_controller.rs](../app/SiloUI/src-tauri/engine/src/backup_controller.rs), lines 4262–4445, checks disk markers only; those survive both cold boot and RAM resume, so its current assertions cannot distinguish the defect.

### R-29 Checkpoint Start loses the GitHub CLI environment default

**P2.** Locations: [runtime.rs](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs), ordinary creation at 5307–5308 and policy modification at 2408–2416; [checkpoints.rs](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs), lines 1166–1210; [restore-policy patch](../app/SiloUI/patches/microsandbox-restore-policy-0.7.6.patch), lines 34–36, 91–98, and 167–170.

**Trigger and cause.** Start a sandbox from a checkpoint or imported disk. Ordinary creation supplies `--env GH_TOKEN=$MSB_SILO_GITHUB`. The restore argument path recreates labels, secret bindings, network policy, resources, and mounts, but supplies no environment defaults. A secret binding alone does not put the placeholder into `GH_TOKEN`. The bundled restore patch already supports `--env` for future exec commands.

Post-restore GitHub reconciliation updates secret bindings and identity; it does not restore the environment default. `setup-github.sh` installs a Git credential helper, which does not supply the missing GitHub CLI variable. Backup validation also permits environment entries, while the import pending record does not retain the accepted `runtime_config.env` fields. See [backup.rs](../app/SiloUI/src-tauri/engine/src/backup.rs), lines 2415–2429, and [backup_controller.rs](../app/SiloUI/src-tauri/engine/src/backup_controller.rs), lines 1883–1915.

**Observed.** Executing the unchanged restore argument builder produced `GH_TOKEN_argument=false` and `env_flags=0` for both disk and full cases. No GitHub request or guest command was run.

**Consequence and boundary.** Cold disk restores and newly executed commands lose the configured automatic `gh` authentication path despite retaining a GitHub secret binding. A resumed process in a full snapshot can retain its captured environment, so not every existing session is affected. Accepted portable environment overrides are also discarded on import. The outcome of an individual `gh` command can still depend on independently configured guest credentials.

**Correction.** Reapply the built-in placeholder at the existing restore boundary. Explicitly define which additional environment fields are portable; preserve and validate those fields or reject an unsupported export contract. Use the already bundled upstream feature rather than introducing another credential mechanism.

**Acceptance.** A newly executed guest command after disk import and full-checkpoint fork must receive the placeholder. Verify denied and currently authorized synthetic GitHub policies without copying old credentials. The ignored live GitHub test in [runtime_github_tests.rs](../app/SiloUI/src-tauri/engine/src/runtime_github_tests.rs), lines 361–385, expects a post-restore HTTP response; ordinary unit tests do not exercise that path.

### R-30 GitHub Retry resends saved state instead of the rejected choice

**P2.** Locations: [github-page.tsx](../app/SiloUI/src/features/application/pages/github-page.tsx), lines 156–170, 237–260, and 301–304; [github.rs](../app/SiloUI/src-tauri/engine/src/github.rs), lines 2955–2959.

**Trigger and cause.** Enable All repositories, disable it before the first save settles, let the second save receive a stale-revision rejection, then refresh and click Retry. Rejected saves retain workspace names, not their values. A refreshed authoritative snapshot replaces the repository/access draft. Retry submits that replaced draft.

**Observed.** The real React component first submitted `all`, then `selected`. Refresh installed the accepted first policy. Retry submitted `all` against revision 11. The latest rejected user choice was lost. The native stale-revision guard works as intended; the frontend loses the intent needed to recover from its rejection.

**Consequence.** Retry can preserve broader repository access than the user's latest choice while appearing to retry that choice. The draft replacement also covers repository selections and submitted identity intents; the concrete executed case was the All repositories toggle.

**Correction.** Keep pending/rejected values separately from the authoritative snapshot. Rebase the retained intent on the latest revision explicitly. Preserve the current protection for identity text still being edited.

**Acceptance.** In the observed sequence, Retry must send `selected` at the new revision. Cover the reverse toggle order, interleaved saves for different sandboxes, and a source refresh between failure and Retry. Assert the submitted payload, not only the visible toggle or error message.

### R-31 Onboarding misses policies that load after its initial render

**P2.** Locations: [onboarding-app.tsx](../app/SiloUI/src/features/onboarding/onboarding-app.tsx), lines 155–168, 198–216, and 351–376; [production-surface.tsx](../app/SiloUI/src/desktop/production-surface.tsx), lines 88–92; [production-onboarding.tsx](../app/SiloUI/src/desktop/production-onboarding.tsx), lines 107 and 215–216.

**Trigger and cause.** Onboarding is incomplete, persisted sandbox/GitHub policies exist, no recovered onboarding draft exists, and the application snapshot arrives after the component mounts. `repositoryPolicies` seeds state only in the initializer. The later source effect handles machine configurations, uses the legacy `source.githubPolicies` input, and returns early when machine configurations match. Production supplies that legacy input as an empty list; the actual policies arrive through the separate prop.

**Observed.** The real component mounted with placeholder machines and no policies, then received the same authoritative machine configuration and an existing nonempty `dev` policy. Clicking GitHub Continue submitted `dev.repositories=[]` without a user policy edit.

**Native consequence.** Finish invokes the GitHub setup step in [production-source.ts](../app/SiloUI/src/desktop/production-source.ts), lines 1341–1346. Connected submission sends each draft policy at lines 1314–1317. Native [apply_patches](../app/SiloUI/src-tauri/engine/src/github.rs), lines 2974–2985, replaces existing policy objects. The empty draft therefore clears configured repository access when saved. This is more than a transient display discrepancy.

**Correction.** Reconcile untouched policy fields when the first authoritative policies arrive, independently of whether machines changed. Track authoritative initialization and user/restored edits separately so loading cannot overwrite deliberate choices.

**Acceptance.** Loading policies before the first render and loading them later must produce identical submissions for untouched policies. Include equal and changed machine lists, explicitly edited fields, and recovered drafts. The current diagnostic establishes the no-recovered-draft/equal-machine case, not all recovery histories.

### R-32 Onboarding changes an existing token policy to OAuth

**P2.** Locations: [onboarding-app.tsx](../app/SiloUI/src/features/onboarding/onboarding-app.tsx), lines 160, 351–364, and 440–457; [onboarding-draft.ts](../app/SiloUI/src/features/onboarding/model/onboarding-draft.ts), line 45; [onboarding-source.ts](../app/SiloUI/src/features/onboarding/model/onboarding-source.ts), lines 81–94; [github.rs](../app/SiloUI/src-tauri/engine/src/github.rs), lines 2887–2896 and 2974–2985.

**Trigger and cause.** Resume onboarding with an existing `authenticationMethod: "token"` policy while OAuth is also connected, and initialize from the persisted policies rather than a recovered draft. Initialization copies repository mode and write permission but drops authentication method. The recovery schema and completion policy contract omit it, and the editor receives no connected-token state.

**Observed.** The real component displayed OAuth selected, disabled Use token, and submitted the policy without `authenticationMethod`. Native validation explicitly interprets omission as OAuth. Connected OAuth permits that method change, and native save replaces the previous policy.

**Consequence.** Continuing setup changes the credential source and its repository-access behavior without the user choosing that change. This claim does not cover disconnected token-only onboarding, where the connected GitHub save is skipped. It is separate from R-31: it reproduces even when the policy is present before the first render.

**Correction.** Carry the authentication method through initialization, draft recovery, completion serialization, and the editor's available-token state. Keep one policy vocabulary across the normal GitHub page and onboarding; do not use omission to represent preservation when the native contract interprets it as a default.

**Acceptance.** An unchanged token-selected policy must remain token-selected through Continue, Finish, draft persistence, and reload when OAuth is also connected. Inspect the native saved policy as well as the request. Test a deliberate switch separately.

### R-33 Remote notifications route to the wrong same-named sandbox

**P2.** Locations: [remote.rs](../app/SiloUI/src-tauri/engine/src/remote.rs), lines 1307 and 1328–1349; [notifications.rs](../app/SiloUI/host/src/notifications.rs), lines 75–88; [macOS integration](../app/SiloUI/host/src/system_integrations/macos.rs), line 281; [application-app.tsx](../app/SiloUI/src/features/application/application-app.tsx), lines 236–238 and 251–254.

**Trigger and cause.** A local sandbox and a remote sandbox share a display name, such as `dev`. The remote action completion/failure notification retains the raw VM ID and name but drops its computer identity. `Notice::route` sends the name. The frontend resolves the first matching ID, name, or target.

**Observed.** A diagnostic routed the real application through its established preview fixture with local `dev` and Office `dev`. The native-style name-only route selected local detail. Clicking Terminal invoked `openTerminal("dev")`. The qualified remote route correctly invoked `openTerminal("silo-remote:office:remote-dev-uuid")`. Native macOS notification delivery was source-traced, not clicked in a live desktop session.

**Consequence.** A notification about one computer opens another computer's sandbox, and subsequent actions target that selected sandbox. This is distinct from R-06's Network error-row mismatch and the first report's notice-wording opportunity; the navigation error has an executed action consequence.

**Correction.** Carry a computer-qualified target independently of display name through native notification routes. Use qualified identity consistently for grouping and clearing too. The frontend already understands the qualified target.

**Acceptance.** Remote completion and failure routes must select the correct owner despite duplicate names across local and multiple remote computers. Verify a subsequent action's target, not only the visible heading. Preserve local routes and rename behavior.

### R-34 Unrelated runtime data can block a small export

**P2.** Location: [backup.rs](../app/SiloUI/src-tauri/engine/src/backup.rs), lines 817–870.

**Trigger and cause.** Export an existing checkpoint when disk space is limited and the native store contains unrelated snapshots or cache data. Admission measures the entire `snapshots` and `cache` trees, multiplies those bytes by the selected sandbox count, and charges the resulting estimate to staging and destination volumes. The comment deliberately calls this conservative, but the estimate is a hard rejection without a narrower fallback.

The exact upstream [archive selection](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/sdk/rust/lib/backend/local/snapshot/archive.rs#L265-L361) includes the selected head, its parent chain, and selected image dependencies. It does not include unrelated snapshot groups or `cache/tmp` downloads.

**Observed.** The extracted production estimator admitted a 23-byte selected fixture with `1,078,984,704` available bytes, the reserve plus 5 MiB. Adding only a 4 MiB `cache/tmp/unrelated-download` changed the result to `InsufficientSpace`. The selected data and injected free-space value were unchanged. Both the requested and available space were rounded to “1.0 GiB” in the resulting error. This proves an admission dependency on excluded data; no native archive was produced in the diagnostic.

**Consequence.** Unrelated downloads/checkpoints prevent exporting recoverable data, particularly on a nearly full computer. Selecting multiple sandboxes repeatedly charges unrelated bytes. A later check against actual archive size cannot help an operation rejected before archive creation. Walking unrelated trees also adds avoidable work, although this pass did not benchmark that latency.

**Correction.** Estimate the actual selected dependency closure, preserving physical-allocation accounting, reserve, and overlapping capture/staging/destination requirements. Prefer an upstream archive-planning facility if available; otherwise align the selection logic with the pinned upstream contract and seek a reusable upstream API. Do not remove the safety reserve or assume compressed output is free.

**Acceptance.** Growing unrelated groups and cache temporary files must not change admission when the selected closure and available space are fixed. Shared ancestors must be charged according to how the chosen export format writes them. Compare estimates with real archive output before release, including sparse disks and same-volume staging. Report sufficient precision to explain a rejection.

### R-35 Failed Debian restart leaves a running app without its instance claim

**P2.** Locations: [updates.rs](../app/SiloUI/src-tauri/engine/src/updates.rs), lines 608–622 and 681–699; [updates/debian.rs](../app/SiloUI/src-tauri/engine/src/updates/debian.rs), lines 180–189; [single_instance.rs](../app/SiloUI/src-tauri/src/single_instance.rs), lines 30–35.

**Trigger and cause.** A Debian installation succeeds but replacing the process with `/usr/bin/silo-ui` fails. Before `exec`, Silo explicitly destroys the single-instance plugin claim. The failure callback then cancels shutdown and restores sandboxes without reacquiring that claim. The UI reports that relaunch is needed, but the surviving process resumes normal operation.

The exact cached `tauri-plugin-single-instance 2.4.5` Linux source was inspected: `destroy` releases the session D-Bus name and does not reacquire it. The [upstream implementation](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/single-instance/src/platform_impl/linux.rs) exposes the same release mechanism. Rust's [exec contract](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.exec) confirms that return means replacement failed; the old process still exists. The pinned cached implementation, rather than the moving upstream branch, is the version evidence here.

**Consequence and limit.** Another runnable same-channel copy can acquire the claim while the old app is still active. The remote-control lease does not prevent full startup: [remote.rs](../app/SiloUI/src-tauri/engine/src/remote.rs), lines 1551–1567, records its failure and continues. Scoped runtime-worker locks protect individual commands, but they do not restore application-lifetime ownership or serialize every settings/credential state writer. Competing state writers are therefore permitted; data corruption was not demonstrated. No package installation, failed reexec, or second live Linux instance was executed in this review.

**Correction.** Restore verified instance ownership before reopening mutation admission after failed reexec. If reacquisition cannot be guaranteed, keep mutations blocked and provide a safe exit/relaunch path. Prefer a supported plugin lifecycle facility or upstream extension over an additional custom ownership subsystem.

**Acceptance.** In an isolated Linux session, inject an `exec` failure and attempt a second instance. The first process may resume writes only while it owns the instance claim. If ownership is lost, neither recovery nor ordinary UI commands may resume mutations under a false single-owner assumption. Keep the successful replacement path covered too.

### R-36 Editor repair discards file failures and reports success

**P3.** Locations: [editor.rs](../app/SiloUI/src-tauri/engine/src/editor.rs), lines 816–845, 890–903, and 919–929.

**Trigger and cause.** An editor transport configuration cannot be read or replaced during migration or launch-time repair. `rewrite_configs` returns no result, skips directory/file errors, and discards `write_private` failures. The migration path can install a new Include and return `Repair::Done` even though its included files retain the previous runtime paths.

**Observed.** Extracted production read/write, rewrite, Include, repoint, and repair functions ran against temporary homes. A directory set to mode `0500` caused an independently checked write failure. `refresh_configs` returned normally with the old ProxyCommand. The full repair helper returned Done and prepended the new Include while retaining the old ProxyCommand, IdentityFile, and UserKnownHostsFile. Runtime-path and transport-string collaborators were synthetic; no editor or SSH process was launched.

**Consequence.** Automatic reconnect retains obsolete paths and can fail after the previous runtime backup disappears, without the intended repair warning. This does not prove data loss or successful access to an old VM. Reopening through Silo after storage becomes writable provides a practical recovery path, which keeps this P3.

**Correction.** Return per-file repair failures and surface an incomplete repair through `Repair::Failed`. Preserve originals when replacement fails. Adding an Include is not proof that every referenced file was repaired.

**Acceptance.** A failed rewrite must produce an explicit incomplete result. After restoring writability, retry must update all three directives and then return Done. Cover unreadable files and partial batches without treating an absent optional directory as a failure unnecessarily.

### R-37 Linux browser selection accepts a path its launcher cannot use

**P3.** Locations: [applications.rs](../app/SiloUI/host/src/applications.rs), lines 224–243; [applications/linux.rs](../app/SiloUI/host/src/applications/linux.rs), lines 87–99, 203–223, and 299–322.

**Trigger and cause.** Select an installed browser executable through Choose application, for example an executable under `/usr/bin`, instead of its desktop-entry file. Generic `application_at` accepts executable files and returns a valid application. Explicit browser launch always calls `desktop_at`, which requires a parsable GIO desktop entry. An executable is therefore accepted by selection and rejected by use.

**Consequence.** Browser-dependent actions, including GitHub authorization and release-page opening, report the selected application unavailable after Settings accepted it. Choosing a valid `.desktop` file or the default browser is a workaround. This is source-confirmed; no Linux picker or launch was exercised.

**Correction.** Either support direct executable browser launches with explicit argv and the existing sanitized environment, or reject executable-only selections specifically for Browser with a clear desktop-entry requirement. Preserve GIO parsing for desktop entries; do not interpret their Exec fields through a shell.

**Acceptance.** Every accepted browser selection must reach a successful fake launch through the corresponding launcher. Cover a desktop entry, an executable, a removed target, and the default browser. Validate the chosen behavior in the supported Linux desktop environment.

## Historical compilation failure at an intermediate revision

### R-38 Native tests failed to compile at ac83d94

**P2 at the intermediate revision; excluded from the final finding count.** All locations in this entry refer to `ac83d94`: `app/SiloUI/src-tauri/src/computer_use/tests.rs`, lines 1913 and 1919; `app/SiloUI/src-tauri/src/runtime/checkpoints.rs`, line 6083; and the subsequently removed `app/SiloUI/src-tauri/src/test_support/computer_use_live/approval.rs`, line 48. The corresponding `computer_use.rs` APIs were `after_boot` at 835 and `apply_approval_with` at 1295.

**Trigger and observed result.** Running the normal native test target at `ac83d94` failed before any test executed. Two test calls referred to nonexistent `boot`; two passed `&ProcessRunner` to a function requiring `Arc<dyn RuntimeRunner + Send + Sync>`. Both the isolated copy and a direct checkout `cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked --no-run` produced these four errors. The later checkout movement to `a1197ae` removed all four offending calls; do not present this as a current compile failure.

**Consequence at that revision.** Ordinary native tests and focused Cargo test filters could not run. The offending live tests were ignored for execution but still typechecked, so their opt-in status did not isolate this failure. The earlier 1,302 passing native tests belong to an earlier revision and do not contradict this result. This finding does not establish that the non-test application build failed.

**Requirement before reintroducing those tests.** Update them to the current apply/boot interfaces and preserve isolated operation gates where needed. Compile the ordinary test target after merging API changes, including ignored tests; execution filters do not replace compilation.

**Acceptance for a future merge.** The exact no-run command must compile, followed by the relevant computer-use and checkpoint unit tests. Live tests remain opt-in and need separate disposable-VM qualification. Retain the failed logs so a later successful run does not erase the evidence of this integration failure.

## Follow-up on the first pass

### R-21 fails on the old revision and passes at the final cutoff

The first report classified the approval-change-back race as source-confirmed and left its drafted regression unexecuted. This pass appended that unchanged regression to an isolated copy of the actual native test harness at `f9c1260`. Production Rust source was unchanged. It seeded applied Ask, stalled an Auto apply, selected Ask again, released Auto, and checked the final guest setting.

The test **failed as predicted**: actual `Some("auto")`, expected `Some("ask")`. The result was 0 passed, 1 failed, and 1,322 filtered out. This establishes the old defect deterministically without a real guest.

The final cutoff includes a subsequent convergence loop in [computer_use.rs](../app/SiloUI/src-tauri/engine/src/computer_use.rs), lines 880–918, plus an unfinished-attempt marker. The loop reads the latest desired mode after an attempt and applies again within the turn. Running the same unchanged regression against an isolated copy of the final source **passed: 1 passed, 0 failed, 1,328 filtered out**. Production Rust was unchanged and no test call-site repairs were needed at this revision. **R-21's demonstrated sequence is fixed at the final cutoff.** This establishes the deterministic policy behavior, not live guest qualification.

The intermediate attempt at `ac83d94` could not execute because of R-38; those compiler errors were also reproduced directly in the checkout. The subsequent checkout movement removed the unrelated broken test calls and allowed the final regression to run. R-22 also has a targeted warning correction in [computer-use-panel.tsx](../app/SiloUI/src/desktop/computer-use-panel.tsx), lines 117–134; this pass source-checked it but did not rerun the earlier compatibility fixtures.

The other earlier findings were not comprehensively requalified against the concurrent merges. Their first-report status remains revision-scoped. This report does not turn the combined historical ledgers into a claim that every item remains open today.

## Verification and reproducibility

Focused frontend diagnostics used Node `24.11.1`. Rust diagnostics and native compilation used `1.94.0`. Native compilation used only the explicit synthetic GitHub configuration permitted by the release guide; no local build secrets were printed. No distributed executable was produced.

| Check | Result and boundary |
| --- | --- |
| Real React diagnostics | **4/4 passed**, reproducing R-30, R-31, R-32, and R-33 with synthetic source/actions |
| Log/SSH repair Rust harness | Completed all assertions: split console PEM leak, split exec PEM leak, complete-block redaction control, failed transport rewrite, and false successful migration repair |
| Current restore-argument harness | Two cases, disk/full: both omit `--env`, built-in GH_TOKEN, and `--disk-only` |
| Current export-space harness | One before/after scenario: admission changes from success to insufficient space solely after adding unrelated cache data |
| R-21 at the first-report revision | Actual native harness regression **failed**, leaving Auto instead of Ask |
| R-21 at intermediate `ac83d94` | Could not execute because the native test target did not compile |
| Direct `ac83d94` native test compilation | **Failed with four compiler errors**, historical R-38; no tests executed |
| R-21 at final `a1197ae` | Actual native harness regression **passed**, converging on Ask |
| Direct final `a1197ae` native test compilation | **Passed** with `--no-run`; test compilation only, no full-suite execution |
| Documentation | Relative local targets and whitespace checked after writing this report |

Commands run from the repository root:

```sh
# Real React component diagnostics.
PATH=/Users/polarzero/.nvm/versions/node/v24.11.1/bin:$PATH \
  app/SiloUI/node_modules/.bin/vitest run \
  --config app/SiloUI/src-tauri/target/verification/pass2-frontend-2026-10-02/vitest.config.ts \
  --maxWorkers=1

# Extracted production log and editor repair functions.
cargo +1.94.0 run --offline --quiet \
  --manifest-path app/SiloUI/src-tauri/target/verification/pass2-security-2026-10-02/Cargo.toml

# Direct current-checkout compilation, using synthetic test-only configuration.
SILO_GITHUB_APP_SLUG=silo-ci-test \
SILO_GITHUB_CLIENT_ID=test-client \
SILO_GITHUB_CLIENT_SECRET=test-secret \
  cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked --no-run

# Same native regression in the isolated source copy, run at both revisions.
# CARGO_TARGET_DIR reuses the main dependency cache; packaging changes stay in the copy.
CARGO_TARGET_DIR="$PWD/app/SiloUI/src-tauri/target" \
SILO_GITHUB_APP_SLUG=silo-ci-test \
SILO_GITHUB_CLIENT_ID=test-client \
SILO_GITHUB_CLIENT_SECRET=test-secret \
  cargo +1.94.0 test --locked \
  --manifest-path app/SiloUI/src-tauri/target/verification/pass2-root-2026-10-02/isolated-repo/app/SiloUI/src-tauri/Cargo.toml \
  changing_back_to_previous_mode_while_another_apply_runs_eventually_applies_latest_choice
```

For the data diagnostics, the extraction scripts generate standalone Rust files from the current functions. Each was compiled with `rustc +1.94.0` and executed against temporary/synthetic inputs. The local evidence directories preserve the exact scripts, source, outputs, and cutoff source hashes:

| Evidence directory under `app/SiloUI/src-tauri/target/verification/` | Contents |
| --- | --- |
| `pass2-frontend-2026-10-02/` | `github-retry.test.tsx`, `onboarding-policy.test.tsx`, `remote-notice-route.test.tsx`, configuration, and combined `results-3.log` |
| `pass2-security-2026-10-02/` | Extracted source, fixture boundary README, `results-final.txt`; earlier setup failure retained |
| `pass2-data-2026-10-02/cutoff-ac83d94/` | Restore/space extraction scripts and source, `restore-environment-result.txt`, `export-space-result.txt`, and `source-sha256.txt` |
| `pass2-data-2026-10-02/cutoff-a1197ae/` | Final source hashes and byte-for-byte production-block comparison with the executed diagnostics |
| `pass2-root-2026-10-02/` | Old failing R-21 output and source hash confirmation; intermediate compilation failures; final passing `r21-cutoff-a1197ae-final.log`; per-revision direct-checkout compilation logs and provenance README |

The R-21 test copies disabled sidecar/resource/framework staging only in their scratch Tauri configuration because generated package inputs were absent. They used the repository's actual Rust harness and unchanged production Rust. Initial missing-staging failures were retained before test execution. At the intermediate revision, compilation reached the four source errors also reproduced directly in the checkout. Those errors are therefore not attributed to packaging adjustments or the appended regression. The final source compiled and passed the unchanged appended test with the same packaging-only adjustment.

The first frontend fixture had a configuration-import setup failure; the security fixture first used an unsupported local OpenSSL key-generation algorithm. Those outputs were retained, then corrected fixture setup produced the reported results with a disposable RSA key. Neither initial setup failure is a product finding.

No full frontend, release, website, guest-script, or native execution suite was rerun in this pass. The first report records those earlier results. No Linux package installation, native notification delivery, two-computer workflow, live checkpoint/RAM behavior, or dependency-advisory scan was performed. A passing component fixture does not qualify those integrations.

## Engineering implications and next work

The additional findings share three concrete causes. Policy values lose information while moving through UI drafts and native requests; R-30 through R-32 need preservation of the user's complete intent. Native integration paths reconstruct only part of an existing contract; R-28, R-29, R-33, and R-35 need explicit restore mode, environment, target, and ownership. Success is inferred from partial progress; R-36 needs write results, while historical R-38 shows why ignored execution is not the same as compiled coverage.

The existing tools already provide much of the required behavior: MicroSandbox has disk-only and environment restore arguments, the frontend accepts qualified remote targets, GIO owns desktop-entry launching, and the native gate provides isolated test seams. Use these boundaries before adding abstractions. Extract shared policy or restore construction only where both create/restore or onboarding/settings callers prove the common contract. Correctness comes before deduplicating code that merely looks similar.

Start with **R-27: make PEM redaction survive record boundaries and add the export regression**. Then repair the policy and restore contract defects with their specific reproductions. Keep live acceptance separate from deterministic tests, and update the dated ledger only after each acceptance criterion is met.
