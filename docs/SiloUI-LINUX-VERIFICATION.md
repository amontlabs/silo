# Linux verification

Linux parity needs separate evidence for each layer. Compilation alone does
not prove the WebKit UI, desktop services, or hardware virtualization works.

| Layer | Command | What it proves |
| --- | --- | --- |
| Frontend | `npm test -- --maxWorkers=2` | Component behavior and bridge contracts on Linux; test adapters remain outside production. |
| Native | `cargo test --manifest-path src-tauri/Cargo.toml --locked` | Linux application discovery, login entries, settings, resource checks, files, network, GitHub boundary behavior, secrets, and backup validation. |
| Desktop | `xvfb-run -a dbus-run-session -- python3 scripts/test-linux-desktop.py` | Real WebKit and native IPC in the Dev app, dependency failure gating, page navigation, inline validation and persisted settings. |
| GNOME integration | `sh scripts/test-linux-gnome.sh` | Real GNOME Wayland, tray reopen/quit, native backup picker and visible notification delivery. |
| Hardware | `python3 scripts/test-linux-runtime.py` | Real KVM creation, bundled image import/cache reuse, guest tools/identity, backup/restore data round trips, live secret changes and interrupted restart recovery. |

Run from `app/SiloUI`. Hardware tests use temporary Silo runtime directories and
synthetic secret material. They do not touch existing computers. Desktop tests
use a temporary HOME, temporary XDG directories and a private D-Bus session;
their saved settings fixture is a file owned by the test, with no hooks or
fixtures in the application UI.
Authenticated GitHub workflow testing is separate and requires explicitly scoped
test repositories and credentials; these Linux CI jobs never receive credentials.

## Preparation

