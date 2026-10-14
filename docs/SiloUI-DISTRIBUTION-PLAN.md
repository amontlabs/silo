# Distribution and update acceptance

> Historical plan. Linux host packages and their updater paths are discontinued;
> current releases contain only the macOS host app. Linux guest computers on
> Mac hosts remain supported.

Approved scope: GitHub-hosted versioned releases, ad-hoc macOS signing, signed
Tauri updates, Apple Silicon DMG, Linux ARM64/x86-64 AppImage and Debian packages.
No Apple membership, update server, forced installation, or public publication
before the owner's review of the completed release.

## Implementation and ownership

- Native updater: trusted configuration, download/signature verification,
  operation exclusion, confirmed computer shutdown, durable exact-ID restart recovery.
- Application UI: compact General Updates card, default-on automatic checks,
  explicit download/install, real progress, actionable errors and confirmation.
- Packaging: complete versioned draft, matching checksums/signatures/feed,
  protected signing and publication, previous releases retained.
- Integration: real two-version upgrades and independent review of safety gates,
  clean installation, resource layout, and desktop behavior.

## Required evidence

| Boundary | Required observation |
| --- | --- |
| Authenticity | Correct signature installs; wrong key or modified bytes refuse installation. |
| Version selection | Current/older versions do not install; missing platform and malformed feeds report failure. |
| Downloads | Offline/timeout/truncated response is recoverable; current installation stays usable. |
| Installation | Read-only destination and insufficient space fail before computer shutdown or replacement. |
| Runtime safety | Active operations block installation; new operations cannot race confirmed shutdown. |
| Relaunch | Previously running exact computer IDs resume; stopped computers remain stopped; interrupted shutdown/install is recoverable. |
| Data | Settings, accounts, secret references, root/workspace data and backups survive the upgrade. |
| macOS | Downloaded ad-hoc DMG first launch and next-version upgrade exercised, including Keychain/login prompts and bundled helper execution. |
| AppImage | Both architectures boot the installed app, resolve packaged tools, and update the actual AppImage path. |
| Debian | Clean installation and package upgrade work; UI opens the correct package download rather than self-replacing managed files. |
| Publication | Incomplete matrix/signatures never publish; version assets are not overwritten; latest advances only after verification. |

Tests use isolated keys, feeds, builds and app-data directories. No fixture IPC,
debug routes or runtime endpoint override belongs in the production UI. Report
untested boundaries explicitly; compiling a package is not installation proof.

## Release credentials

On 10 September 2026, the owner authorized setup of the release automation.
`release-signing` and `release-publish` GitHub environments require owner review
and permit only `v*` version tags. The private updater key is an encrypted secret
in `release-signing`; the app contains only its public verification key. A local
owner-only, Git-ignored copy exists for backup. Never commit or print private
keys or upload native build directories containing account configuration.

## Primary references

Signing follow-up: MicroSandbox's checked-in `msb-entitlements.plist` already
disables library validation; this establishes its configuration, not the authors'
reason or that alternatives are impossible. Apple's macOS 14 library constraints
offer a narrower OS-enforced loading policy. The isolated ad-hoc tests and real
computer proof now pass on macOS 26.5; see [the evidence and limits](SiloUI-LIBRARY-CONSTRAINTS.md).
Minimum-OS testing and production packaging remain outstanding. This establishes
a library-load boundary, not Developer ID-equivalent publisher authentication.
Do not claim a checksum before launch is equivalent to OS enforcement at load time.

