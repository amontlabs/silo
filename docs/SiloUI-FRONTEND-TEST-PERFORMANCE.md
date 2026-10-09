# Frontend test environment measurement

2026-09-14. Split 25 browser-free suites (158 tests) into a Node project and retain 58 jsdom suites (599 tests). Test bodies, full-window journeys, DOM implementation, isolation, timeouts and the default four-worker limit are unchanged. Unclassified files retain jsdom. Only the DOM project loads `src/test/setup.ts` and its browser stubs and matchers.

## Controlled local pair

Measured in an isolated worktree based on `40c7a4522e9a90aad9c888a35d5a3df8610fd6fb`, including the concurrent runtime-input implementation with its import fix held stable for the pair. Host: Apple M4 Max, 16 CPUs, arm64, macOS 26.5, Node 24.11.1, Vitest 4.1.11. Dependencies were installed; no native compilation ran during either measurement. These are local results, not GitHub runner results.

Both runs used this command from the repository root, with `LABEL` replaced by `baseline-fixed` or `treatment-fixed`:

```sh
/usr/bin/time -p npm --prefix app/SiloUI test -- --maxWorkers=2 \
  --reporter=default --reporter=json \
  --outputFile.json=src-tauri/target/verification/frontend-perf/LABEL.json
```

| Measurement | All jsdom baseline | Node/jsdom treatment |
| --- | ---: | ---: |
| Passed files / tests | 83 / 757 | 83 / 757 |
| Vitest wall time | 67.33 s | 64.51 s |
| Process wall time | 67.70 s | 64.88 s |
| Aggregate environment setup | 20.49 s | 14.42 s |
| Aggregate test setup | 2.68 s | 1.91 s |
| Aggregate test execution | 98.88 s | 99.72 s |
| Process user CPU time | 144.24 s | 136.29 s |
| Maximum overlapping test files | 2 | 2 |

The observed wall reduction is 2.82 seconds (4.2%); environment setup fell 29.6%. Exact file paths and all 757 test names match between JSON inventories. One pair does not establish the distribution or a universal speedup. Aggregate phase times overlap and must not be added to wall time. Packaging still dominates the release critical path.

The original failed baseline remains recorded: a new runtime-input read using `new URL(asset, import.meta.url)` failed during Vite transformation. Replacing that read with `resolve(dirname(fileURLToPath(import.meta.url)), path)` fixed the import; its focused 11 tests passed before the valid pair. An initial treatment inherited `maxWorkers: 4` into each project, overriding the root CLI limit; its 38.63-second result had four overlapping files and is excluded. The final configuration shares only aliases and transforms. Installed Vitest 4's `resolveMaxWorkers` prioritizes project limits over root limits, so the worker cap must remain at the root.

Raw logs and JSON reports are ignored local evidence under `app/SiloUI/src-tauri/target/verification/frontend-perf/`. The accepted files are `baseline-fixed.{log,json}` and `treatment-fixed.{log,json}`; other samples are diagnostic only.

## Full-window profile

The initial profile showed the expensive application tests cover committed edits across pages, reduced-motion tooltips rendered through portals, global filtering, and preference persistence. Onboarding's slowest cases cover machine-capacity validation across the save-action boundary, custom memory values, and final machine order reflected in Review. These exercise shared state and production adapters. All 88 application and 48 onboarding tests remain at their existing seams; moving those assertions into isolated components would weaken the behavior under test.

Use `npm --prefix app/SiloUI test -- --project node` for the browser-free suite or the existing `test:watch` command with a file filter for the affected behavior. The normal `test` command still discovers both projects. Typecheck, lint, and a direct TypeScript check of `vitest.config.ts` passed.

