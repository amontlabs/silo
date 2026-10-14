# Silo development and release optimization plan

2026-09-14. Investigation by three GPT-6 Astra subagents plus the coordinating agent. This is a plan, not an implemented optimization or a new benchmark result. No builds or releases were triggered for this investigation.

> Historical plan. Linux host build and cache work is closed because Linux is
> no longer a supported host; Linux guest computers on Mac hosts remain supported.

## Decision

Optimize early failure detection, redundant compilation, and dependency compilation before tuning test execution. Keep ordinary development independent of release packaging. Preserve all release validation, platform, signature and provenance checks.

A universal speed guarantee is not supportable. Hosted scheduling, machine performance and cache availability vary. What we can make reliable is correctness on cache misses, early rejection of inconsistent inputs, unchanged test coverage, and a measured acceptance gate for each optimization. Do not add theoretical savings from different changes: each changes the critical path.

## Measured baseline

Primary evidence: [0.4.4 build](https://github.com/0xpolarzero/silo/actions/runs/34850376103), [0.4.2 build](https://github.com/0xpolarzero/silo/actions/runs/34838057001), and [failed 0.4.3 build](https://github.com/0xpolarzero/silo/actions/runs/34848722275). These are different commits/runs, not a controlled experiment.

The failed attempt and correction delayed the next launch by 16m05s. The successful 0.4.4 build-to-draft took 14m50s. Its platform jobs ran concurrently.

| Work | macOS ARM64 | Linux x64 | Linux ARM64 |
| --- | ---: | ---: | ---: |
| Restore and prepare warm runtime | 16s | 6s | 4s |
| Compile native tests | 3m26s | 2m25s | 2m15s |
| Execute native tests | 16s | 7s | 9s |
| Entire native-test step | 3m43s | 2m34s | 2m24s |
| Updater test step | 5s | 1s | 7s |
| Compile optimized application | 8m11s | 6m08s | 4m57s |
| Package bundles | 10s + 26s signing/repack | 1m25s | 1m51s |

Frontend tests ran concurrently in 3m08s and did not gate this release. The 538 runtime tests executed locally in 2.01s; the native suite executed locally in about 12s after compilation.

The prior release's macOS application compilation took 4m06s, versus 8m11s here. Linux x64 was 6m05s versus 6m08s, and ARM64 4m53s versus 4m57s. The macOS variation prevents assigning a trustworthy percentage from one run. Cold runtime preparation previously took 7–12 minutes; warm restoration already removes most of that cost.

## Implementation order

### 1. Add a no-build preflight and remove duplicated runtime inputs

**Outcome:** the checksum mistake fails before downloads, compilation or tag creation.

Create a tracked `app/SiloUI/runtime-inputs.json` for approved MicroSandbox source, patch, toolchain, features and platform-specific inputs. JS staging and the Rust verifier must consume the same reviewed inputs. Rust must embed the tracked inputs, not trust the generated runtime manifest it is supposed to verify. Keep the guest lock as its existing source of truth; migrate Git pins separately if that expands this change materially.

Add a dependency-free Node checker that validates schema, supported targets, fields, safe patch path and the actual patch digest. Verification must never silently update approved hashes. Invoke it:

- At the start of `scripts/prepare-microsandbox-runtime.mjs`, before probing/building/downloading.
- In `scripts/release.mjs` before creating or pushing a tag.
- In release `validate`, release-tooling/PR verification, and cache warming before matrix work.
- Through a local `npm run preflight` command that can run without installing npm dependencies.

Update cache keys and workflow path filters in the same change. Retain existing native provenance and tampered-artifact tests. If consolidation delays the guard, use a temporary strict named-constant comparison, then delete it when the shared manifest lands.

**Acceptance:** mutate the patch, one pin, target, source revision, feature set and schema; each must fail before a sentinel download/build/tag action runs. A valid clean checkout needs no Rust, credentials, generated files or network. Target under one second after Node startup; measure rather than assume it.

**Expected benefit:** eliminates this class of late failure. It does not save 16 minutes on every healthy release. Do not gate expensive builds on the full three-minute frontend suite: that would lengthen successful releases. Gate on the tiny preflight; retain frontend success as a publication requirement.

### 2. Remove the unnecessary normal debug application build

`src-tauri/tests/github_build.rs` has five tests and `github_permissions.rs` has one. Neither executes the application. Cargo nevertheless builds the normal binary when integration tests exist, in addition to the unit-test harness. Move these six tests into `cfg(test)` modules in the existing binary crate, adjusting paths and preserving the generated-permissions assertions. Keep the actual application target and release build.

**Acceptance:** compare before/after test inventories and results; all six behaviors must still run. Cargo timing reports must show the redundant normal debug binary build is gone. Test both relevant OS families. Do not replace integration tests that genuinely launch a binary in future.

**Expected benefit:** removes demonstrably unnecessary compilation/linking. Seconds saved remain unmeasured because Cargo overlaps work. This improves local native checks as well as CI. [Cargo target selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection) documents the automatic binary build.

### 3. Pilot native tests and packaging on separate runners

After remeasuring step 2, split macOS first:

```text
fast preflight
  ├─ frontend checks ───────────────────────────────┐
  ├─ minimum-macOS checks ──────────────────────────┤
  ├─ native tests + updater tests, per platform ────┤
  └─ release build + packaging + all verification ──┤
                                                   └─ draft → verified publication
```

The draft must require every branch. Run compilations on separate runners, not concurrently in the same target directory or competing on the same small runner. Native unit-test jobs can use explicit synthetic GitHub configuration and need no signing key. Packaging keeps the protected signing environment and every package check.

On the original 0.4.4 trace, a macOS-only split has a 2m18s wall-saving ceiling because Linux then becomes the slowest job. Splitting all platforms has a 3m48s ceiling, modeling roughly 11m02s to draft instead of 14m50s. These are schedule calculations, not forecasts: additional queues, setup and runner variation can erase savings. Moving the six tests or adding a compiler cache changes this calculation.

**Acceptance:** prove a deliberately failing native test blocks the draft even when packaging succeeds. Benchmark macOS pilot first. Extend to Linux only if its new critical-path delay justifies two extra runners. Peak concurrent jobs rise from five to six for the pilot, or eight for a full split. Track runner cost and queues as well as elapsed time.

### 4. Add a dependency-only compiler-cache experiment

Current caches contain downloaded Cargo sources, not compiled dependencies. This leaves two largely cold compiler profiles per platform. Prototype a pinned sccache local store with these boundaries:

- Only a trusted main-branch warmer writes shared cache archives. It has synthetic GitHub configuration and no signing credentials.
- Warm the exact test and Tauri release target/profile/features, including Tauri's `custom-protocol` feature. Cache compatibility must be measured against the actual installed CLI command.
- A compiler wrapper explicitly allows reviewed dependency packages; application crates/build scripts and unknown local packages bypass caching. Do not depend solely on incidental sccache limitations to protect application output.
- Save only the compiler-object cache. Never upload the Cargo target tree, application executables, build-script output or private logs.
- Release jobs restore only and use local read-only cache mode. No implicit remote uploads or post-job saves. Cache misses compile normally.
- Keys include the cache/tool versions, platform/toolchain and compilation inputs. Warm on dependency manifests/lock, vendor, feature/profile and toolchain changes. Preserve current runtime integrity checks and cold fallback.

**Acceptance:** sentinel tests prove real configuration is neither cached nor reused from synthetic builds; rotating synthetic configuration rebuilds the app; dependency hits remain possible; release jobs cannot write the cache. Test corruption/miss fallback and all package checks. Measure restore size/time, hit reasons, application link time, peak disk and warm-job cost. Do not claim benefit if transfers cost as much as avoided compilation.

**Expected benefit:** largest remaining potential compiler improvement, currently unmeasured. A useful experiment gate is at least 30% lower combined compilation time on repeated warm runs, without correctness or cache-boundary regressions. This is a proposed acceptance threshold, not an expected result. Recalculate scheduling after caching; never add the 3m48s schedule ceiling to a cache percentage.

The repository prohibits blanket target caching. The proposed scoped cache preserves that rule. Versioned references: [sccache Rust support](https://github.com/mozilla/sccache/blob/v0.12.0/docs/Rust.md), [configuration/read-only mode](https://github.com/mozilla/sccache/blob/v0.12.0/docs/Configuration.md), [Tauri CLI 2.11.4 build options](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-cli/src/interface/rust.rs), and [GitHub cache scope](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache). The cited sccache version is a verified candidate, not a claim that it is latest.

### 5. Make the everyday development loop explicit

| Change | Inner loop | Before completion |
| --- | --- | --- |
| UI behavior | Existing Vite preview and focused Vitest/watch | Relevant interaction regressions, typecheck/lint; broader UI suite once |
| Native behavior | Minimal reproduction and focused Cargo tests, reuse that worktree's build products | Relevant native suite; exact debug bundle only when native/package inspection is needed |
| Runtime patch | Patch applicability, preflight, focused upstream runtime regression | Runtime suite and intended platform verification |
| Release | Cheap preflight and prepared inputs | Full joined platform/package/publication gates |

Keep Cargo incremental builds enabled locally unless measuring a specific reason otherwise; CI's clean/incremental settings are not a local-development template. A filtered Cargo test still compiles the crate, so filtering alone cannot fix a cold native build. Run full verification once after the last relevant change, then rerun only affected checks unless a new failure or edit warrants more. Do not perform a full artifact-only release immediately followed by an identical signed release merely as a routine preflight.

Frontend evidence gives a narrower next target: `application-app.test.tsx` took 105.799s and `onboarding-app.test.tsx` 84.544s, about 70% of the suite's 273.80s aggregate test execution. All files currently use jsdom. Profile these two files first; preserve essential full-window journeys and move genuinely component-local assertions to their existing seams. Run pure non-DOM suites in a Node project with appropriate separate setup. Experiment with 2 versus 4 workers only after this work; increasing workers can worsen CPU contention. Do not shorten timeouts, disable isolation, or replace the DOM implementation to manufacture a green speedup. [Vitest performance guidance](https://vitest.dev/guide/improving-performance) supports separating environment/setup cost from execution.

This helps local feedback; it does not directly shorten a release while packaging remains slower.

### 6. Escalate only against measured residual costs

If app optimization/link dominates after caching, compare a larger macOS runner or a confirmed domain-crate split. Measure cost, memory, package size and startup/VM/archive behavior. Linux linker experiments matter only if Linux linking becomes critical. Release code-generation changes are last: existing release has no explicit whole-program LTO setting to simply remove; panic abort would break existing panic recovery, and release-only tests would change debug-assertion behavior. Removing `cargo clean --profile dev` cannot make unoptimized dependencies reusable as optimized ones. [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) explain these distinct settings.

## Bounded measurement and rollout

1. Use the existing run data for triage; do not rerun full builds to reconfirm seconds-long test execution.
2. Add per-command wall times and Cargo timing reports to diagnostic, artifact-only builds. Record source SHA, runner image/CPU, cache state, toolchain, features/profile, queue time, disk/RSS, test inventory, failures and billed minutes. Upload only reviewed timing reports, not enclosing build output directories. [Cargo timings](https://doc.rust-lang.org/cargo/reference/timings.html).
3. Land the cheap structural/preflight changes with focused regressions. Measure compilation removal on one controlled baseline/treatment pair first.
4. For scheduling and cache candidates, run an initial paired control/treatment against the same source inputs. Stop an ineffective candidate immediately. For promising candidates, use three paired macOS measurements with interleaved order because variance is large; validate Linux behavior and repeat if its measured critical-path effect matters. Benchmark changes separately, then once together. No release publication for experiments.
5. Report median and full observed range, never a population p95 or universal guarantee from three samples. Keep unchanged test counts/semantics, cache correctness, signing and provenance checks as hard gates. A speed change with flaky tests or a missing gate is rejected.
6. Accept a scheduling change only when end-to-end gain exceeds its observed noise and runner/queue overhead. If ambiguous, report it as unproven rather than repeatedly running until a favorable number appears. Cold builds must remain correct. Revert one optimization at a time on regression.

First implementation: the shared runtime-input contract and early preflight. Next: remove redundant integration-triggered binary compilation, measure, then pilot macOS job separation. Add the scoped dependency cache only after its sentinel boundary tests pass. Reassess the critical path after each accepted change.
