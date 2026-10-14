# Silo third code review pass: release and CI, 2026-10-02

This pass reviews release tooling, workflow gates, updater artifacts, release documentation, and development/production channel separation. The ledger records fifteen new findings: thirteen P2 and two P3. Fourteen are fixed or corrected and folded; RL-06 remains open because its correction requires a qualified macOS 14 execution environment. These results do not establish release readiness.

## Revision and scope

Initial revision: `f94219259d081ae3297887d0a5660faac22f2d3e`, branch `codex/review-release-ci`. Each finding records its confirmation and subsequent fix separately because integration changes during this pass. Final code-verification cutoff: `8e497736`. Later integration merges are recorded by Git and do not extend this verification claim. RL-07, RL-08 and RL-10 came from concurrent release-script review commits folded into integration; their original evidence is attributed below, and their regressions were rerun here.

The first two reports were read from the main checkout before review. This ledger excludes R-01 through R-38 and previously recorded improvement opportunities, including APT retention (R-13), website/demo CI coverage (R-14), LFS compiler identity (R-20), and independent draft-asset authentication (O-11).

Reviewed boundaries: `app/SiloUI/scripts/`, `.github/workflows/`, `.github/actions/prepare-release-runtime/`, updater configuration and asset contracts, `docs/SiloUI-RELEASES.md`, and host names defined by `src-tauri/src/channel.rs`.

P1 denotes an essential delivery or isolation failure; P2 denotes a concrete correctness, availability, or verification failure; P3 denotes a narrower defect with a practical workaround. Evidence distinguishes executed fixture behavior from source-confirmed consequences.

No release was published or signed, and no repository or remote release tag was changed. The existing Node adapter suite created unsigned tags in disposable test repositories in two runs; their cleanup removed those fixtures. The attempted exclusion filter did not exclude that test; a subsequent positively selected publication regression ran alone. This verification mistake violated the instruction not to tag and is recorded explicitly. No runtime preparation, app launch, real VM, production HOME, or credential store is used. Tests use synthetic inputs and temporary paths.

## Findings at a glance

| ID | Priority | Finding | Status |
| --- | --- | --- | --- |
| RL-01 | P2 | Wrapper configuration crosses the Cargo argument separator | Fixed and folded: `9baaac4e` |
| RL-02 | P2 | Linux verification seeds production paths for a Dev binary | Fixed and folded: `5a14fe19` |
| RL-03 | P2 | Desktop smoke isolation leaves HOME-dependent state live | Fixed and folded: `024c8268` |
| RL-04 | P2 | Direct publication bypasses the explicit stable-release opt-in | Fixed and folded: `5b18ec21` |
| RL-05 | P3 | Release guide omits the implemented in-app Debian update | Corrected and folded: `17edafc6` |
| RL-06 | P2 | Required minimum-macOS gate uses a runner with imminent brownouts | Open; replacement qualification required |
| RL-07 | P2 | MicroSandbox fallback executable cache ignores compiler flags and build recipe | Fixed and folded: `abae62fe` |
| RL-08 | P2 | Failed license staging replaces the MicroSandbox sidecar without its manifest | Fixed and folded: `3329b2b3` |
| RL-09 | P2 | Optional GNOME verifier uses obsolete tray and channel expectations | Fixed and folded: `028ce994` |
| RL-10 | P2 | Package identity gate accepts another Debian package or macOS Dev bundle | Fixed and folded: `8102c670` |
| RL-11 | P2 | Lifecycle guest probes use the production alias for Dev | Fixed and folded: `f7cec265`, `b64a60e5` |
| RL-12 | P2 | Failed Linux tool staging destroys complete prior package inputs | Fixed and folded: `a173affb` |
| RL-13 | P2 | macOS archive verification ignores conflicting archive roots | Fixed and folded: `296966fc` |
| RL-14 | P2 | AppImage verification accepts missing or unsupported format magic | Fixed and folded: `8346345f` |
| RL-15 | P3 | Linux packaging filters omit runtime implementations and locked inputs | Fixed in accompanying commit |

## Detailed findings

### RL-01 Wrapper configuration crosses the Cargo argument separator

**P2.** Confirmed at `81523e2d`. Location: [build_desktop.py](../app/SiloUI/scripts/build_desktop.py), development configuration insertion and Linux packaging branch.

**Trigger.** Run a supported debug/unbundled build with raw Cargo arguments, for example `desktop:build -- --debug --no-bundle -- --locked`. Linux bundled builds with raw Cargo arguments have the same placement error for their package configuration.