Use native Ubuntu 24.04 ARM64 or x86-64 with Node 24, Python 3.11 or newer,
Rust 1.94.0 and Go 1.25 for cold runtime preparation:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends build-essential pkg-config libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libdbus-1-dev libclang-dev libcap-ng-dev cmake webkit2gtk-driver xvfb xauth dbus-x11 python3-selenium
cargo install tauri-driver --version 2.0.6 --locked
npm ci
npm run runtime:prepare
```

Set the three build configuration variables to synthetic values for these tests:
`SILO_GITHUB_APP_SLUG=silo-linux-test`, `SILO_GITHUB_CLIENT_ID=test-client`,
`SILO_GITHUB_CLIENT_SECRET=test-secret`. Never copy a developer's ignored
`github-build.local.json` into a test machine. Build the Dev app with
`npm run desktop:build -- --debug --no-bundle --ci` before desktop testing.
The [build wrapper](../app/SiloUI/scripts/build_desktop.py) selects the development
identity for `--debug`; the [desktop harness](../app/SiloUI/scripts/test-linux-desktop.py)
defaults to that identity and `src-tauri/target/debug/silo-ui`. If Cargo writes to
another target directory, set `SILO_LINUX_APPLICATION` to the absolute path of
the freshly built Dev executable before running the harness.
For memory-limited machines, set `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0` and bound `CARGO_BUILD_JOBS`.
Do not run Cargo tests concurrently with desktop builds in the same target
directory: a test build can replace the runnable debug executable with a build
that expects the development server. Always rebuild immediately before UI tests.

The test-only `linux-verification.yml` workflow runs both architectures. It has
read-only repository permissions and cannot publish packages or releases.
Evidence and screenshots are stored under ignored `app/SiloUI/test-results/linux/`.
It restores the patched MicroSandbox build from the cache that
`warm-release-caches.yml` saves from `main`, and leaves unit tests and lint to
`ci.yml`. It runs on pull requests that change the app, on `main`, nightly and
on manual runs. The AMD64 job runs real KVM tests after successful build and
desktop checks on `main`, nightly and manual runs; pull requests skip them.
ARM64 hosted runners have no `/dev/kvm`, so that CI hardware step is
explicitly skipped; the separate local Lima hardware evidence below covers
ARM64. The hardware script itself exits nonzero when KVM is unavailable,
rather than marking an unexercised computer workflow successful.

## Coverage limits

An OrbStack Linux machine without `/dev/kvm` can run native and WebKit tests but
cannot prove MicroSandbox lifecycle behavior. CPU emulation does not supply KVM.
The script checks the actual KVM API and creates a VM handle before running
hardware tests; checking for the device file alone is insufficient.

GitHub-hosted Ubuntu offers Android hardware acceleration, but general nested
virtualization is not a guaranteed service. The workflow probes the actual host
instead of assuming it is available. If a runner lacks usable KVM, that
architecture still needs a Linux KVM host before claiming full runtime parity.

Xvfb alone does not prove desktop services. Separate GNOME 46 X11 and native
Wayland runs now prove the UI; the Wayland run also proves tray reopen/quit,
a visible production notification and the native backup picker. These sessions
use nested software rendering. Physical GPU behavior, HiDPI and multiple
monitors, KDE/other desktop environments, and every external terminal/editor
application remain unverified. Do not generalize the GNOME evidence to every
Linux desktop.

## Recorded local evidence, 10 September 2026

An isolated Ubuntu 24.04 ARM64 OrbStack machine ran the following successfully:

- Bundled MicroSandbox, Git and guest image preparation, followed by the real
  production Tauri debug build. The first runtime link failed because
  `libcap-ng-dev` was missing; installing it fixed the build. Both Linux build
  workflows now install it, and Linux packages declare `libcap-ng0` / `libcap-ng`.
- 608 frontend tests in 65 files. At that stage, the deliberate 64-card capacity interaction
  received a 15-second timeout; its previous 5-second timeout failed in the full Linux
  suite but the isolated behavior passed. No global timeout was increased.
- 291 native tests and 5 build-configuration tests after the final recovery changes; 10 opt-in hardware/account
  tests excluded from the default native run.
- Eleven real WebKit assertions: first-run onboarding, truthful missing-KVM
  failure and disabled Continue, dependency retry, five main page routes,
  secret inline validation and Escape, actual XDG login enable/disable, and
  persisted settings after full native application quit/relaunch. The final
  integrated sources were rebuilt after the final native run and all eleven
  WebKit checks passed again; evidence is under `test-results/linux/arm64-final/`.

OrbStack explicitly failed the hardware probe with missing `/dev/kvm`.
A separate disposable Lima VM provides the hardware evidence below. The GitHub-hosted two-architecture workflow
was pushed to the approved public `verify-linux-parity-20260910` test branch.
The first hosted run exposed two long form/navigation tests exceeding their
5-second limits; only those tests receive 15 seconds. It also confirmed hosted
ARM64 has no KVM and that changing the primary group through `sudo -g` prompts
for a password. The AMD64 hardware step uses `sudo runuser` with the existing
KVM group, without broadening device permissions. Screenshots are `test-results/linux/onboarding.png` and
`settings.png`; detailed success/failure reports are `desktop.json` and
`runtime.json` in the same ignored directory.

### Additional architectures and real KVM

An existing local Ubuntu 24.04 AMD64 OrbStack machine runs through Rosetta on
Apple Silicon. The complete frontend suite passed: 609 tests in 65 files.
Its patched MicroSandbox, bundled Git and AMD64 Ubuntu guest image also staged
successfully, and the final native suite passed 291 tests plus 5 build-configuration
tests (10 opt-in tests excluded). The production Tauri debug build and all eleven
WebKit assertions then passed, including real XDG autostart and full native
quit/relaunch persistence. Evidence is under `test-results/linux/amd64-final/`.
The emulated frontend run used `--testTimeout=30000 --maxWorkers=2` after five tests hit
5-second time limits under CPU contention. This changes only that test command,
not application behavior or the default test configuration. The first AMD64
WebKit run reached the correct app but clicked during a transient layout state;
the existing bounded semantic-click wait now also retries WebDriver
`ElementNotInteractableException`. It still uses normal clicks and fails after
the same 45-second deadline.

For real hardware tests, a temporary Lima 2.2.0 Ubuntu 24.04 ARM64 VM uses Apple's
Virtualization framework with `nestedVirtualization: true` on an M4 Max host.
It has 4 CPUs, 8 GiB RAM and a 24 GiB sparse disk. Its only shared directory is a
read-only export of synthetic test binaries and the bundled public guest image;
no host home, credentials or existing sandbox data are shared. The normal user
runs tests with the existing `kvm` group. Device permissions remain unchanged.
Both KVM API version 12 and actual VM-handle creation succeeded.

The Linux test executable was built from the final integrated native sources
with synthetic GitHub configuration. Each hardware test runs independently with
`--ignored --nocapture --test-threads=1`, a ten-minute limit, and an assertion that
exactly one test passed. The executable and bundled MicroSandbox firmware are
copied into the temporary VM; the compiled guest-image path points to its
read-only export. Its evidence output directory is writable by the test user.
An initial backup test completed every disk assertion but failed writing its
final evidence because that directory belonged to root; the complete test passed
after correcting only this temporary directory's ownership.

- [Apple nested virtualization](https://developer.apple.com/documentation/virtualization/vzgenericplatformconfiguration/isnestedvirtualizationsupported):
  supported Apple Silicon hardware can expose virtualization to a Linux guest.
- [Lima Virtualization framework](https://lima-vm.io/docs/config/vmtype/vz/) and
  [Lima configuration](https://raw.githubusercontent.com/lima-vm/lima/master/templates/default.yaml):
  `nestedVirtualization` enables the supported host capability.
- [OrbStack Linux machines](https://docs.orbstack.dev/machines/): local Linux
  machines and AMD64 execution through Rosetta on Apple Silicon.

All five real KVM tests passed in this VM:

| Test | Result |
| --- | --- |
| Bundled image import, boot and cache reuse | Passed, 10.96 seconds |
| GitHub guest tools and live Git identity | Passed, 27.50 seconds |
| Backup/restore root and workspace without original VM/cache | Passed, 80.70 seconds |
| Live secret changes with the same guest boot | Passed, 56.63 seconds |
| Worker exit, restart recovery and no duplicate restart | Passed, 17.49 seconds |

These tests use real MicroSandbox guests. They do not exercise authenticated
GitHub API requests; that evidence belongs to the separate GitHub test lane.
Full logs and a result JSON are saved locally under
`test-results/linux/kvm-arm64/`. The temporary Lima VM is deleted after evidence
capture. The existing OrbStack machines and macOS application remain untouched.

## Hosted verification, 10 September 2026

[Run 34480478578](https://github.com/0xpolarzero/silo/actions/runs/34480478578)
passed both native architectures at commit `6e32bcf` on the approved
`verify-linux-parity-20260910` branch. It publishes test artifacts only.

| Hosted runner | Frontend | Native | Production WebKit | Real KVM |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04 ARM64 | 609 tests / 65 files | 291 tests + 5 build checks | 11 assertions | Explicitly skipped: runner has no KVM; see five local ARM64 KVM passes above |
| Ubuntu 24.04 AMD64 | 609 tests / 65 files | 291 tests + 5 build checks | 10 assertions | All five real guest tests passed |

Both also passed lint, patched runtime/Git/guest image preparation and the
production Tauri build. Native suites exclude 10 opt-in hardware/account tests;
the separate AMD64 hardware step explicitly runs the five applicable guest
tests. AMD64 WebKit has one fewer assertion because KVM exists, so the
missing-KVM blocking assertion does not apply.

| Real AMD64 KVM test | Duration |
| --- | --- |
| Bundled image import, boot and cache reuse | 12.72 seconds |
| GitHub guest tools and live Git identity | 21.16 seconds |
| Backup/restore root and workspace without original VM/cache | 85.75 seconds |
| Live secret changes with the same guest boot | 27.50 seconds |
| Worker exit, restart recovery and no duplicate restart | 18.50 seconds |

Downloaded artifacts and complete job logs are stored under
`test-results/linux/ci/arm64/` and `test-results/linux/ci/amd64/`.
The AMD64 `runtime.json` records usable KVM and five passing tests; each detailed
log contains an actual one-test pass. This is guest execution evidence, not
only a capability check. Later GitHub-account tests and GNOME-only test tooling
commits are separate from this pinned hosted run.

## GNOME desktop integration

Ubuntu 24.04 ARM64 with GNOME 46 passed the eleven UI assertions on both GNOME
X11 and native Wayland. The final Wayland run passed sixteen assertions:
the same eleven plus real notification-service ownership, AppIndicator
registration and reopening a closed window, native backup file-picker Cancel,
a visible notification from a production health failure, and the real tray Quit
action removing both the window and tray item. The test checks native window
visibility before and after Open; notification visibility belongs to GNOME Shell.
It does not substitute fake desktop services or add production test hooks.

Install these additional **test-machine** dependencies after the normal Linux
preparation above, then run against the freshly built production binary:

```sh
sudo apt-get install -y --no-install-recommends gnome-shell gjs gnome-shell-extension-appindicator xdg-desktop-portal-gnome python3-pyatspi xdotool
PATH="$HOME/.cargo/bin:$PATH" sh scripts/test-linux-gnome.sh
```

The launcher creates temporary XDG state and a private D-Bus session. GNOME runs
as a nested compositor with software rendering; Silo explicitly uses its native
Wayland socket. It removes its own processes and private portal mount after the
run. Existing desktop sessions, app data and computers remain untouched.

Evidence is under `test-results/linux/gnome-wayland/`: `desktop.json` records
all sixteen passes, `gnome-services.json` records the five service assertions,
`gnome-notification.png` shows the real banner, and the D-Bus/GNOME logs record
delivery. All test-owned Silo, GNOME and WebDriver processes were removed.

## September 2026 x86-64 checkpoint migration qualification

An isolated Ubuntu 24.04 KVM container on the devbox host used a genuine
MicroSandbox 0.6.17 Silo guest, migrated it through the production 0.7.2
application, and exercised the packaged AppDir binary with SHA-256
`42b4a7bf4d3c7c3a66a47fe2702f2eb4da370d9f209a85e356d605dce65333c1`.
The core production WebKit run passed eleven lifecycle checks, including
explicit Start, full checkpoint, stopped fork, RAM/process survival, restore,
recovery fork, disk rollback and host-side isolation. Evidence is in the
task-owned container at `/work/evidence/lifecycle-full-final5/lifecycle.json`.

The production archive command wrote the v3 file
`/work/archives/Silo-Backup-1790360984.silo-backup` (922,837,009 bytes).
Production import created a stopped VM. Its explicit Start cold-booted the
captured disk, and a guest probe verified the original workspace marker and
absence of post-checkpoint source/fork files. Evidence is
`/work/evidence/resume-imported-v3-final/lifecycle.json`. The overall archive
continuation **did not pass**: a second export of the source failed while
writing its self-contained snapshot with
`snapshot identity snap_a6253d64fbdc953f3dfd3eb4d730a211 has 5 local copies; use group:member or an explicit artifact path`.
The source and imported VM were left stopped. Native GTK folder-picker
automation was not verified; archive export/import used real production Tauri
commands in the main WebView, with no archive mocks.

The tested Silo flow's repeat export failed with the ambiguous-ID error above.
Inspection of pinned MicroSandbox 0.7.2 source explains a possible integration
failure but does not establish an upstream contract violation. The [runtime
inputs](../app/SiloUI/runtime-inputs.json) pin
source commit `60d4dc8a436fb9365491567ec21d073e924e3c6d`. In that source,
`sdk/rust/lib/backend/local/snapshot/lineage.rs` persists a snapshot ID for the
source cursor; `snapshot/archive.rs::resolve_parent_artifact` prefers a
same-group sibling parent before global lookup; and
`snapshot/store.rs::lookup_by_digest` rejects multiple local copies of one
snapshot ID. Silo creates each export capture in a fresh group, while the
inspected resolver has explicit same-group handling. The supported contract
between these behaviors is not established. The qualification did not run an
independent MicroSandbox-CLI-only reproduction or inventory the five copies;
earlier failed imports may have contributed duplicates. Thus a single clean
import followed by repeat export has not been proven to fail. The release gate
remains open for this Silo/MicroSandbox integration behavior. No snapshot index
or lineage record was hand-edited.

### September 26 follow-up

The native GTK folder chooser was subsequently exercised successfully. It
selected `/home/siloqa/silo-linux-ui-qualification-luna/ui-run-20260926-short-home/home/backups`
and exported a source-only v3 archive of SHA-256
`73791219b0b3fe2ecfed8a323480b96a4fa9c548d0692c214b680abdb432b2e1`; the
accepted-folder screenshot is retained as
`native-folder-picker-accepted.png` under the isolated guest's `evidence/`
directory. This proves chooser acceptance and one source-only export, not an
import or repeated export.

The portable-image-cache fix was exercised in a fresh XDG/MSB_HOME: import
registered a stopped VM, its regenerated VMDK extents referenced the new
destination cache, and explicit Start plus `qemu-img` verified the 40 GiB root.

The authentic x86 predecessor then passed production migration and lifecycle
qualification with the runtime-7 package SHA-256
`f741e941c7d215835282ab7159b7ae32cef50bfba51311aceb5593ed9a4b1189`. Migration
completed 1/1 and the converted VM appeared stopped in the real overview.
Explicit Start/Stop preserved `/workspace/sentinel` with bytes
`legacy-source-before-checkpoint\n` and UID:GID `1001:1001`. Full checkpoint,
fork, restore, disk independence, RAM/process survival, and recovery assertions
passed before the helper stalled at its GTK chooser step. The GTK chooser had
already passed separately: it selected the backup folder and wrote a valid
source-only archive, SHA-256
`73791219b0b3fe2ecfed8a323480b96a4fa9c548d0692c214b680abdb432b2e1`.

The same-home export matrix passed with exactly one seed export, one import,
one stopped fork, and two exports each from source/import/fork. All seven
archives including the seed verified; the source group and imported group
remained stable after app relaunch. Each VM retained 2 CPU, 2048 MiB RAM, and
an 8 GiB root; `qemu-img` reported 8,589,934,592 bytes for each raw root image.
Evidence is
`app/SiloUI/src-tauri/target/verification/x86-legacy-final7-matrix-20260926/evidence/snapshot-groups-result.json` (untracked local evidence).

The guest desktop service's exact stale `:1` lock/socket recovery passed 35
focused tests. A later X11-directory normalization fix passed 40 focused
tests, including preservation of a healthy live supervisor, and its x86 guest
package passed live recovery from the verified dead-display state. The
September 27 production desktop run preserved an unsaved Mousepad buffer
through full checkpoint, stopped-fork Start, and source restore; its package
and evidence are recorded in the Linux acceptance research note. The positive
live remote-viewer gate passed on the later x86 AppImage described below. The final
eight-patch runtime passed ARM64 packaging and live port-control tests. The
AppImage rebuilt after the guest script fix has SHA-256
`f32c1049cb53fb28d8767ae12e6db8512a3f316f60746a227fc81bb8ed369a61`; tested
`msb` SHA-256:
`f552ad75296c8964b7ed974e1fafac2533374e4caefe2c09dc34321c250ba06b`;
runtime-input manifest SHA-256:
`be2b4f29fb21faf08df79ddae12c5b070b762f7ad79588fe07b0d4dd8efc35fd`. Live
proof covers positive/denied/absent probes, 128-port enforcement, occupied
host-port conflict preservation, established relay closure, and immediate
port reuse. Evidence is in
`app/SiloUI/src-tauri/target/verification/native8-live-ports/` (untracked local evidence).
The initial patch8 transfer to the x86 build host was rejected by automatic
review. The user later authorized the exact transfer, and the x86 native8
runtime was built and exercised in the task-owned Ubuntu 24.04 guest. Its live
port proof passed positive, absent, and ingress-denied probes; denied-listener
behavior; host-port-zero allocation; idempotence and conflict preservation;
the 128-port cap; established relay closure after removal; and exact-port
republish with new traffic. The guest was stopped and mappings removed.
Evidence is `app/SiloUI/src-tauri/target/verification/native8-live-ports/x86-live-ports.json` (untracked local evidence)
(SHA-256 `b6991e628ae18875057dac7c043caace1f6c357bad145f052137c2312d29935d`);
the tested `msb` SHA-256 is
`285d0bb9a67dcef45e78fe4e4f2bc1608fa4ae6e3532053461c54a8878be2fe2`.
The final AppImage package-content and dependency smoke is still in progress;
this live runtime proof alone does not establish package verification or
release readiness.

## Primary sources

- [Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/): external
  `tauri-driver` can drive native Linux WebKit without adding an embedded server
  or test plugin to the application.
- [Tauri WebDriver CI](https://v2.tauri.app/develop/tests/webdriver/ci/):
  `webkit2gtk-driver` and Xvfb provide native Linux browser automation.
- [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners):
  Linux hardware acceleration and runner characteristics.
- [GitHub-hosted runner limits](https://docs.github.com/en/actions/concepts/runners/github-hosted-runners):
  nested virtualization is not officially supported.
- [Linux KVM API](https://docs.kernel.org/virt/kvm/api.html): `KVM_GET_API_VERSION`
  and `KVM_CREATE_VM` verify usable hardware virtualization.

- [GNOME nested Wayland testing](https://wiki.gnome.org/Initiatives%282f%29Wayland%282f%29GnomeShell%282f%29Testing.html):
  a private D-Bus session can run a nested GNOME Wayland compositor.
- [Ubuntu AppIndicator extension](https://github.com/ubuntu/gnome-shell-extension-appindicator):
  the real GNOME extension supplies StatusNotifierItem support.
- [GNOME notifications](https://help.gnome.org/gnome-help/shell-notifications.html):
  desktop banners and the notification list are owned by GNOME Shell.

## Patch8 ARM64 package and live network proof, 26 September 2026

A task-owned Ubuntu 24.04 ARM64 Lima VM built the final 0.7.2 MicroSandbox patch series with `net,ssh,embed-binaries` and staged it through `stageRuntime`. The app checkout was compared against a hash-only manifest of 557 current source and input files before the supported `npm run desktop:build -- --bundles appimage` command; all 557 matched. The manifest excluded private GitHub configuration, credentials, generated targets, `node_modules`, and runtime output. No x86 build or app package is implied by this ARM result.

The initial `Silo_0.9.0_aarch64.AppImage` SHA-256 was `4c6aee7c1352611ee931ca2a747186e88c7ebe58d09c31aaa4a15b93ec3c8ae3`. Extraction showed the packaged MicroSandbox executable SHA-256 `f552ad75296c8964b7ed974e1fafac2533374e4caefe2c09dc34321c250ba06b`, matching the binary exercised against the live task guest. The packaged runtime manifest SHA-256 was `8d4c9b61cffb3d3bee8096d2c4684ff901f89f59f35a995e6394e41ef3ddd41d`; it records all eight patches and patch8 SHA-256 `ad1abf1973c7e542ec7015575ab4ad35b321ac434e2bb9049aef096fa6b2b012`. Packaged MicroSandbox, Git, Git LFS, Git remotes, and libkrunfw matched their manifests. MicroSandbox, Git, and Git LFS version commands ran; `ldd` found no unresolved dependencies for the app, MicroSandbox, or Git, and Git LFS is static. Evidence: `app/SiloUI/src-tauri/target/verification/native8-live-ports/arm64-package-smoke.json` (SHA-256 `5c17ceb46e35a43481ad46bf7ceafccd7eeca9de5fdecfc67555311d413b3041`).

The isolated live guest used the same final8 MicroSandbox binary and confirmed positive, absent, and ingress-denied direct TCP probes; a denied published listener; exact mapping preservation after an occupied-port error; the 128-publication limit; an established echo relay closed after `port_remove`; and immediate exact-port republishing with new echo traffic. The client checked EOF/reset after receiving the remove response. The native implementation waits for its owned relay task to stop before sending that response. All test mappings were removed and the guest stopped gracefully. Evidence: `app/SiloUI/src-tauri/target/verification/native8-live-ports/arm64-live-ports.json` (SHA-256 `f675e4dbf1e98c4a77c697a02709a9b822ab2891b479e59795513a8d330faefb`). This proves native port behavior and package contents; it does not exercise the Network panel in an installed AppImage.

The first patch8 transfer to the x86 build host was rejected by automatic review. The user later authorized that exact transfer; the resulting x86 runtime8 live proof and final AppImage verification are recorded below. The earlier rejection is historical, not a remaining gate.

## Patch8 x86-64 package and live network proof, 27 September 2026

The final x86 AppImage SHA-256 is
`58a0516b396632390b9637966219ff732b577810fcf18483dcc9cbb1409d804a`.
Its extracted application executable SHA-256 is
`e2d23bb113a5d10f7ddf93d89b95a5cc3e6bf4839a31e2316dd8385628a6cad0`; the
embedded MicroSandbox SHA-256 is
`285d0bb9a67dcef45e78fe4e4f2bc1608fa4ae6e3532053461c54a8878be2fe2`.
The staged runtime manifest SHA-256 is
`d0c5dbf0c10d0d04eddf4f08f2d1957b7c6b9407170d58e327820a9b355875ad` and
records all eight patches. The package smoke verified the embedded payload
hashes, MicroSandbox 0.7.2 and all five Silo protocol probes, Git/Git LFS
versions and hashes, and dynamic dependencies for the app, MicroSandbox, and
Git. The preserved package report is
`app/SiloUI/src-tauri/target/verification/native8-final-package/package-smoke.json` (untracked local evidence)
(SHA-256 `d7b44744a6b363237293a6182d4b39784fccfc0390c08f4f785ba265baf68d04`);
the verified AppImage is stored beside it. The final x86 live port result is
`app/SiloUI/src-tauri/target/verification/native8-live-ports/x86-live-ports.json` (untracked local evidence)
(SHA-256 `b6991e628ae18875057dac7c043caace1f6c357bad145f052137c2312d29935d`);
it covers allowed, absent, and ingress-denied probes, bind conflicts and
mapping preservation, the 128-port cap, relay closure on removal, and
successful exact-port reuse. The guest was stopped and mappings removed.
These checks ran on an Ubuntu 24.04 x86-64 guest with nested KVM under an
Ubuntu 26 host; they are not bare-metal coverage. Release signing and
distribution remain outside this qualification.

After the root-owned X11-directory normalization fix, the same task-owned ARM64
checkout rebuilt the AppImage using the supported command and existing native8
cache. The new AppImage SHA-256 is
`ae027cf42f654d63afcba238bf8e8172c6ee5633e759adc828e9be04802a44f6`.
The extracted app executable contains the exact full
`desktop-service.py` source bytes with SHA-256
`735bd1ee3acda327c46fe031953f2e399d8f3c9a9a1008a234f08e6d7de035dc`.
Its packaged MicroSandbox SHA-256 remains
`f552ad75296c8964b7ed974e1fafac2533374e4caefe2c09dc34321c250ba06b`,
the earlier live-tested binary. The eight-patch runtime manifest SHA-256 remains
`8d4c9b61cffb3d3bee8096d2c4684ff901f89f59f35a995e6394e41ef3ddd41d`.
Packaged Git support hashes, version commands, and unresolved-dependency checks
passed. Evidence is
`app/SiloUI/src-tauri/target/verification/native8-live-ports/arm64-package-x11-fix.json` (untracked local evidence)
(SHA-256 `be7686f86d6333cf6cd11ba091983afe077248834ca85cb3993bc02aaf7c68af`).
This is a package-content and dependency check; it did not rerun installed-app
UI or live VM behavior.

The remote desktop bridge then fixed a deterministic framed-response deadlock by
flushing its buffered writer. The new regression failed before the fix and
passed after it; the focused stream suite passed 4/4. The ARM64 app was rebuilt
from `remote.rs` SHA-256
`21249ff78fecbd23aef9293c44bb2cabe4534a3578a5aa2e1f062a285b00b5a9`
and the unchanged guest service source above. That AppImage SHA-256 is
`9be614fb8dcb33148bd791da698ada6ab46ae8d31e8e26c94f8abc26db99e515`.
Its app executable differs from the preceding package and embeds the exact
guest service script. Its packaged MicroSandbox, eight-patch manifest, and
libkrunfw hashes remain unchanged. Packaged Git support hashes, tool version
commands, and unresolved-dependency checks passed. Evidence is
`app/SiloUI/src-tauri/target/verification/native8-live-ports/arm64-package-remote-flush.json` (untracked local evidence)
(SHA-256 `b7f82ece5a891035f5c62e99a1123c7f5b2b72a888d065103dfe2828c8112a12`).
This ARM rebuild does not itself prove the remote desktop UI flow; the x86
production result is recorded below.

The final binary-stream flush then made short SSH data visible before newline
or stream closure. Its regression failed before the fix with a 500 ms
`WouldBlock` while input stayed open; the focused stream suite passed 5/5 after
the fix. The ARM app-only rebuild used `remote.rs` SHA-256
`9502f606d7de1c0e66b8fcbecf2bc3328a99f35719a82821193bbcf06b37d88d`
and the unchanged native8 runtime. The final ARM64 AppImage SHA-256 is
`f32c1049cb53fb28d8767ae12e6db8512a3f316f60746a227fc81bb8ed369a61`;
its executable differs from the preceding package and still embeds the exact
guest service source. The packaged MicroSandbox SHA-256 remains
`f552ad75296c8964b7ed974e1fafac2533374e4caefe2c09dc34321c250ba06b`,
and its eight-patch manifest SHA-256 remains
`8d4c9b61cffb3d3bee8096d2c4684ff901f89f59f35a995e6394e41ef3ddd41d`.
Packaged Git support hashes, tool version commands, and unresolved-dependency
checks passed. Evidence is
`app/SiloUI/src-tauri/target/verification/native8-live-ports/arm64-package-binary-flush.json` (untracked local evidence)
(SHA-256 `7b7ee39f14e632ec9daf7f841673fee4ca26bfe32b216ae1a7af9753fdeb3947`).
This package check did not rerun installed-app UI or live VM behavior.

The x86 production remote-viewer attempt made before the final raw-stream
flush fix returned no SSH bytes after 25 seconds. The subsequent raw-stream
flush regression and focused stream suite passed 5/5. The final x86 AppImage
SHA-256 `8079943262a6a70e007403aa3900d1fd857080d63a04ea8dc4fb700dc5c7d8b2`
passed the live remote gate: pinned-key SSH authenticated and exited 0, the
running source viewer connected, and opening stopped fork
`54e8b201-7e30-44e1-b866-855b23fb7d4a` left it stopped. The controller
executable SHA-256 is
`96ffb0bad56f0e655d2072a7d9ad7bf987c83961292b5c62d84f5d437d671465`. Evidence
is in `app/SiloUI/src-tauri/target/verification/x86-final-desktop-20260927/remote-final/` (untracked local evidence)
(`final-binary-remote-viewer-result.json`, running/stopped viewer screenshots,
and `final-binary-flush-real-ssh.log`). The earlier patch-transfer rejection
was superseded by explicit user authorization. The final x86 runtime-8 live
port proof is recorded above; final AppImage package verification remains
pending.
