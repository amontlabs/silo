# Silo documentation

The application lives in [`app/SiloUI`](../app/SiloUI): React in `src/`, Rust and
native integration in `src-tauri/`, and build tooling in `scripts/`. Start with
the [repository README](../README.md) to install and use Silo, or the
[source-build guide](SiloUI-BUILD-FROM-SOURCE.md) to develop it with your own
GitHub App.

This index has three parts: [current implementation](#current-implementation-and-operations),
[current research and evidence](#current-research-and-design-evidence), and
[historical records](#historical-records) that no longer describe the app or
its plans. Paths under `app/SiloUI/src-tauri/target/verification/` name
untracked evidence kept on the machine that ran the check; they are not in Git.

## Current implementation and operations

These documents describe the Tauri app. Dated verification results establish
coverage for that run, not a guarantee that every platform or live VM workflow
has been exercised.

| Area | Documents |
| --- | --- |
| Vocabulary | [Silo vocabulary](SiloUI-VOCABULARY.md): the terms (computer, device, Connections, workspace, sandboxed) and the identifiers that keep older names |
| Build and release | [Build from source](SiloUI-BUILD-FROM-SOURCE.md), [build channels (production and Dev)](SiloUI-BUILD-CHANNELS.md), [release workflow and CI](SiloUI-RELEASES.md), [Linux system updates](SiloUI-LINUX-UPDATES.md), [distribution acceptance](SiloUI-DISTRIBUTION-PLAN.md), [release history](releases/) |
| Runtime | [Packaging](SiloUI-RUNTIME-PACKAGING.md), [guest images](SiloUI-GUEST-IMAGES.md) (downloaded on first use), [SSH agent TLS regression](SiloUI-ZCODE-TLS-INVESTIGATION.md), and the [current runtime pins](../app/SiloUI/runtime-inputs.json). The dated [runtime and backup decision log](SiloUI-RUNTIME-BACKUP-FINDINGS.md) was validated against MicroSandbox 0.6.17 unless a section says otherwise; its Backup page became per-sandbox Export and Import. |
| Checkpoints | [Checkpoint implementation plan and qualification](SiloUI-CHECKPOINTS-PLAN.md), [snapshot lineage groups](research/silo-snapshot-lineage-groups-2026-09-26.md) |
| Native tests | [Rust test support and live-test boundaries](SiloUI-RUST-TEST-SUPPORT.md), [skipped and ignored test audit, 2026-10-02](SiloUI-TEST-SKIP-AUDIT-2026-10-02.md) |
| Platform verification | [Linux](SiloUI-LINUX-VERIFICATION.md), [Linux verification session, 2026-09-30](research/linux-verification-2026-09-30.md), [Linux acceptance, 2026-09-25](research/silo-linux-acceptance-2026-09-25.md), [macOS VM library loading](SiloUI-LIBRARY-CONSTRAINTS.md), [dependencies, export and import testing](SiloUI-DEPENDENCIES-BACKUP-TESTING.md) |
| Connections | [Connections and Quit behavior](SiloUI-CONNECTIONS.md), [managed SSH access](SiloUI-MANAGED-SSH.md) |
| GitHub and secrets | [GitHub implementation](SiloUI-GITHUB-IMPLEMENTATION.md), [personal GitHub tokens](SiloUI-GITHUB-PERSONAL-TOKENS.md), [secrets](SiloUI-SECRETS.md), [secret placeholders in agent requests](SiloUI-AGENT-PLACEHOLDER-STALL.md), [0.4.4 authentication investigation](SiloUI-GITHUB-044-AUTH-INVESTIGATION.md) |
| Computer tools | [Working account](SiloUI-WORKING-ACCOUNT.md), [computer migration](SiloUI-WORKING-ACCOUNT-MIGRATION.md), [Linux desktop](SiloUI-DESKTOP.md), [macOS computers](SiloUI-MACOS-COMPUTERS.md), [computer-use plan](SiloUI-COMPUTER-USE-PLAN.md), [Files](SiloUI-FILES.md), [network](SiloUI-NETWORK-PLAN.md), [terminal handoff](SiloUI-TERMINAL-HANDOFF.md), [editor and browser handoff](SiloUI-EDITOR-HANDOFF.md) |
| Logs | [Retained history, search and export](SiloUI-LOGS.md), [computer failure reporting](SiloUI-FAILURE-REPORTING.md) |
| Storage | [Workspace disk reclamation policy and verification](SiloUI-STORAGE-RECLAMATION.md), [disk discard regression](SiloUI-STORAGE-DISCARD-RESEARCH.md) |
| Desktop | [Built-in desktop and computer use plan](SiloUI-COMPUTER-USE-PLAN.md): approved 2026-10-01, replaces optional desktops and Luda for new computers. [Pinned ChatGPT app manager](SiloUI-CHATGPT-APP.md): device download, verification and extraction. [Background preparation](SiloUI-PREPARATION.md): launch-time computer image import and LCU download. [Detached desktop implementation record](SiloUI-DETACHED-DESKTOP-IMPLEMENTATION-PLAN.md): September Selkies rollout and its original design evidence. Existing Kasm guests remain supported until explicit update. [Viewer integration plan](SiloUI-VIEWER-INTEGRATION-PLAN.md): proposed clipboard, file transfer, audio and window-sized screen work. [Viewer direction](SiloUI-DESKTOP-VIEWER-DIRECTION.md) and [experience research](SiloUI-DESKTOP-EXPERIENCE-RESEARCH.md) record rationale and limits. |
| Desktop behavior | [Settings](SiloUI-SETTINGS.md), [native menus](SiloUI-NATIVE-MENUS.md), [status panel](SiloUI-STATUS-PANEL.md), [notifications](SiloUI-NOTIFICATIONS.md), [macOS title-bar alignment](SiloUI-TITLEBAR-ALIGNMENT.md), [glass material](SiloUI-GLASS-STUDY.md) |

[Bundled application help](../app/SiloUI/docs/silo-help.html) describes the user-facing
controls and workflows.

## Current research and design evidence

Research records the inputs to a decision. Follow the implementation documents
above for current behavior and build commands.

### Review remediation

- [Second code review pass, 2026-10-02](SiloUI-CODE-REVIEW-PASS-2-2026-10-02.md): 11 additional findings at `a1197ae` from three specialist subagents and integration review, covering log redaction, GitHub policy preservation, checkpoint restore contracts, and native routing/repair; verifies the earlier approval-race fix and separately records a compilation failure on an intermediate revision.
- [Comprehensive code review, 2026-10-02](SiloUI-CODE-REVIEW-2026-10-02.md): consolidates 26 findings and 11 improvement opportunities, including four additional settings, migration, remote-checkpoint, and status-subscription defects; separates fresh verification from prior evidence and defines regression and live-qualification criteria.
- [Background preparation flows, 2026-10-03](research/background-preparation-flows-2026-10-03.md): VM image import, LCU archive, creation boot, operation queue and start flow, with verified code paths and open questions. The resulting design is in [background preparation](SiloUI-PREPARATION.md).
- [LCU registration for harnesses installed later, 2026-10-03](research/lcu-agent-preregistration-2026-10-03.md): what LCU 0.8.7 does for absent harnesses, what each registers, existing multi-client installers, and the `--allow-missing` plus `--reconcile` design.
- [Independent current-checkout review, 2026-10-02](research/codebase-review-current-2026-10-02.md): 22 findings and 10 improvement opportunities, including six additional defects and a follow-up on the concurrent computer-use merge, independently checked reproductions, verification results, and explicit qualification gaps.
- [Code review, 2026-10-02](research/codebase-review-2026-10-02.md): 16 current findings and 9 improvement opportunities, with deterministic reproductions, prioritized fixes, local check results, and live-verification limits.
- [Workflow micro-review fixes](research/micro-reviews/workflows-fixes.md): privileged QEMU image pinning, fail-closed publication checks, pending guest-publication retention, and verification limits.
- [Desktop contract fix-loop findings](research/micro-reviews/fe-desktop-contracts-fixes.md): status-event ordering, saved CPU bounds, notice disposal, and the pinned native menu checks.
- [Release dry run, 2026-09-30](research/release-dry-run-2026-09-30.md): non-publishing all-target release verification for A-01, A-04, A-08 and A-09.
- [Review remediation plan](SiloUI-REVIEW-REMEDIATION-PLAN.md): ledger of the 2026-09-29 review findings with landed-fix statuses and verification, plus the original work packages, phases, merge-queue orchestration and live verification sessions.
- [Review remediation design notes](SiloUI-REVIEW-DESIGN-NOTES.md): Phase 0 decision records (options checked against upstream tools, recommended decision, implementation outline, owner questions) for the review items marked design.
- [Codebase review, 2026-09-29](research/codebase-review-2026-09-29.md): ranked findings from a read-only review of the whole app — owner decisions, release blockers, security, data loss, stuck states, performance, UX, CI, tests and code health.
- Follow-up review records, 2026-10-02: [release tooling](SiloUI-CODE-REVIEW-PASS-3-RELEASE-2026-10-02.md), [security](SiloUI-CODE-REVIEW-PASS-3-SECURITY-2026-10-02.md), [guest bridge](SiloUI-CODE-REVIEW-PASS-3-GUEST-BRIDGE-2026-10-02.md), and [computer use](SiloUI-CODE-REVIEW-PASS-3-COMPUTER-USE-2026-10-02.md). Findings and verification describe the recorded commits; compare HEAD before reopening an item.
- [Micro-review index and finding counts](research/micro-reviews/README.md): original audits and fix-loop findings, including [status, storage and updates](research/micro-reviews/fe-status-storage-updates-fixes.md). Counts include fixed findings and overlap across reports; each record states its scope and verification limits.
- [Changeset audit, 2026-10-02](research/micro-reviews/changesets.md): metadata, user-facing wording, duplicate corrections, and commit coverage since `f9421925`.

### Runtime, checkpoints and network

- [Native bridge contract audit](research/native-bridge-contract-2026-09-30.md): Rust-emitted state fixtures, typed error codes, remote compatibility, and verification limits.
- [Runtime failure contract](SiloUI-RUNTIME-ERRORS.md): summary, diagnostic and partial-change fields for setup, lifecycle and Activity failures.

- [Checkpoint and desktop direction](research/checkpoints-desktop-direction-2026-09-24.md): newer MicroSandbox snapshot/fork support, the upstream-upgrade alternative, Btrfs limits, desktop candidate fit, LCU boundaries and qualification requirements.
- [MicroSandbox live public ports](research/microsandbox-live-public-ports-0.7.2.md): pinned control and publisher source, Silo's loopback TCP contract, ingress and multi-tenant boundaries, and regression limits.
- [MicroSandbox removing a sandbox that never started](research/microsandbox-remove-created-2026-10-01.md): upstream state of the `Created` removal refusal and an unpublished issue and pull request draft for the `remove-created` patch.

### Desktop and agent computer use

- Detached desktop evidence: [viewer implementation](research/desktop-viewer-implementation-evidence-2026-09-27.md), [guest lifecycle](research/desktop-lifecycle-implementation-evidence-2026-09-27.md), [Selkies/Tauri compatibility](research/selkies-tauri-implementation-evidence-2026-09-27.md), [live viewer and LCU probe](research/desktop-viewer-probe-2026-09-27.md), [Selkies first-frame fix and source provenance](research/selkies-web-client-first-frame-fix.md), and [Linux platform, streaming, UX and native-display comparisons](research/linux-desktop-platform-options-2026-09-27.md), with its annexes on [native local delivery](research/linux-desktop-native-options-2026-09-27.md), [streaming engines](research/linux-desktop-streaming-options-2026-09-27.md) and [desktop UX](research/linux-desktop-ux-options-2026-09-27.md). The [LCU 0.4.0 native-desktop compatibility patch](research/lcu-linux-native-desktop-0.4.0.patch) records the upstream source change used by Silo's hash-guarded managed install. The dated viewer probe includes Ubuntu 24.04 ARM64 package qualification and its limits.
- [Desktop stream tuning, 2026-10-09](research/desktop-stream-tuning-2026-10-09.md): measured input latency and sharpness of the streamed desktop, the MicroSandbox metrics-sampler stall behind the 0.5 s input delays, device-pixel rendering with integer Xfce scaling, damage-gated encoding and the H.264 frame-size limit.
- [Native local display, 2026-10-09](research/desktop-native-display-2026-10-09.md): VMPal and Cua Spaces stacks, the libkrun display/GPU route with its upstream PRs and steamac patches, maintainer status and the checkpoint gate.
- [macOS guest engine, 2026-10-09](research/macos-guest-engine-2026-10-09.md): Lume, Tart and direct Virtualization.framework bindings compared, framework constraints probed on this device, and Apple's license terms for macOS virtual machines.
- [MicroSandbox native display](research/msb-native-display-2026-09-27.md): Omarchy demonstration, Silo stack mapping, distribution independence, native-viewer opportunity and checkpoint/upstream adoption constraints.
- [Independent desktop and viewer selection](SiloUI-DESKTOP-SELECTION.md): selection recommendation without agent-library constraints, evidence notation, weighted measurement rubric, candidate leaderboards and qualification protocol, with primary-source annexes on [agent APIs](research/desktop-selection-agents-2026-09-22.md), [desktops](research/desktop-selection-desktops-2026-09-22.md) and [browser delivery](research/desktop-selection-viewers-2026-09-22.md).
- [Desktop and streaming assessment](SiloUI-DESKTOP-STACK-ASSESSMENT.md): September 2026 comparison of desktop environments, viewer stacks, Luda compatibility, maintenance evidence and selection criteria.
- [Linux desktops for agents](SiloUI-LINUX-DESKTOP-RESEARCH.md): the original guest desktop proposal, agent compatibility, estimated costs and prototype acceptance.
- [Codex, E2B and Luda computer use](research/codex-e2b-luda-computer-use-2026-09-22.md): observed native Codex interface, public API distinction, simplicity hypothesis and controlled comparison.
- [Codex Linux engine probe](research/codex-linux-engine-probe-2026-09-22.md): official package distribution and a passing ARM64/X11 accessibility, input and screenshot test.
- Historical Luda agent evidence: [acceptance tests](SiloUI-LUDA-AGENT-TESTS.md), [initial skill evaluation](SiloUI-LUDA-SKILL-EVALUATION.md), [accepted skill benchmark](SiloUI-LUDA-SKILL-BENCHMARK.md), and [upstream handoff](SiloUI-LUDA-UPSTREAM-HANDOFF.md).
- Historical Luda upgrade verification: [0.3.1](SiloUI-LUDA-031-VERIFICATION.md), [0.3.2](SiloUI-LUDA-032-VERIFICATION.md), [0.3.3](SiloUI-LUDA-033-VERIFICATION.md) with its [selection-completion diagnosis](SiloUI-LUDA-033-DIAGNOSIS.md), and the last pinned [0.3.4](SiloUI-LUDA-034-VERIFICATION.md).

### GitHub, logging and tooling

- [Server-free GitHub research](SiloUI-GITHUB-SERVER-FREE-RESEARCH.md): primary sources and authorization constraints behind the native implementation.
- [GitHub client-secret release audit](SiloUI-OAUTH-RELEASE-AUDIT.md): public 0.1.1 package inspection, live App settings, and independent PKCE enforcement checks.
- [Logging and retention audit](SiloUI-LOGGING-AUDIT.md): current storage limits, log and activity presentation, and retention gaps.
- [Development and release optimization plan](SiloUI-WORKFLOW-OPTIMIZATION-PLAN.md): measured bottlenecks, ranked changes, and benchmark acceptance gates.
- [Workflow performance](SiloUI-WORKFLOW-PERFORMANCE.md): implementation, controlled measurements and hosted comparison.
- [Workflow measurement data](measurements/): recorded dependency-cache and cold, cached and warm workflow samples used by the performance documents.
- [Native compilation experiment](SiloUI-NATIVE-COMPILATION-EXPERIMENT.md): measured test-target reduction and the compiler-cache acceptance gate.
- [Frontend test performance](SiloUI-FRONTEND-TEST-PERFORMANCE.md): controlled environment-split measurements.
- [Frontend startup bundle baseline, 2026-10-02](research/frontend-startup-bundle-2026-10-02.md): O-03 production artifact sizes, heavy modules, and the evidence required before splitting.
- [Streaming build inputs, 2026-10-02](research/stream-build-inputs-2026-10-02.md): verified file staging, bounded downloads, synthetic memory measurements and regression coverage; full runtime preparation was not measured.
- [Application launch selection, 2026-10-02](research/application-launch-selection-2026-10-02.md): preserving Flatpak desktop-entry selectors and targeting the selected Ghostty bundle, with resolver and escaping tests rather than live application launches.
- [Editor folder identity, 2026-10-02](research/editor-folder-identity-2026-10-02.md): rejecting control characters that URI serialization would discard, with extracted-function regression evidence and no live editor qualification.
- [Editor environment wrapper follow-up, 2026-10-02](research/editor-env-directory-2026-10-02.md): preserving `env` working-directory operands when resolving and launching a supported Linux editor, with a temporary CLI regression and no GUI or live VM.
- Editor follow-ups, 2026-10-02: [literal directories in SSH Includes](research/editor-ssh-include-2026-10-02.md) checks glob escaping with the system OpenSSH parser; [percent signs in Linux desktop entries](research/editor-desktop-percent-2026-10-02.md) checks field-code decoding with a temporary CLI. Neither qualifies a live editor connection.
- [Guest image size experiment](SiloUI-GUEST-IMAGE-SIZE.md): measured image-size tradeoffs.
- [Codex skills and context audit](CODEX-CONTEXT-AUDIT-2026-09-14.md): agent instruction and skill-trigger recommendations for working on this repository.
- [Jev for natural-language commands](SiloUI-JEV-RESEARCH.md): primary-source findings, command-palette fit, limitations and proposed evaluation.

### Product films and website

- [README and website value review, 2026-09-29](research/public-docs-review-2026-09-29.md): reader decision gaps, broken GitHub explanation link, agent quickstart priority, and proposed observed-user validation.
- [Release film](SiloUI-RELEASE-FILM.md): 59-second storyboard, product-claim sources, fixture boundaries, and rendering commands.
- [Demo script](SiloUI-DEMO-SCRIPT.md): current `SiloDemo` cut and production-component boundaries, with [editing research](SiloUI-DEMO-EDITING-RESEARCH.md).
- [Launch cut notes](SiloUI-LAUNCH-CUT-NOTES.md): feature evidence for the independent 54-second launch cut.
- [Landing page reference](SiloUI-LANDING-REFERENCES.md): approved Zed direction, product evidence, and website implementation.
- [Domain research](SiloUI-DOMAIN-RESEARCH-2026-09-19.md): domain availability, registrar pricing, and naming options checked on 2026-09-19.

Shared branding files live in [`assets/`](../assets/). Generated native bundles,
logs, and Rust outputs belong in the ignored `app/SiloUI/src-tauri/target/` tree;
frontend build output belongs in the ignored `app/SiloUI/dist/` tree.

## Historical records

These no longer describe current behavior or active plans. They are kept for
their evidence and reasoning.

### Removed Luda integration

Silo no longer installs Luda; [LCU](SiloUI-COMPUTER-USE-PLAN.md) replaces it. The
[agent desktop tools](SiloUI-LUDA.md) description, [acceptance tests](SiloUI-LUDA-AGENT-TESTS.md),
[skill evaluation](SiloUI-LUDA-SKILL-EVALUATION.md), [skill benchmark](SiloUI-LUDA-SKILL-BENCHMARK.md),
[upstream handoff](SiloUI-LUDA-UPSTREAM-HANDOFF.md) and upgrade verification
([0.3.1](SiloUI-LUDA-031-VERIFICATION.md), [0.3.2](SiloUI-LUDA-032-VERIFICATION.md),
[0.3.3](SiloUI-LUDA-033-VERIFICATION.md), [0.3.3 diagnosis](SiloUI-LUDA-033-DIAGNOSIS.md),
[0.3.4](SiloUI-LUDA-034-VERIFICATION.md)) are historical evidence.

### Archived plans

Superseded plans live in [`archive/`](archive/):

- [Optional Kasm desktop plan](archive/SiloUI-DESKTOP-IMPLEMENTATION-PLAN.md): the original Kasm rollout; the detached desktop plan above replaced it.
- [Luda integration plan](archive/SiloUI-LUDA-IMPLEMENTATION-PLAN.md): the original pinned-installer research and verification gates; [agent desktop tools](SiloUI-LUDA.md) documents the removed implementation.
- [E2B replacement plan](archive/SiloUI-E2B-REPLACEMENT-PLAN.md): the proposed breaking replacement of Silo's backend, superseded on 2026-09-25 by MicroSandbox checkpoints and forks.
- [E2B PoC investigation and completion handoff](archive/SiloUI-E2B-QUALIFICATION-HANDOFF.md): the 2026-09-23 brief for the E2B PoC, with its failure reproductions, evidence corrections and qualification gates.

### E2B evaluation, 2026-09-22 to 2026-09-24

Silo evaluated replacing its backend with a local E2B deployment, then kept
MicroSandbox. The PoC in `experiments/e2b-local` was removed from `main`; it is
kept on the `archive/media-and-experiments` branch and at
[`c122a49`](https://github.com/amontlabs/silo/tree/c122a498da064f08321c29bff82a6e92056a917d/experiments/e2b-local).

- [E2B fit for Silo](research/e2b-fit-2026-09-22.md): agent desktop capabilities, snapshots, local hosting requirements and the proposed comparison workflow.
- [E2B adoption assessment](research/e2b-adoption-assessment-2026-09-24.md): feature inventory, established benefits and drawbacks, current-stack coverage limits, and the recommendation to refactor desktop packaging/viewing before replacing the backend.
- [Executed local E2B desktop PoC](research/e2b-local-poc-2026-09-22.md): working ARM64 nested desktops, human handoff, memory snapshots, host restart recovery and measured resource costs.
- [E2B + LCU qualification](research/e2b-lcu-qualification-2026-09-22.md): LCU, credentials, Git/LFS, SSH and native viewer results, with corrected limits on checkpoint/pause/restore attribution.
- [E2B provenance and incident evidence audit](research/e2b-provenance-audit-2026-09-23.md): read-only inventory of the PoC's sources, revisions and incident evidence.
- [E2B qualification work log](research/e2b-qualification-worklog-2026-09-23.md): scratch deployment, isolated SDK controls, changes made, and remaining runtime gates.
- [E2B qualification gate matrix](research/e2b-qualification-gates-2026-09-23.md): investigation status, execution verdicts, evidence, and exact blocked or unrun work.
- [E2B lifecycle source audit](research/e2b-lifecycle-source-audit-2026-09-23.md): D1/D2 call graphs and pause-upload shutdown boundary for the captured source.
- [E2B D1 post-capture reproduction](research/e2b-d1-post-capture-repro-2026-09-23.md): controlled source-built failure, independent state oracles, survivor inventory, and restore limit.
- [E2B D1 ResumeSandbox-boundary fault](research/e2b-d1-resume-allocation-boundary-2026-09-23.md): an unexecuted diagnostic patch at the resume allocation boundary.
- [E2B D3 artifact audit](research/e2b-d3-artifact-audit-2026-09-23.md): exact failed restore identity, catalog row, panic/upload timeline, and canonical file hashes.
- [E2B D3 controlled restarts](research/e2b-d3-controlled-restarts-2026-09-23.md): exact SDK control across orchestrator and clean host restart, including canonical readback and recovery limits.
- [E2B fresh desktop and synthetic Git qualification](research/e2b-fresh-desktop-qualification-2026-09-23.md): two scratch-candidate runs, current-grant restore, browser/native viewer observations, cleanup, and remaining limits.
- [E2B viewer input readiness](research/e2b-viewer-input-readiness-2026-09-23.md): native pointer/modifier diagnosis and two-viewer ownership evidence.
- [E2B upstream report readiness](research/e2b-d1-report-readiness-2026-09-23.md): D1 checkpoint attribution and exact reproduction threshold; see also [D2 report readiness](research/e2b-d2-report-readiness-2026-09-23.md), [D2 report review](research/e2b-d2-independent-review-2026-09-23.md), [D3 restore review](research/e2b-d3-report-readiness-2026-09-23.md), and [release-source mapping](research/e2b-release-source-mapping-2026-09-23.md).
- [E2B causal bounds, late session](research/e2b-causal-bounds-2026-09-23-late.md): D1 fault at the real `ResumeSandbox` call boundary, an actual kernel `fsync` EIO reproducing the exact historical pause error on the pinned release binary, D3 cache-topology finding plus an activity control, Gate A/R live results, a real-ENOMEM checkpoint reproduction on the release binary, the D2 physical-cause bound, and upstream issues e2b-dev/runtime#3658 and #3659.
- [E2B D3 root cause](research/e2b-d3-root-cause-2026-09-23.md): deterministic 3/3 reproduction of the 41919-descriptor restore panic from hash-verified stored artifacts, clean previous-generation control, capture-time corruption during the documented full-disk window; upstream issue e2b-dev/runtime#3659.
- [E2B real-provider qualification](research/e2b-real-provider-qualification-2026-09-24.md): nine-case GitHub matrix on a disposable private repo — Git/LFS round trips, REST/GraphQL, denied controls and credential-host accounting.
- [E2B viewer input root cause](research/e2b-viewer-input-root-cause-2026-09-24.md): layer-by-layer isolation proving the WKWebView viewer chain delivers modifier-correct input; native-automation failures are macOS synthetic-event trust, not viewer defects.
- [E2B two-computer and native-Linux qualification](research/e2b-gate-h-two-computer-2026-09-24.md): pinned Embed on bare-metal x86_64 with KVM, Mac-controlled lifecycle matrix, UFW deployment gap, teardown record.
- [E2B canonical control readback](research/e2b-canonical-readback-2026-09-23.md): exact SDK pause control, transitive file hashes, and why current readback does not prove restart durability.
- [E2B D2 error audit](research/e2b-d2-error-audit-2026-09-23.md): exact rootfs sync error, process/catalog timeline, and present artifact inventory.
- [E2B D2 rootfs sync reproduction](research/e2b-d2-rootfs-sync-repro-2026-09-23.md): controlled source-built pause failure, SDK recovery checks, and limits of the synthetic EIO.
- [E2B credential contract audit](research/e2b-credential-contract-2026-09-23.md): Silo grant policy at the time, PoC broker gaps, and synthetic receipt evidence.
- [E2B credential-tool comparison](research/e2b-credential-tools-2026-09-24.md): existing brokers and proxy engines, iron-proxy and Infisical Agent Vault qualification order, disqualified alternatives, and the reuse-before-building rule.
- [E2B access and viewer audit](research/e2b-access-viewer-audit-2026-09-23.md): transport, native editor, and viewer qualification gaps.

### Other historical records

- [Historical ext4 discard investigation](../artifacts/ext4-raw-image-root-cause.html): upstream MicroSandbox v0.6.8 reproduction and regression requirements; not current app validation.
- Branding studies: [logo system](../artifacts/silo-logo-system.html), [proportions](../artifacts/silo-proportion-study.html), [structure](../artifacts/silo-structure-study.html), and [top-down study](../artifacts/silo-top-down-study.html).
- Superseded film renders were removed from `main`; they remain on the `archive/media-and-experiments` branch and in the history of `artifacts/`.