**Evidence.** Calling the actual `build` function with a recording runner produced Tauri arguments `--debug --no-bundle`, then Cargo arguments `--locked --config src-tauri/tauri.dev.conf.json`. The wrapper appends configuration after the separator. A direct, offline `cargo +1.94.0 metadata --no-deps --config app/SiloUI/src-tauri/tauri.dev.conf.json` rejected the JSON at line 1 as invalid TOML, before compilation. Tauri documents [runner arguments and configuration options](https://v2.tauri.app/reference/cli/); the observed command crosses those boundaries.

**Consequence.** Debug builds with ordinary Cargo options fail instead of selecting the development configuration. Linux package layout/updater overrides likewise reach Cargo instead of Tauri. This reproduction establishes rejected builds, not successful production-state access.

**Correction.** Insert every wrapper-owned Tauri configuration before the first `--`, preserving the raw Cargo suffix exactly. Keep the local optimized macOS rejection of unsupported raw arguments.

**Rejecting test.** Exercise the real wrapper with debug macOS, debug Linux, and release Linux arguments containing `-- --locked`. Assert the effective Tauri configuration selects Dev for debug and the private Linux tool layout without updater artifacts; the Cargo suffix must contain only the original Cargo arguments. Preserve no-separator behavior and the optimized macOS rejection.

**Status.** Fixed and folded in `9baaac4e`. The regression failed for all three cases before the fix; all eight desktop-wrapper tests now pass. This is internal build routing and needs no application changeset.

### RL-02 Linux verification seeds production paths for a Dev binary

**P2.** Confirmed at `02344c24`. Locations: the then-current `linux-verification.yml`, debug build and WebDriver steps; [test-linux-desktop.py](../app/SiloUI/scripts/test-linux-desktop.py), `run` fixture identity, settings and autostart paths.

**Trigger.** Run the Linux verification workflow. It invokes `desktop:build -- --debug --no-bundle --ci`, which selects `org.silo.dev`, then invokes the smoke harness without `SILO_LINUX_APPLICATION_ID`.

**Evidence.** The actual wrapper selects `tauri.dev.conf.json` for this command. The harness defaults its fixture identifier to `org.silo.preview`, writes `onboardingComplete` under that identifier, relaunches the Dev binary, and waits for the post-onboarding Backup navigation. Dev reads `org.silo.dev` instead. The harness also waits for `autostart/org.silo.preview.desktop`, while `channel.rs` and `system_integrations/linux.rs` select `org.silo.dev.desktop`. These are two separate waits in the same incompatible fixture setup. No native GUI was launched in this review.

**Consequence.** The Linux desktop verification cannot complete its stated workflow: the seeded settings do not advance onboarding, and correcting only the settings path still leaves the autostart wait broken. A failing smoke job provides no claimed settings/relaunch/autostart evidence.

**Correction.** Pass the exact Dev identifier from the workflow and derive the autostart expectation from the selected channel, preserving production-package qualification when explicitly requested. Label the debug build as development.

**Rejecting test.** Run the actual build-wrapper seam with the workflow's build arguments, resolve its configuration identifier, and evaluate the harness's fixture path expressions with the workflow environment. Settings and autostart paths must match that identifier's channel. Then qualify the full WebDriver smoke on Linux; deterministic path checks alone do not prove native GUI behavior.

**Status.** Fixed and folded in `5a14fe19`. The new path regression failed before the fix; both path tests, all eight wrapper tests and both workflow-pin checks pass. These tests execute the actual wrapper and extracted harness expressions, not WebDriver. This is internal verification tooling and needs no application changeset. The local default-binary path is also a debug build: follow-up `b64a60e5` defaults the smoke fixture to Dev and forwards that identifier to the optional GNOME adapter. Explicit production fixtures retain their released paths.

### RL-03 Desktop smoke isolation leaves HOME-dependent state live

**P2.** Confirmed at `9ea078cd`. Locations: [test-linux-desktop.py](../app/SiloUI/scripts/test-linux-desktop.py), `run` environment setup; [remote.rs](../app/SiloUI/src-tauri/src/remote.rs), `directory`, `read_config_in`, and `start`; [runtime.rs](../app/SiloUI/src-tauri/src/runtime.rs), `runtime_home_alias`.

**Trigger.** Run the documented desktop smoke command on an account that already has Silo or Silo Dev host state. The smoke harness copies the caller's environment and replaces only XDG CONFIG/DATA/CACHE roots.

**Evidence.** `HOME` remains inherited. Native startup calls `remote::start`; its directory comes directly from HOME plus the current channel's private directory, reads or creates the remote host configuration, and binds the channel's control socket. Runtime aliases also use HOME. A disposable XDG directory therefore does not isolate those paths. With the Dev workflow fixed in RL-02 this is `.silo-dev`; explicit production-package smoke uses `.silo`. No real host state was accessed in this review.

**Consequence.** A supposedly isolated smoke run can read existing remote-host settings, create persistent host state, contend with a user's control socket, or repoint an enabled management bridge to the test app. Its temporary XDG state can disappear while aliases remain in the caller's home. Hosted CI has disposable accounts; the documented local invocation does not provide that boundary.

**Correction.** Create HOME inside the smoke fixture before starting any native process and pass it alongside the XDG roots. Preserve inherited executable/tool search paths. Keep the separately supplied lifecycle fixture contract separate from this automatically created smoke fixture.

**Rejecting test.** Execute the real smoke environment prelude against a synthetic caller HOME containing sentinel channel state. HOME and all three XDG roots passed to the app must be inside the temporary task root, and the sentinel must remain unchanged. Native GUI qualification remains separate.

**Status.** Fixed and folded in `024c8268`. The environment regression failed for HOME before the fix; all 13 focused environment/path/wrapper/pin tests pass. This is internal verification tooling and needs no application changeset.

### RL-04 Direct publication bypasses the explicit stable-release opt-in

**P2.** Confirmed at `1aeac26d`. Locations: [publish-release.py](../app/SiloUI/scripts/publish-release.py), `main` version gate; [publish-release.yml](../.github/workflows/publish-release.yml), dispatch inputs and publication command; [release.mjs](../app/SiloUI/scripts/release.mjs), publication dispatch.

**Trigger.** A maintainer directly dispatches the publication workflow for a prepared `v1.0.0` or later draft, bypassing the local npm wrapper. Manual workflow dispatch is a documented retry path.

**Evidence.** `sync-release.mjs` and `release.mjs` enforce `assertPreStable` unless `--allow-stable` is explicit. The Python backend accepts every nonzero three-part version, including `1.0.0`; its publication path and the workflow read no stable-release opt-in. The early build workflow also accepts synchronized 1.x metadata, as its existing `1.2.3` fixture demonstrates. Given a complete valid newer draft, ordinary signature/checksum/identity gates do not reject its major version. This is source-confirmed policy bypass; no GitHub publication was attempted.

**Consequence.** The final publication boundary does not enforce the repository's requirement to keep Silo below 1.0.0 until explicitly authorized. Environment review alone does not express the promised distinct stable-release decision.

**Correction.** Require an explicit stable-release flag in the final Python publication gate and carry it through the workflow's boolean input and the existing local wrapper option. Keep parsing/comparison of historical public versions independent of this policy so a prior 1.x tag does not cause unrelated parsing failures. Enforce the same opt-in in the early draft build gate.

**Rejecting test.** Direct draft creation and publication for `1.0.0` and later must fail before any GitHub call without opt-in; explicit opt-in must reach the ordinary asset gates. The local wrapper must dispatch the workflow opt-in only when explicitly requested. Execute the workflow command with a fake Python verifier and verify its argument list in both modes, without signing or contacting GitHub.

**Status.** Fixed and folded in `5b18ec21`, including the early build gate, both final workflow commands, the backend, wrapper propagation and release-guide instructions. Tag-triggered 1.x drafts have no opt-in and fail early; an owner-approved stable draft requires manual dispatch with both booleans selected. No stable release is authorized by this change. The four Python rejection subcases, wrapper propagation, workflow forwarding, and early draft regression failed before their fixes. All 33 focused Python tests and all 27 Node release-adapter tests pass. This is release-policy tooling and needs no application changeset.

### RL-05 Release guide omits the implemented in-app Debian update

**P3.** Confirmed at `2c78975b`. Location: [SiloUI-RELEASES.md](SiloUI-RELEASES.md), supported-packages table and Debian update description.

**Trigger.** Use the authoritative release guide to choose the Linux update acceptance path or advise an installed Debian-package user.

**Evidence.** Both Linux rows say in-app updates are AppImage-only, and the following prose describes Debian's package-manager/download flow. Current `updates.rs::install_update` instead dispatches Debian packages to `install_debian`, which invokes the authenticated `pkexec` helper, refreshes and downloads through APT, then installs the selected version. [The dedicated Linux update guide](SiloUI-LINUX-UPDATES.md#in-app-debian-updates-14-september-2026) documents that implemented route.

**Consequence.** The release guide understates supported behavior and directs contributors away from the actual in-app system-authentication and update path when planning installer qualification. Existing acceptance requirements still mention Debian upgrades, so this is narrower than missing all Debian coverage.

**Correction.** Describe AppImage's signed updater and Debian's authenticated APT operation separately in both Linux rows, and link the in-app Debian procedure. Corrected in this update without changing application behavior.

**Rejecting test.** Review the table against the package-kind branches in `install_update`: macOS uses the signed app archive, AppImage uses signed replacement, and Debian uses authenticated APT. Keep all three represented. This is a prose correction; no implementation-mirroring test was added and no package was installed.

**Status.** Corrected and folded in `17edafc6`; no application changeset is needed.

### RL-06 Required minimum-macOS gate uses a runner with imminent brownouts

**P2; scheduled availability failure, not an observed hosted run.** Confirmed at `071ca883` against the primary runner announcement on 2026-10-02. Locations: [release.yml](../.github/workflows/release.yml), `macos-minimum-constraints` and the draft job's required dependencies; [SiloUI-RELEASES.md](SiloUI-RELEASES.md), minimum-version runner qualification.

**Trigger.** Build a draft during a macOS 14 runner brownout, first October 5 at 14:00 UTC through October 6 at 00:00 UTC, or after runner retirement on November 2, 2026.

**Evidence.** The required `macos-minimum-constraints` job runs only on `macos-14`; the draft job requires its success. GitHub's [runner retirement announcement](https://github.com/actions/runner-images/issues/13518) explicitly says brownout jobs fail, lists eight October windows, and says the image becomes unsupported November 2. The release guide correctly names the retirement date but misses the earlier brownout failure windows. There is no alternative minimum-version job or qualified runner in this workflow. Package/platform jobs can still complete while draft creation remains blocked.

**Consequence.** Draft delivery predictably fails during the announced windows and becomes unavailable on this workflow after retirement. Successful macOS 15 package builds cannot satisfy the separate promised macOS 14 library-constraint evidence.

**Correction.** Qualify and configure a maintained macOS 14 execution environment before the first brownout. Keep the minimum-version gate blocking. Until replacement is ready, document the brownout windows and rerun failed draft builds outside them; that workaround expires at retirement. Changing the runner to macOS 15 or skipping the job would discard the supported-minimum proof.

**Rejecting test.** Run the exact constraint suite on the replacement's verified macOS 14 environment and attach its OS identity and results to a complete draft-verification run. Inject a gate failure and confirm draft creation stays blocked. A source assertion that merely changes `runs-on` or a newer-OS passing run rejects neither failure mode.

**Status.** Left open because no qualified replacement runner or macOS 14 host was provided, and the shared task instructions prohibit app launches and network-heavy builds. Provisioning and qualifying a runner is not a small obvious source fix. The release guide now records the first brownout and the temporary rerun limitation. No CI/environment configuration was changed to bypass this gate.

### RL-07 MicroSandbox fallback executable cache ignores compiler flags and build recipe

**P2.** Location: [microsandbox-runtime.mjs](../app/SiloUI/scripts/microsandbox-runtime.mjs), compiled executable cache identity. Confirmed by concurrent review before `abae62fe`.

**Trigger.** Restore the fallback executable cache, then change the staging recipe or inherited Cargo compiler flags without changing source, patches, agent bytes, compiler version, target or features.

**Evidence.** The concurrent review executed real staging/cache orchestration with deterministic external tools. It compiled eight times across existing capability/source cases; changing `RUSTFLAGS` left the count at eight instead of nine. The cache key omitted both the recipe digest and compiler flags. No native build was run. The focused staging suite was independently rerun here.

**Consequence.** Runtime preparation can silently retain an executable built under obsolete compiler policy, even though the current build invocation requests different flags or a corrected recipe.

**Correction.** Include the staging script digest and the four Cargo flag environment inputs in compiled-cache identity. The release guide now describes these inputs.

**Rejecting test.** Change plain, encoded, build-wide and target-specific flags and require a rebuild for each. Keep the inputs unchanged and require reuse; changes to previously covered source/capability inputs must still invalidate the executable.

**Status.** Fixed and folded by concurrent review in `abae62fe`. All thirteen focused runtime-staging tests passed here. This is internal preparation tooling and needs no application changeset.

### RL-08 Failed license staging replaces the MicroSandbox sidecar without its manifest

**P2.** Location: [microsandbox-runtime.mjs](../app/SiloUI/scripts/microsandbox-runtime.mjs), `stageRuntime` output publication. Confirmed by concurrent review before `3329b2b3`.

**Trigger.** Prepare a complete runtime, then stage a newly compiled executable while a later license fetch fails.

**Evidence.** The concurrent fixture ran real staging with deterministic external adapters. The second call rejected on the license URL, but the sidecar contained replacement bytes while the old library and manifest remained. Its rejecting regression failed on the original sidecar comparison. No native executable or network request was used. The focused staging suite was independently rerun here.

**Consequence.** A failed preparation corrupts the previously usable runtime set: the sidecar digest no longer matches its retained manifest. Retrying cannot rely on the prior complete outputs remaining intact.

**Correction.** Publish the sidecar after resource and manifest staging completes. Keep late license failures before any replacement of the sidecar.

**Rejecting test.** Force a license failure after a new sidecar is ready and require all three previously prepared outputs to retain their original bytes. A successful-stage-only test or checking only the manifest would miss this failure.

**Status.** Fixed and folded by concurrent review in `3329b2b3`. All thirteen focused runtime-staging tests passed here. This establishes preservation for the tested staging failure; it does not establish atomic publication across a process crash or final filesystem errors. No application changeset is needed.

### RL-09 Optional GNOME verifier uses obsolete tray and channel expectations

**P2.** Confirmed at `ac8aaf87`. Locations: [linux_desktop_services.py](../app/SiloUI/scripts/linux_desktop_services.py), tray item selection, Open action and health fixture; [tray.rs](../app/SiloUI/src-tauri/src/tray.rs), Linux `menu` and `title`.

**Trigger.** Enable the smoke harness's optional `SILO_LINUX_DESKTOP_SERVICES=gnome` mode against a current production or Dev fixture.

**Evidence.** Native Linux tray code exports the menu label `Open Silo`; the GNOME helper selects `Open Silo…` and therefore raises `StopIteration` for the current menu even in production. For Dev, it first rejects the channel's `Silo Dev` tray title and window title because both comparisons require `Silo`. Its health-error injection also writes metadata under the production identifier regardless of the selected fixture identifier. These are source-confirmed adapter mismatches; no desktop service was launched.

**Consequence.** The optional verification cannot reach its claimed native reopen, health notification, and Quit checks. The title and metadata assumptions also prevent extending that qualification to the Dev channel selected by the current build workflow. This is separate from RL-02's ordinary settings/autostart smoke paths.

**Correction.** Match the actual Open action and derive tray/window names and health metadata from the selected channel. Keep the native menu's current shared action labels; changing product menus is outside this fixture correction.

**Rejecting test.** Feed the extracted real GNOME menu selector a layout populated from the native tray's actual action labels. It must select Open and Quit. Exercise the tray-title predicate and health metadata path for both standard channel identifiers, requiring Dev state to remain under the Dev fixture root. Then qualify the full optional GNOME run separately.

**Status.** Fixed and folded in `028ce994`; merge correction `ee4419c5` follows the integrated native menu change. Integrated `466b6ae8` reads product names from `channel.rs`. No application changeset is needed.

**Integration follow-up for RL-09.** A subsequent native channel-name change made Open and Quit labels use the product name. The merged regression caught the obsolete literal adapter and failed with `StopIteration`. The adapter and source-derived menu regression now cover `Open Silo` / `Quit Silo` and `Open Silo Dev` / `Quit Silo Dev`; both channel cases pass.

### RL-10 Package identity gate accepts another Debian package or macOS Dev bundle

**P2.** Location: [verify-release-metadata.py](../app/SiloUI/scripts/verify-release-metadata.py), final package identity checks. Confirmed by concurrent review before `8102c670`.

**Trigger.** Supply a correctly versioned and architected Debian package belonging to another application, or a macOS archive with the Dev/another bundle identifier, to the final metadata verifier.

**Evidence.** The concurrent regression supplied synthetic package contents for both Debian architectures with `Package: unrelated`, plus ARM64 macOS archives with Dev/other identifiers. Four rejecting subcases failed because all were accepted. These fixtures exercise metadata verification, not signing or publication. Their tests were rerun here; the real Debian-container case is skipped on this macOS host because `dpkg-deb` is unavailable.

**Consequence.** Version/architecture checks alone do not ensure that a signed artifact is the production Silo application. A wrong package can pass the metadata gate and be assigned a production updater asset name.

**Correction.** Require Debian `Package: silo` and the production macOS identifier from tracked Tauri configuration, in addition to existing version/architecture checks.

**Rejecting test.** Accept the production macOS fixture and reject Dev/other identifiers. Reject the wrong Debian package name for both architectures; on Linux, build real `.deb` fixtures and accept `silo` while rejecting the unrelated package. Keep prior old-version/wrong-architecture rejections.

**Status.** Fixed and folded by concurrent review in `8102c670`. Synthetic identity regressions passed here; real Debian qualification remains a Linux check. This is release verification tooling and needs no application changeset.

### RL-11 Lifecycle guest probes use the production alias for Dev

**P2.** Confirmed at `0aef0323`. Locations: [test-linux-desktop.py](../app/SiloUI/scripts/test-linux-desktop.py), `run_lifecycle` identifier and nested `guest_for`; [runtime.rs](../app/SiloUI/src-tauri/src/runtime.rs), `runtime_home_alias`; [channel.rs](../app/SiloUI/src-tauri/src/channel.rs), `from_identifier` and `state_dir_name`.

**Trigger.** Run lifecycle verification against an isolated Dev fixture, or the helper's default historical `org.silo.preview.linux-checkpoints` identifier. Native code classifies every identifier except exact `org.silo.preview` as Dev.

**Evidence.** `guest_for` computes its alias under HOME plus literal `.silo`, while native runtime aliases use `.silo-dev` for Dev and historical/custom identifiers. A deterministic execution of the actual nested guest probe with the correct fixture alias fails its missing-directory assertion for both Dev and the historical default. Explicit production passes the same seam. A separate default-identifier regression rejects the historical default, which is no longer one of the two supported builds.

**Consequence.** Migration can finish, but the first guest probe fails before invoking the bundled CLI, so lifecycle verification provides no claimed guest/checkpoint/archive evidence for Dev. This is a hard-coded host name outside `channel.rs`, distinct from ordinary smoke settings in RL-02 and the GNOME adapter in RL-09.

**Correction.** Default lifecycle fixtures to the supported Dev identifier and select the private directory using the native channel rule, retaining explicit production fixture compatibility and the native Dev treatment of historical identifiers.

**Rejecting test.** Execute the actual guest probe with synthetic aliases for Dev, production, and the historical identifier. Only the matching alias exists; the recorded CLI environment must receive that alias as `MSB_HOME`. The default must select Dev. No app, VM, CLI executable, or real HOME is needed.

**Status.** Fixed and folded in `f7cec265`, then `b64a60e5` adopted the integrated native-name reader from `466b6ae8` instead of duplicating identifiers/private directory names. The first regression failed in three subcases; the follow-up also rejected changed Dev identifiers/private directories before correction. All 23 focused fixture/channel/wrapper/workflow tests pass. No application changeset is needed.


### RL-12 Failed Linux tool staging destroys complete prior package inputs

**P2.** Confirmed at `d4ecab4a`. Location: [linux-package-tools.mjs](../app/SiloUI/scripts/linux-package-tools.mjs), `stageLinuxPackageTools`.

**Trigger.** Prepare Linux package tools with a complete prior `runtime/linux-package/tools` directory, then encounter a missing or unreadable later source, such as the bundled `libkrunfw` library.

**Evidence.** The real staging function removes the destination before copying any source. A temporary fixture supplied five replacement executables but omitted the final library. The call rejected with `ENOENT`, and the original library no longer existed; the earlier executable copies had replaced the old bytes. The regression failed before correction. No real runtime preparation, compiler, app or network was used.

**Consequence.** Failed preparation destroys the previously usable package-tool inputs and leaves a partial replacement. Bundling can no longer use the prior complete staging directory. This is a different publication boundary from RL-08's MicroSandbox sidecar/manifest pair.

**Correction.** Copy and preserve modes in an operation-owned sibling directory, replace the published directory only after all six copies succeed, and clean the temporary directory in `finally`.

**Rejecting test.** Seed all six previous tool files and omit the last replacement source. After rejection, every previous byte must remain and no temporary staging directory may remain. Successful preparation must still preserve exact bytes and executable modes. This does not claim crash-atomic publication or protection against final destination removal/rename failures.

**Status.** Fixed and folded in `a173affb`. All three Linux package-tool tests pass; no application changeset is needed for internal preparation tooling. Failure/passing evidence is retained in `rl12-{before,after}.log` in the existing ignored verification directory.

### RL-13 macOS archive verification ignores conflicting archive roots

**P2.** Confirmed at `9707624f`. Location: [verify-release-metadata.py](../app/SiloUI/scripts/verify-release-metadata.py), macOS archive validation.

**Trigger.** Supply an updater archive containing valid `Silo.app` metadata followed by a second app root with conflicting `Contents/Info.plist`, or ambiguous/traversing archive entries.

**Evidence.** The verifier reads only the expected root's metadata and accepts the additional root. The [pinned `tauri-plugin-updater` 2.11.0 source](https://github.com/tauri-apps/plugins-workspace/blob/updater-v2.11.0/plugins/updater/src/updater.rs#L1306-L1322), inspected in the local Cargo registry at `src/updater.rs:1306–1322`, drops the first path component from every member and unpacks all of them into the same replacement tree. The second root therefore maps to the same installed `Contents/Info.plist`, although that entry was never verified. Four temporary archive subcases (second root, parent traversal, absolute path, duplicate entry) were accepted before correction. No signature or application installation was performed.

**Consequence.** The final publication gate can approve package identity for one member tree while the updater installs another member's conflicting contents. Cryptographic signing authenticates bytes but does not resolve that semantic mismatch. This differs from RL-10's missing identity fields and O-11's independent asset provenance concern.

**Correction.** Validate every archive member before inspecting metadata: one expected app root, no absolute or parent-traversing paths, and no duplicate normalized paths. Keep valid production archives accepted.

**Rejecting test.** Start with the ordinary valid production fixture, append each conflicting/unsafe entry, and require rejection before publication. Preserve successful identity verification for the single-root archive. Link-target safety and complete application execution remain separate qualification boundaries.

**Status.** Fixed and folded in `296966fc`. The four subcases failed before correction; 31 metadata/publication tests complete: 30 pass and one Linux-only Debian test is skipped. Logs are `rl13-{before,after}.log`; no application changeset is needed for release validation tooling.

### RL-14 AppImage verification accepts missing or unsupported format magic

**P2.** Confirmed at `ff51e12b`. Location: [verify-release-metadata.py](../app/SiloUI/scripts/verify-release-metadata.py), `appimage_offset` and Linux verification.

**Trigger.** Supply an ELF/SquashFS file with the expected CPU and release-info fixture but no type-2 AppImage marker, or a marker for another unsupported format type.

**Evidence.** [The upstream type-2 AppImage specification](https://github.com/AppImage/AppImageSpec/blob/master/draft.md#type-2-image-format) requires bytes `AI\x02` at offset eight. The verifier previously checked only ELF class/endianness/CPU and the SquashFS boundary. Actual verifier execution accepted all six missing/type-1/type-3 marker subcases across x86-64 and ARM64; metadata extraction was supplied by a recording adapter. No generated image was executed, signed or installed.

**Consequence.** The final verifier can label an unsupported ELF/container combination as a valid AppImage even though the file does not satisfy the format used by the supported updater path. The build job's executable extraction check remains useful, but the final publication boundary must validate the stored artifact's format independently.

**Correction.** Require the type-2 magic before extracting release-info metadata. Preserve the existing architecture and filesystem-boundary checks.

**Rejecting test.** For both supported CPUs, reject missing and unsupported markers before any metadata-extraction call. Accept type-2 fixtures and retain wrong-CPU/bad-SquashFS rejections. Header validation is not complete loader or launch qualification.

**Status.** Fixed and folded in `8346345f`. The six rejecting subcases failed before correction; 32 focused tests complete with one Linux-only Debian case skipped. Logs are `rl14-{before,after}.log`; no application changeset is needed for release validation tooling.

### RL-15 Linux packaging filters omit runtime implementations and locked inputs

**P3.** Confirmed at `d36c4914`. Location: the then-current `linux-packaging.yml`, push and pull-request path filters.

**Trigger.** Change only a runtime staging implementation imported by `prepare-microsandbox-runtime.mjs`, the locked guest image, or package/version/dependency metadata.

**Evidence.** [GitHub path filters](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpushpull_requestpull_request_targetpathspaths-ignore) select workflows only when changed paths match. Both filters include the preparation entry point and Linux tool-copy module but omit six imported runtime modules and three locked/metadata inputs. The regression walks the entry point's actual static local import graph and compares each file plus those three inputs with both positive filter lists. Eighteen event/path subcases failed. Ordinary unit/native CI still runs; no hosted job was invoked.

**Consequence.** Relevant runtime/package changes can merge without selecting the disposable Debian install/AppImage layout job, despite the workflow's claim that packaging-input changes trigger it. Nightly scheduling and release builds provide later feedback, so this is P3 rather than a missing delivery gate.

**Correction.** Add the six runtime implementations and three locked/metadata inputs to both filters, preserving the existing focused packaging selection and nightly/manual triggers.

**Rejecting test.** Every local module in the preparation entry point's current static import closure, plus guest-image and npm metadata inputs, must match both event filters. New static imports must fail this test until included. The check models the positive patterns currently used; negative filter ordering would require extending the matcher. Hosted package installation remains separate qualification.

**Status.** Fixed in the accompanying commit. The eighteen rejecting subcases failed before correction; the path-selection test and relevant workflow/fixture tests pass afterward. Logs are `rl15-{before,after}.log`; no application changeset is needed for CI selection.

## Other reviewed boundaries and limits

[release-platform.yml](../.github/workflows/release-platform.yml) compiles production bundles under `src-tauri/target/release-compile/<target>/release/bundle`. The macOS finalizer receives the app at that path; Linux packaging copies the single expected Debian/AppImage outputs to fixed release asset names. [publish-release.py](../app/SiloUI/scripts/publish-release.py) maps `darwin-aarch64`, `linux-x86_64` and `linux-aarch64` to those updater assets. The inspected producer/consumer paths agree; no bundle was generated or opened in this pass.

[package-macos-release.py](../app/SiloUI/scripts/package-macos-release.py) finalizes and verifies the app's VM signature policy before regenerating its `.app.tar.gz` and DMG. It signs the regenerated updater archive, and [verify-release-signatures.py](../app/SiloUI/scripts/verify-release-signatures.py) independently verifies each signature with the committed production public key. The final publication gate also verifies asset membership, checksums, package metadata and feed mappings. The existing O-11 draft-asset trust finding remains excluded; these observations are not independent build provenance or live updater proof. See the upstream [Tauri CLI](https://v2.tauri.app/reference/cli/) and [updater requirements](https://v2.tauri.app/plugin/updater/).

Signing secrets are scoped to packaging/signing steps; synthetic native-test configuration is separate. Public runtime/dependency cache exporters validate allowed archive members and reject application/private build outputs in the exercised fixtures. [ci.yml](../.github/workflows/ci.yml) runs ordinary frontend/script/native checks; the then-current `linux-packaging.yml` installed/removed a disposable Debian package and inspected AppImage tool layout. This pass reviewed their source and workflow-pin checks without executing hosted jobs or package installers. Passing packaging does not establish KVM or installed-app behavior.

The host-name scan covered `.silo`, production identifiers/Keychain services, `silo-remote`, and Silo editor profile names across native sources and scripts. Runtime aliases, editor/remote private directories, Keychain services, bridge links and autostart identities use `channel.rs` in the inspected native consumers. The shared `silo-remote:` URL protocol, `.silo-backup` format suffix, guest policy markers and fixture expectations are separate contracts. Owner-invoked production-settings import intentionally handles both channels and was not executed. RL-02, RL-03, RL-09 and RL-11 record concrete fixture-channel failures found by this scan. The integrated script-name reader now executes the native channel API without building the application; its mutation test follows renamed Dev fields.

No full Linux WebDriver/GNOME run, real Debian install, signed version-to-version updater, bundled VM execution, SIP-enabled macOS constraint run or replacement-runner qualification was performed. No native application was launched and no production data was used. Rust verification was formatting only plus the small channel-name exporter exercised by Python tests. Full `test:release`, macOS signing tests and full Python discovery were deliberately not used because this task prohibits signing, tagging and heavy/live work. No actionlint or complete hosted workflow execution was performed; extracted workflow shell commands and pinned-action assertions were tested.

## Verification and reproducibility

RL-01: `PYTHONPATH=app/SiloUI/scripts python3 -m unittest test_desktop_release` passed 8 tests. The new regression first failed in 3 subcases. Failure/passing output is retained under ignored `app/SiloUI/src-tauri/target/verification/release-review-2026-10-02/rl01-{before,after}.log`.

Before the fix commit: Node 24.11.1 `typecheck` passed; `lint` passed with 12 existing warnings; `cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check` passed. Native checks use the shared `/tmp/silo-codex-target`; Node dependencies use the existing main-checkout installation. A fixture or source review does not qualify a signed release or installed application.

RL-02: `PYTHONPATH=app/SiloUI/scripts python3 -m unittest test_linux_verification_release test_desktop_release test_workflow_pins` passed 12 tests. Typecheck, lint (5 existing warnings after integration updates), and Rust formatting passed before its fix commit. `rl02-{before,after}.log` retains the failing/passing evidence in the same ignored directory.

RL-03: the same focused command passed 13 tests. Lint (2 existing warnings) and Rust formatting passed. Typecheck at `60c1b862` failed on two stale `ComputerUseProvider` imports introduced by concurrent integration changes; the next integration fold supplied corrected imports. The failure preceded the fix commit and did not involve the changed Python files. Typecheck is rerun after integration. `rl03-{before,after}.log` retains the focused regression evidence.

RL-04: `python3 -m unittest test_publish_release test_release_publication test_release_workflow test_workflow_pins` (with the scripts directory on PYTHONPATH) passed 33 tests; Node 24.11.1 `node --test app/SiloUI/scripts/release.test.mjs` passed 27. Typecheck, lint without warnings, and Rust formatting passed before commit. Logs prefixed `rl04-` retain failures and passing results. The initial Python diagnostic omitted synthetic GH_REPO and failed fixture setup; the corrected diagnostic reached the unexpected mocked GitHub calls. An intermediate shell regression caught macOS Bash 3's empty-array/nounset behavior; positional arguments replaced that array before the final pass.

## Final verification cutoff

Final fixture/check results below apply to code at `8e497736`, plus the report/documentation completion. Output is retained under ignored `app/SiloUI/src-tauri/target/verification/release-review-2026-10-02/`.

| Check | Result |
| --- | --- |
| Python 3.12 focused release, cache, wrapper, fixture, identity and workflow suite | 114 tests: 113 passed, 1 skipped (`dpkg-deb` unavailable) |
| Node 24.11.1 positive selection of stable-publication adapter regression | 1 passed; real tag-creation fixture excluded |
| Vitest runtime staging/cache regressions (`src/test/microsandbox-runtime.test.ts --maxWorkers=1`) | 13 passed |
| `npm --prefix app/SiloUI run typecheck` | Passed |
| `npm --prefix app/SiloUI run lint` | Passed without warnings |
| `CARGO_TARGET_DIR=/tmp/silo-codex-target cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check` | Passed |
| `git diff --check`; relative-file targets in both edited documents | Passed; this checks files, not web availability or every heading anchor |

The Python command was `PYTHONPATH=app/SiloUI/scripts python3.12 -m unittest test_desktop_release test_linux_verification_release test_linux_desktop_services test_channel_names test_publish_release test_release_publication test_release_workflow test_release_metadata test_release_bundle_retry test_release_runtime_transfer test_release_cache test_release_dependency_identity test_release_dependency_cache test_release_dependency_integration test_workflow_pins`. Logs are `final-python-cutoff.log`, `final-node-focused.log` and `final-runtime-cutoff.log`. Compiler/build messages inside cache fixtures come from fake external adapters; no application compilation occurred.

RL-11's original failure log is named `rl10-before.log` because integration assigned RL-10 to the concurrent package-identity finding before this branch folded. Its passing output is `rl11-after.log`; the native-name/default follow-up retains `channel-followup-{before,after}.log`.

The full Node adapter suite passed 27 tests in earlier runs but created unsigned fixture tags. The attempted negative exclusion filter also ran that test; it was not a valid filtered verification. The final positive selection `node --test --test-name-pattern='explicit stable publication passes' app/SiloUI/scripts/release.test.mjs` passed exactly one test without running the real Changesets tag fixture. Messages about pushing/dispatching in this suite come from fake command adapters; no real push or publication occurred.

The ledger includes three independently authored concurrent findings and their integration fixes. Report commits were folded from the initial scaffold onward, including conflict resolutions that preserved both authors' findings. None of these internal tooling/documentation changes needs an application changeset. Versions were not bumped in the task repository, changesets were not consumed there, and no real release operation was performed.

## Continuation review

The requested twenty-minute continuation started at 17:21 UTC. Integration was merged before reviewing. RL-06 was rechecked at `d4ecab4a`: the required job still uses `macos-14`, and no qualified replacement environment is present. It remains open for the same concrete infrastructure dependency; no gate was bypassed. Continuation fixture checks are recorded separately from the original `8e497736` cutoff.

## Next action

Qualify a maintained macOS 14 runner before the first announced brownout on October 5 at 14:00 UTC, retaining the blocking minimum-version constraint gate and recording its OS identity and results. RL-06 remains open until that evidence exists.