Primary references: [Vitest test projects](https://vitest.dev/guide/projects) and [performance guidance](https://vitest.dev/guide/improving-performance). Configuration inheritance and worker precedence were also verified against the installed Vitest 4.1.11 types and `resolveMaxWorkers` implementation; current online documentation can describe a newer major version.

## Bounded CPU follow-up

After the native experiment released the CPU, profiled all 88 tests in `application-app.test.tsx` with one worker and Node's `--cpu-prof`. An ignored temporary config supplied `execArgv` directly to the DOM project, preserving every other setting. Passing `--execArgv` only at the CLI root did not produce a worker profile with these inline projects; that first run was diagnostic only. Both runs passed all 88 tests.

The profiled run took 36.88 seconds in Vitest, including 35.84 seconds executing tests, 72 ms setup, 487 ms imports and 398 ms environment initialization. The worker profile sampled 36.855 seconds of elapsed timeline; this includes idle samples and profiler overhead and is not an uninstrumented benchmark.

| Profile stack | Inclusive sampled time | Share of worker timeline |
| --- | ---: | ---: |
| jsdom `prepareComputedStyleDeclaration` | 20.618 s | 55.9% |
| jsdom `applyStyleSheetRules` | 20.575 s | 55.8% |
| jsdom stylesheet selector `matches` | 18.853 s | 51.2% |
| Testing Library role queries | 15.223 s | 41.3% |
| Accessible-name calculation | 14.290 s | 38.8% |
| user-event dispatch and descendants | 4.241 s | 11.5% |

These rows overlap: role queries compute accessible names, which inspect computed styles and match stylesheet selectors. Exclusive samples attribute 34.7% to jsdom, 22.3% to its DOM selector engine, 13.6% to React plus React DOM, and 0.4% to user-event itself. Idle accounts for 6.3%; garbage collection for 2.8%. This is style and selector work, not a browser layout measurement.

There is no demonstrated cheap setup/configuration waste left in this file. Setup and environment startup together account for under half a second. Real CSS affects collapsed-sidebar visibility and pointer interaction, and the suite explicitly checks tooltip and reduced-motion styling. Removing stylesheet processing, caching computed styles across mutations, disabling interaction checks, or substituting cheaper queries indiscriminately would weaken the tested behavior. Further work should reduce unnecessary query scope at proven component boundaries or minimize a reproducer for the upstream style/selector implementation, then benchmark that specific change. No such unmeasured test or dependency change was retained.

Local ignored evidence: `frontend-perf/cpu-profile/run-fixed.log`, `CPU.20260914.164040.27935.0.001.cpuprofile`, `summary.json`, and `hot-functions.json`. The temporary profiling configuration and sample-aggregation script are beside them. Analysis stopped within five minutes of the CPU handoff.

## Stylesheets in the jsdom document (K-09)

2026-09-30, branch `fix/wp-k2` on `c63ade1`. The review hypothesised that Sonner's injected stylesheet dominates the selector matching measured above. Sonner inserts a 14.9 KB sheet (96 style rules, 101 selectors) into `<head>` on import, and `src/test/setup.ts` imports Sonner for every DOM suite. The component stylesheets the tests load (`sidebar-shell.css`, `application-shell.css`, `sandbox-list.css`; Tailwind is not compiled in tests) hold 71 rules and 92 selectors.

Counting sheets at the end of each file found a larger source. jsdom 29.1.1 keeps the stylesheet of a `<style>` element that leaves the document inside a removed subtree: `Node-impl.js` `_remove()` calls `_detach()` before it clears the descendants' cached roots, so the nested element still reports `isConnected` and re-adds its sheet. Radix ScrollArea renders such an inline `<style>` (2 rules) per instance, and the leaked sheets stay for the rest of the file: 237 sheets at the end of `application-app.test.tsx`, 151 in `onboarding-app.test.tsx`, 57 in `sandbox-detail.test.tsx` and 2 in `status-bar.test.tsx`, instead of 1–7.

### Method

Host: Apple M4 Max, 16 CPUs, macOS 26.5, Node 26.10.0, Vitest 4.1.11, jsdom 29.1.1. One test file per Vitest process under `nice -n 10`, `--testTimeout=60000`, JSON reporter; the figure is the sum of test durations (process user CPU in brackets). Variants were temporary environment toggles in `src/test/setup.ts` that were not committed: **drop** removes Sonner's sheet after import, **purge** re-runs the style-block update of disconnected `<style>` owners before each test, **both** combines them. Runs were interleaved with the order rotated per repetition, three repetitions each, medians reported. About 20 agents shared the machine; the one-minute load average was 4.5–20.4 during the matrix and 13.6–58.9 during the final pair, and single runs varied by up to 60%. Treat the numbers as directional.

| File (tests) | Baseline | drop | purge | both |
| --- | ---: | ---: | ---: | ---: |
| `application-app.test.tsx` (97) | 53.9 s (58.3) | 51.3 s, −4.9% (54.5) | 27.6 s, −48.8% (31.7) | 21.7 s, −59.7% (25.4) |
| `onboarding-app.test.tsx` (48) | 37.0 s (41.0) | 33.7 s, −8.9% (37.0) | 19.8 s, −46.5% (23.1) | 16.5 s, −55.4% (19.7) |
| `sandbox-detail.test.tsx` (22) | 3.01 s (4.78) | 2.57 s, −14.5% (4.40) | 2.33 s, −22.7% (4.08) | 1.91 s, −36.4% (3.61) |
| `status-bar.test.tsx` (24) | 1.13 s (2.60) | 0.92 s, −18.3% (2.39) | 1.12 s, −0.3% (2.59) | 0.92 s, −18.0% (2.37) |

Every variant passed every test. An earlier two-variant run (baseline against drop, three repetitions) agreed: −9.3%, −7.7%, −13.7% and −18.8%.

### Result

The hypothesis is rejected as stated: Sonner's sheet costs about 5–9% on the two heaviest files and 14–18% on smaller ones. The leaked sheets cost about half of the two heaviest files. Both are now handled in the shared DOM setup (`src/test/stylesheets.ts`, wired in `src/test/setup.ts`) without changing any style a test can observe:

- Sonner's sheet stays in the document exactly while an `<ol data-sonner-toaster>` exists, which Sonner renders only while toasts are shown. A wrapper checks this before every `getComputedStyle` call and prepends the sheet to keep its cascade position. Apart from custom properties on `html[dir]`, every Sonner rule targets that list, so toast visibility and styling assertions still compute against Sonner's real CSS. No suite opts in or out.
- Before each test, disconnected `<style>` owners re-run their style-block update, which drops the leaked sheets.

`src/test/stylesheets.test.tsx` covers both. Its canary assertion fails once jsdom stops leaking the sheet; then delete `purgeDetachedStyleSheets`. jsdom 30.1 reworked node removal (its `main` branch clears descendants' cached roots right after removal), which likely fixes the leak; upgrading from 29 is a major dependency change outside K-09 and was not tried. All 19 Sonner-using DOM files passed with the change (194 tests).

Final pair, the committed baseline setup against the K-09 setup, three interleaved repetitions:

| File (tests) | Baseline | K-09 setup |
| --- | ---: | ---: |
| `application-app.test.tsx` (97) | 68.1 s (70.5) | 28.6 s, −58.0% (32.0) |
| `onboarding-app.test.tsx` (48) | 43.1 s (47.4) | 27.3 s, −36.5% (28.3) |
| `sandbox-detail.test.tsx` (22) | 3.48 s (5.64) | 2.30 s, −34.0% (4.20) |
| `status-bar.test.tsx` (24) | 1.40 s (3.21) | 1.05 s, −24.8% (2.72) |

The full suite was not timed here (the coordinator runs it). The earlier conclusion that no cheap setup waste remained was wrong for stylesheets: this change removes rules no element can match rather than stylesheet processing, so real CSS still reaches every element it applies to.

Local ignored evidence: `app/SiloUI/src-tauri/target/verification/k09/` holds `matrix/results.jsonl`, `main/results.jsonl` and `final/results.jsonl` with per-run JSON reports, the drivers `ab.mjs` and `ab-final.mjs`, `summarize.mjs`, `count-rules.mjs`, and the minimal jsdom reproducer `jsdom-style-leak.mjs`.

## Deterministic GitHub notification waits (2026-10-02)

At `087b9063`, the verbose full-suite profile ranked
`application-shell-navigation.test.tsx` (12.297 s / 38 tests),
`application-lifecycle.test.tsx` (10.216 s / 31), and
`onboarding-machines.test.tsx` (9.275 s / 15) highest by summed test
duration. The former application/onboarding monoliths have been split.
`github-page.test.tsx` ranked eighth (5.745 s / 7), with a 4.626-second
success-notification test: it waited 4,500 ms on a real timer. Two background
notification assertions also waited 300 ms each.

Replace those waits with Vitest's supported asynchronous fake-timer advancement
inside React `act`. Keep the real page, Sonner, stylesheet processing, user
interaction, 4,500 ms persistence boundary, background-notification assertions,
and all seven tests. Add an explicit Close toast interaction and disappearance
assertion to complete the existing test's stated behavior. During folding,
`29dfb30b` independently landed the same optimization with the shared
`setupFakeTimerUser` helper. The merged file keeps that helper, which uses
user-event's supported `advanceTimers` option and an `act` async wrapper, and
adds the close assertion followed by 200 ms of fake exit-animation time. Shared
setup restores real timers on failure. A temporary mutation changing this success toast's `persist: true` to
`false` failed at the post-4,500 ms assertion; the production source was restored.

### Controlled comparison

Host: Apple M4 Max, 16 CPUs, macOS 26.5, Node 24.11.1, Vitest 4.1.11. Two
interleaved real/fake pairs used the following command (replace `LABEL`):

```sh
PATH=/Users/polarzero/.nvm/versions/node/v24.11.1/bin:$PATH \
  /usr/bin/time -p npm --prefix app/SiloUI test -- \
  src/features/application/pages/github-page.test.tsx \
  --maxWorkers=1 --testTimeout=60000 --reporter=verbose --reporter=json \
  --outputFile.json=src-tauri/target/verification/test-speed/LABEL.json
```

The comparison baseline holds the merged assertions and `act` boundaries
stable, but uses real timers and ordinary user-event scheduling. The higher CLI timeout is for both measurements
only; the retained test uses the ordinary default timeout. All four runs passed
the same seven names and assertions. Values below are medians of two runs.

| Measurement | Real timers | Fake timers | Reduction |
| --- | ---: | ---: | ---: |
| Success test duration | 5.747 s | 1.293 s | 77.5% |
| Sum of seven test durations | 10.390 s | 7.032 s | 32.3% |

Twenty-two agents shared this host; load averages rose above 100. The paired
results remove a known 5,100 ms of real waits but do not establish a stable
full-suite wall-time improvement. The initial Node 26 full run passed 207 files
and failed two: the two GitHub background tests emitted React `act` warnings,
and an unrelated Updates `it.fails` unexpectedly passed. The initial focused
Node 24 baseline reproduced both GitHub warnings. A diagnostic controlled
baseline without the higher CLI timeout exceeded the default timeout; its
subsequent failures are excluded. A Homebrew Node invocation failed before
Vitest because of a missing dylib; subsequent focused runs used the working
Node 24.11.1 installation. All failing logs remain preserved.

Focused treatment checks passed all seven tests, typecheck, and touched-file
oxlint. This is fixture verification; no app bundle or live VM was inspected.
Ignored raw evidence is under
`app/SiloUI/src-tauri/target/verification/test-speed/`: `frontend-before.*`,
`github-before-fixed.*`, `github-before-controlled.*`,
`github-merged-{real,fake}-{1,2}.{log,json}`, `github-mutation.log`, and
the exact comparison sources `github-merged-{baseline,treatment}.tsx`. Earlier
`github-{real,fake}-{1,2}` pairs timed a narrower fake-timer variant before the
merge and are excluded from the final table.

Primary references: [Vitest timers](https://vitest.dev/guide/mocking/timers)
and [Testing Library fake timers](https://testing-library.com/docs/using-fake-timers/).
Both document advancing supported fake timers and restoring real timers;
Testing Library also warns about user-event scheduling with fake timers.

### Rejected query-scope follow-up

A disposable change scoped `onboarding-machines.test.tsx`'s configured-sandbox
list query to the active tab panel. The baseline passed all 15 tests; treatment
failed the existing editor-focus assertion (14 passed). These durations are not
a valid speed comparison. Restore the original query rather than retaining an
unverified optimization. Both verbose logs and JSON reports remain in
`test-speed/machines-{before,after}.{log,json}` under the ignored evidence root.

## Further Node-project suites (2026-10-09)

Moved 18 more browser-free suites (the Node project grew from 40 to 58 files) by checking every remaining `*.test.ts` and `*.test.tsx` suite in the DOM project: first for DOM globals, testing-library, and timers in the file, then by running the candidates in the Node project. Eight candidates failed there because they read `window`, `document`, `localStorage` or the setup stubs (`identity-resume`, `network-mutations`, `production-directory`, `production-file-transfers`, `production-source-cleanup`, `workspace-storage`, `bundled-help`, `console-error-guard`) and stay in jsdom. The moved suites passed without `src/test/setup.ts`, none of their modules branch on `window`/`document`, and the Node run printed no `console.error` output that the DOM setup's guard would otherwise have rejected. Moved: `desktop/{editor-include,linux-desktop-state,native-contracts,production-source-validation,transfer-result-notice}`, `features/application/components/application-commands`, the three `features/computers/model` suites, `features/status-bar/computer-menu-items`, `fixtures/{application-scenarios (.ts and .tsx),directory-loader,log-pages,scenarios}`, `lib/{relative-time,visible-text}` and `test/native-bridge-mock`.

Same command as above (`npx vitest run --maxWorkers=2 --reporter=default --reporter=json`), 257 files and 2,636 tests passing every time. Host: Apple M4 Max, shared with other agents (load average 7 to 12), so wall times are noisy.

| Run | Vitest wall time | Aggregate environment setup |
| --- | ---: | ---: |
| Before | 123.59 s | 50.63 s |
| After, run 1 | 135.49 s | 47.25 s |
| After, run 2 | 114.46 s | 43.17 s |

The moved suites are tiny, so the saving is the removed jsdom environment setup (about 3 to 7 s of aggregate time, not wall time) and is within the run-to-run noise. The slowest files are DOM-bound (`application-shell-navigation` 8.1 s, `application-lifecycle` 6.8 s, `linux-desktop-bridge` 6.1 s, `onboarding-configurations` 5.1 s, `onboarding-github` 4.2 s). No per-file change was made.

Suggestion, not applied: `src/desktop/linux-desktop-bridge.test.ts` spends about 5 s in real waits (20 to 600 ms) that match grace periods of the script evaluated with `window.eval`; fake timers would need the script to use the faked clock, which was not verified. Evidence is under `app/SiloUI/src-tauri/target/verification/frontend-perf-2026-10-09/`.