- [MicroSandbox entitlements](https://github.com/superradcompany/microsandbox/blob/main/msb-entitlements.plist)
- [Apple library constraints](https://developer.apple.com/documentation/security/defining-launch-environment-and-library-constraints)

- [Tauri updater](https://v2.tauri.app/plugin/updater/): established signed update
  transport and platform artifacts; static GitHub feed needs no hosted service.
- [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/): ad-hoc signing
  supports Apple Silicon but does not confer Gatekeeper trust or notarization.
- [Tauri AppImage packaging](https://v2.tauri.app/distribute/appimage/): package
  format and Linux build compatibility constraints.

## Verification record, 10 September 2026

The local macOS two-version test used a separate app copy, disposable signing
key and loopback feed compiled into test builds. None of those settings belongs
in the shipping configuration.

- A bad update signature produced a recoverable error and left the executable
  unchanged. A correctly signed retry reached the explicit install confirmation.
- Cancel kept the running `dev` computer and downloaded update available.
- A malformed archive failed installation without replacing the app; the stopped
  computer resumed and its recovery journal cleared.
- The corrected archive upgraded the app from 0.1.0 to 0.1.1. The running `dev`
  computer restarted, its guest boot identifier changed, and its workspace test
  file survived. The completed recovery journal was removed.
- The first archive exposed BSD tar's AppleDouble metadata as a second top-level
  entry. Release archives must contain only the `Silo.app` root.

These were debug-compiled native bundles with hardened runtime and ad-hoc
signatures, not proof of a downloaded production DMG or Gatekeeper first launch.
The temporary `msb` helper required the library-validation exception to load
libkrun firmware without an Apple Team ID. Applying that exception to release
packaging remains pending explicit owner authorization; the outer app and Git
helpers do not need it.

Native atomic-replacement tests also terminated child installers immediately
before and after replacement and verified a complete installed app remained.
This is process-interruption evidence, not a claim about every power-loss case.

Final local checks:

- Frontend: 633 tests across 69 files; TypeScript and lint passed.
- Native app/build: 300 + 5 tests passed, 10 opt-in tests skipped.
- Updater transport: three real loopback HTTP tests passed, covering version
  comparison, malformed/offline feeds and signature refusal.
- Release helpers: 24 tests passed, including full publication refusal and
  Debian tool relocation. GitHub and package-tool boundaries use test doubles.
- Both Linux architectures passed real AppImage download-failure/retry,
  signature-refusal, installation-byte comparison, 0.1.1 relaunch and settings
  preservation checks. Installed Debian desktop checks passed, followed by real
  package-manager upgrades from 0.1.0 to 0.1.1. System Git's hash stayed unchanged;
  private MicroSandbox, Git and LFS executables worked.
- Linux updater transport/atomic tests: seven passed, one child-only test ignored.
  Local evidence is under the ignored `app/SiloUI/test-results/distribution/`.
  These Linux update UI runs used empty VM sets in OrbStack without KVM. Running
  VM shutdown/resume during an actual app upgrade was verified on macOS, not Linux.
  Linux fixture packages were debug builds of production code with isolated
  signing keys, feeds and synthetic OAuth configuration. Optimized Linux release
  builds remain part of the pending GitHub CI run.
- Optimized macOS app compiled; deep/strict code signature verification,
  production updater signature verification, and version/architecture inspection
  passed. The signing readiness gate correctly rejected that release bundle
  pending the helper entitlement and accepted the authorized temporary test app.
- The normal development app was rebuilt with the production endpoint/key and
  reopened on Settings → General. It reports version 0.1.0 and `dev` is running.
  No test feed override or temporary app copy is left running.

Low/unknown disk-space tests inject the capacity measurement; an unwritable
parent is tested against the filesystem. We did not fill the host disk.
GitHub CI execution is pending authorization to push the source to the explicit
verification branch. Nothing was published. Downloaded macOS DMG/Gatekeeper and
post-update Keychain behavior remain release acceptance work after the helper
signing decision; the successful temporary upgrade alone does not close those
checks.

## Linux installation UX recommendation, 13 September 2026

Implementation follow-up: [Linux system updates](SiloUI-LINUX-UPDATES.md) describes
the new installer and publishing workflow. The assessment below records the
previous released behavior that motivated it.

The current Debian package installs the application normally but has no Silo
APT update source. Its in-app update action opens a package download. This is
usable for testing, but repeated manual downloads do not meet the desired
install-once experience.

For the supported Ubuntu-compatible audience, the recommended next distribution
step is to retain the Debian installer and offer a signed APT source during
installation. Updates can then arrive through the normal system updater.
This is a proposal, not implemented behavior. VS Code uses this installation
pattern: its Debian package offers the repository and signing key to enable
updates through the package manager. See its [Linux installation guide](https://code.visualstudio.com/docs/setup/linux)
and Ubuntu's [third-party repository guidance](https://ubuntu.com/server/docs/explanation/software/third-party-repository-usage/).
Before shipping, verify package upgrades while Silo or its VMs are running;
system package updates do not invoke Silo's in-app update shutdown coordination.

The existing AppImage supports Silo's in-app updater, but users still need to
make it executable and arrange a stable location/menu entry. Switching the
primary download to AppImage alone does not finish installation UX. See the
[AppImage user guide](https://docs.appimage.org/user-guide/),
[desktop integration reference](https://docs.appimage.org/reference/desktop-integration.html),
and [Tauri updater documentation](https://v2.tauri.app/plugin/updater/).
An identical in-app update flow on Linux and macOS would instead require a
polished AppImage installation/integration flow. Keep package-format choices
out of the primary download journey once a default is selected.
