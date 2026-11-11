# Building and releasing Silo

Every native Silo build requires the GitHub App configuration. The build reads
`app/SiloUI/github-build.local.json` automatically; no terminal exports are needed.
Explicit environment variables take precedence, which is how GitHub Actions
supplies the same configuration. Missing, empty, malformed, or multiline values
stop the native build instead of producing an app with broken GitHub access.
Silo's supported host is Apple Silicon macOS 15+. Linux host apps and packages
are discontinued; Linux computers remain supported as guests on a Mac.

## Release a new version

Silo uses **Changesets** for version decisions and changelogs. Contributors add
short release notes with their changes. Preparing a release combines those notes;
pushing its version tag builds a draft. Publication is a separate explicit step.
Normal branch pushes do not release the app.

Run these commands from `app/SiloUI`. Install dependencies with `npm ci` first.
Use Node.js 24, Python 3.11 or newer, Go 1.25 or newer for cold native runtime
preparation, Zig 0.15.2 (the C compiler for the Linux `silo` Silo installs in Linux
computers; the release workflows install it pinned by checksum, see
[runtime packaging](SiloUI-RUNTIME-PACKAGING.md#the-silo-program-for-linux-computers)),
Git, and GitHub CLI (`gh auth login` for
publication). Your Git remote `origin` must point to the Silo repository, and
your account needs push and Actions permissions. CI holds the signing keys;
local release preparation needs no signing credentials or VM runtime.

### While making changes

```sh
npm run changeset
```

Choose `silo-ui`, the bump type, and write a user-facing summary. Use **patch**
for fixes and **minor** for features and incompatible changes. Silo stays below
1.0.0 until the owner explicitly decides on a stable release, so never use
**major**: `sync-release.mjs` and `release.mjs` refuse 1.0.0 or later unless
`--allow-stable` is passed (for example
`npm run release:version -- --allow-stable`). Include any migration steps. Commit the generated `.changeset/*.md`
file alongside the change. Edit the Markdown freely before release. Internal
refactors, tests, and documentation do not require a note unless users are affected.

Agents can create these files directly; release notes do not depend on commit
message conventions. Each note should explain the resulting behavior, not list
implementation files. Never put credentials or private user information in notes.

### Prepare and review

```sh
npm run release:status
npm run release:version
```

`release:version` first asks Changesets for the planned version and checks it:
below 1.0.0, no existing `docs/releases/VERSION.md`, and version metadata our
adapter can update. A failed check changes nothing. Changesets then updates
`package.json` and `CHANGELOG.md` and consumes the pending notes. Our adapter updates `package-lock.json`,
`src-tauri/Cargo.toml`, and the workspace `Cargo.lock` to the same version without
changing dependencies. It exports the new changelog entry to
`docs/releases/VERSION.md`, which becomes the app update notes. The GitHub
release page shows download links first, then those notes with the full change
lists collapsed. Tauri already reads its version from `package.json`.

Review all generated changes, including the removed changeset files. Several
pending notes produce one release using the largest requested bump.

Commit the generated changes and push your branch. Use your normal review process;
merge the release preparation into `main` before releasing from its clean checkout.
Do not edit version files by hand. To edit wording after preparation, keep the
new changelog entry and `docs/releases/VERSION.md` consistent.

### Build the draft

```sh
npm run release:draft
```

This requires a clean working tree, synchronized versions, release notes, no
pending changesets, a commit already on `origin/main`, and no release tag on
`origin` for a newer version or for this version at another commit. These checks
run locally, before any tag is pushed. Changesets creates the `vVERSION` tag; the command pushes
only that tag to `origin`. The tag push automatically runs **Build Silo release**.
Approve `release-signing` in GitHub Actions if requested. The macOS build and
shared validation jobs must pass before the draft appears under GitHub Releases. Nothing is
published to npm, and no public app update is announced yet.

Test the draft installers on clean supported systems and upgrade an earlier real
installation. Review the notes and the acceptance evidence below.

### Publish the tested draft

From the same release commit:

```sh
npm run release:publish
```

This dispatches **Publish verified Silo draft** against the exact version tag.
Approve `release-publish` if requested. The workflow verifies the stored packages,
signatures, checksums, version metadata and update feed, then publishes and marks
the release latest. A successful command means the workflow was requested;
publication is complete only when that workflow succeeds.

The final publication gate also refuses 1.0.0 or later by default. Only after the
owner approves a stable release, pass `npm run release:publish -- --allow-stable`
or explicitly select `allow_stable` in the publication workflow. An ordinary
publication retry leaves that option off.
For an approved stable draft, manually dispatch **Build Silo release** on its
version tag with both `draft` and `allow_stable` enabled. Tag-triggered builds
have no opt-in and reject 1.0.0 or later before platform builds.

### Preview, retries, and recovery

- `npm run release:status` is read-only. No pending changes is not a new release;
  `release:version` fails without changing the version when there are no notes.
- If versioning succeeds but synchronization fails, fix the reported input and
  run `npm run release:sync`. It can be retried without another version bump and
  refuses to overwrite different existing release notes. Review the working diff
  before committing; failed preparation never pushes or publishes anything.
- For CI verification before tagging, manually run **Build Silo release** on
  your branch with `draft` unchecked. Those packages use isolated test signing
  keys and are not distributable updates.
- A failed tag push leaves a local tag; retry `release:draft`. The command never
  force-moves tags. If a tag identifies another commit, check out that release or
  prepare a newer version.
- Pushing an existing remote tag again does not retrigger CI. Retry **Build Silo
  release** manually on that tag with `draft` checked. An existing incomplete
  draft must be reviewed and explicitly removed before rebuilding; publication
  refuses incomplete drafts. Never replace a published version.
- For a later publication retry, check out the release tag and run
  `release:publish`, or select that tag in **Publish verified Silo draft** and
  enter its version without the `v` prefix.

The installed Changesets CLI is pinned in `package.json` and the lockfile.
Configuration keeps Silo private to npm while enabling versioning and Git tags.
Changesets 3 uses `git-tag`; the wrapper uses the installed command. See the
[Changesets source and documentation](https://github.com/changesets/changesets)
for its note format and release model. Run `npm run test:release` to exercise
actual Changesets versioning in disposable repositories and the desktop adapter.

## Local setup

Native development requires a GitHub App client ID, slug, and client secret.
Contributors can follow the [source-build guide](SiloUI-BUILD-FROM-SOURCE.md)
to create their own App and install all build prerequisites, including Go for
the bundled Git LFS server. Installed-app users do not need this configuration.

Maintainers using the existing Silo GitHub App can configure it as follows.
The local configuration file is ignored by Git. From the repository root:

```sh
cp app/SiloUI/github-build.example.json app/SiloUI/github-build.local.json
chmod 600 app/SiloUI/github-build.local.json
```

Fill in `SILO_GITHUB_CLIENT_SECRET` using the existing GitHub App's client secret.
You need access to that credential only when building with Silo's App; for your
own App, replace all three values. The example contains Silo's two public identifiers:

| Key | Value / source |
| --- | --- |
| `SILO_GITHUB_APP_SLUG` | `silo-amont-labs` |
| `SILO_GITHUB_CLIENT_ID` | `Iv23liEjp3VnGe0sw2LU` |
| `SILO_GITHUB_CLIENT_SECRET` | Client secret from the GitHub App settings; never commit the value |

These identify Silo's GitHub App, not a user's password or personal access token.
They do not alter saved accounts, repository selections, or computer Git identity.
The client secret is embedded in the desktop executable and is extractable; it
is not a confidential boundary in a distributed desktop app. Keep the source
file and verbose Cargo build output private. Cargo's ignored build outputs also
contain the compiled configuration. Do not upload the entire Cargo target tree
as an Actions artifact or cache.

Release review must explicitly acknowledge that publishing packages distributes
this GitHub public-client secret. It is not an App private key or a user's access
token, and it must never authenticate a Silo installation to a backend service.
The [0.1.1 credential-distribution audit](SiloUI-OAUTH-RELEASE-AUDIT.md) records
package inspection, live App settings, PKCE enforcement checks, and their limits.

Use the existing commands:

```sh
npm --prefix app/SiloUI ci
npm --prefix app/SiloUI run desktop
npm --prefix app/SiloUI run desktop:build:debug
npm --prefix app/SiloUI run desktop:build
```

Only run the command needed: `desktop` starts development mode;
`desktop:build:debug` builds the Silo Dev app with ad-hoc signing (Dev channel);
`desktop:build` produces a verified optimized local macOS app. The macOS DMG
and updater artifacts are produced
by the release workflow, after VM signature finalization. Platform resource
preparation runs before native compilation and needs network access on a cold
cache. Install Rust 1.94.0 (`rustup toolchain install 1.94.0`) for the pinned
MicroSandbox source build, plus the device's Tauri prerequisites.

The Rust `build.rs` loads the file relative to the crate, so it also covers
direct `cargo build`, `cargo test`, and direct Tauri CLI invocations from other
working directories. Cargo tracks changes to the file and all three environment
variables and recompiles when they change. An explicitly empty environment
variable fails even when the local file has a value; unset a stale override to
use the file again. An invalid local JSON file must be repaired or removed.

Native tests also require configuration. The isolated CI native-test jobs use
explicit synthetic configuration; package jobs use configured Actions secrets.
Contributors running offline unit tests can explicitly supply synthetic values
for all three variables; such test executables cannot authenticate to GitHub and
must not be distributed. Frontend tests need no GitHub credentials.

### Local macOS bundles

From the repository root, build a debug app:

```sh
npm --prefix app/SiloUI run desktop:build:debug
```

Output: `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Dev.app`. Debug
builds and `npm run desktop` use the separate Silo Dev channel
(`org.silo.dev`); see [build channels](SiloUI-BUILD-CHANNELS.md). The debug
app embeds a debug `silod` (`Silo Engine Dev`) with copies of the app's tools,
keeps Tauri's development signatures on them, and is re-sealed afterwards.
For an optimized local app without installer or updater artifacts:

```sh
npm --prefix app/SiloUI run desktop:build
```

Output: `app/SiloUI/src-tauri/target/release/bundle/macos/Silo.app`.
These commands do not install or publish the app. On macOS, `desktop:build`
wraps Tauri's app bundling with `sign_helpers`, `embed_engine`, `seal_app`,
`verify_bundle` and `verify_engine` from `scripts/macos_release_signing.py` and
`scripts/embed_engine.py`. It retains hardened runtime and applies the existing
exact-engine constraint to the VM helper. The helpers are signed before the
engine is embedded, because `silod` runs computers from its own bundle: the
engine carries copies of `msb`, `libkrunfw`, the Git tools, the network gateway
and bootstrap, and the runtime and Git resources, and `verify_engine` checks
each signed copy has the app's code hash. It forces app-only
output and disables updater artifact creation, so no distribution certificate
or updater signing key is needed. A signing or policy-verification failure
fails the build. Plain `npx tauri build` bypasses this finalization and is not
the supported local macOS app build command.

The previous explicit `--bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`
arguments remain supported.
`--target aarch64-apple-darwin` and `CARGO_TARGET_DIR` are resolved through
Cargo's output directory. Set `CARGO_TARGET_DIR` to an absolute separate path
when the usual output bundle is running. `--no-bundle` builds and non-macOS
targets retain their Tauri behavior. DMG/all bundle requests use the
release workflow instead; generating an archive before helper finalization
would package the wrong signature.

The September 22 startup investigation matched a successful 11:07 local VM
start to the debug bundle and the 11:16 failure to an optimized local bundle.
The latter had ordinary app entitlements on `msb`, so hardened runtime rejected
its ad-hoc engine for lack of a matching Team ID. The local build command had
omitted the finalization already used by release packaging. See
[the library-loading proof](SiloUI-LIBRARY-CONSTRAINTS.md) for the policy and
its primary sources. `scripts/test_macos_release.py` now reproduces that
loader failure with real disposable signatures and verifies local build
finalization; `scripts/test_desktop_release.py` covers output selection,
build/signature failures, and pass-through behavior. Both are included in the
existing release workflow's Python test discovery.

Verification on September 22 used the unchanged packaged helper and engine from
`src-tauri/target/local-signing/release/bundle/macos/Silo.app`, built with an
absolute `CARGO_TARGET_DIR` to leave the running release app untouched. Its
signature-policy gate passed. With a fresh temporary `MSB_HOME`, the helper
imported the packaged guest image, created and started a one-CPU, 512 MB VM
with networking disabled, executed `LOCAL_BUILD_VM_OK`, and stopped it. The
temporary home was removed after successful stop. Results and the disposable
probe are under the ignored `src-tauri/target/verification/local-signing/`.
This validates packaged VM startup and execution; the native UI was not
relaunched, and no installed app or existing VM was modified.

### Verify a change

Run the checks relevant to the change from the repository root:

```sh
npm --prefix app/SiloUI run typecheck
npm --prefix app/SiloUI run lint
npm --prefix app/SiloUI test
cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check
cargo test --manifest-path app/SiloUI/Cargo.toml --workspace --locked
cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked -p tauri-plugin-updater --lib
npm --prefix app/SiloUI run test:release
python3 -m unittest discover -s app/SiloUI/scripts -p 'test_*.py'
```

`test:release` uses [Node's quoted recursive test glob](https://nodejs.org/docs/latest-v24.x/api/test.html#running-tests-from-the-command-line)
`"scripts/**/*.test.mjs"`, so new script suites run without updating a filename
list. `scripts/test_ci_coverage.py` verifies discovery and failure propagation
with disposable new root and nested suites.

Continuous integration runs the same checks. `.github/workflows/ci.yml` runs on
every push to `main` and every pull request: frontend, script, website and demo
checks; a blocking [Rust formatting check](https://github.com/rust-lang/rustfmt#verifying-code-is-formatted)
using the pinned toolchain; the Rust
suites on macOS with synthetic GitHub configuration and the patched updater library tests;
[Cargo's default package selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#package-selection)
uses the workspace's configured default members (`silo-ui` and `silo-engine`); the patched
updater still requires an explicit `-p` command. A relative-link check of the Markdown
documentation with [lychee](https://github.com/lycheeverse/lychee)
in offline mode. The macOS job also runs `test_macos_release.py` against
ad hoc signed disposable binaries. CI does not run the ignored live computer tests.
Pull requests that change only `website/`, `demo/`, `docs/`, `artifacts/`,
`assets/`, top-level Markdown or frontend sources under `app/SiloUI/src/`
(except the contract fixtures and reserved secret names that Rust tests read)
skip the macOS Rust and Linux engine jobs. `scripts/ci-native-changes.py`
decides, `scripts/test_ci_native_changes.py` checks that no Rust source,
manifest or Tauri config names a skipped path, and any other path, a failed
check or a push to `main` runs them.
The link check includes first-party source documentation, guest notices and
changesets. A coverage regression compares its globs with tracked Markdown,
excluding documentation in the partial upstream vendor tree.
To run the link check locally, install lychee and run from the repository root:

```sh
lychee --offline --no-progress README.md AGENTS.md 'docs/**/*.md' 'app/SiloUI/*.md' \
  'app/SiloUI/tests/**/*.md' 'app/SiloUI/src/**/*.md' 'app/SiloUI/.changeset/*.md' \
  'app/SiloUI/src-tauri/guest/**/*.md' 'app/SiloUI/docs/**/*.html' \
  'artifacts/**/*.md' 'website/*.md' 'demo/*.md'
```

Linux host build, package-installation and AppImage verification workflows have
been removed with Linux host support. Linux guest behavior remains part of the
Mac-hosted engine and runtime tests.

Every workflow pins third-party actions to a full commit SHA with the release
version as a comment (`scripts/test_workflow_pins.py` enforces it).
`.github/dependabot.yml` proposes updated SHAs in one weekly pull request. npm
(app, website, demo) and Cargo version updates arrive in two weekly grouped pull
requests per ecosystem, one for minor and patch updates and one for majors.
Security fixes arrive as advisories appear, grouped per ecosystem. Tauri minor
upgrades, noVNC, the vendored updater plugin and the microsandbox crates stay
manual (see the comments in that file).

Native tests require the configuration described above. Frontend fixtures and
unit tests do not prove installed-app behavior, live computer health, or two-device
operation. Keep opt-in live tests separate from ordinary tests.

### Clippy baseline (K-19)

Clippy remains advisory and is not a blocking CI check. On 2026-09-30, Rust
1.94.0 on `aarch64-apple-darwin`, after the formatting sweep, produced the
following baseline from `app/SiloUI/src-tauri` with synthetic GitHub values:

```sh
SILO_GITHUB_APP_SLUG=silo-test SILO_GITHUB_CLIENT_ID=test-client \
  SILO_GITHUB_CLIENT_SECRET=test-secret cargo clippy --locked --all-targets --message-format=json
```

The command exited 101: one `clippy::unused_io_amount` error at
`src/github_http.rs:462` ignores a test server's read length. This lint is
[denied by default](https://rust-lang.github.io/rust-clippy/rust-1.94.0/index.html#unused_io_amount).
The 106 emitted warnings below include repeats across binary and test targets;
deduplicating by lint, message and source spans gives 69 distinct warnings.
No lint fixes or suppressions were applied. Linux-only code needs a separate
baseline. See [Clippy usage](https://doc.rust-lang.org/clippy/usage.html) for
target selection.

| Lint | Emitted warnings |
| --- | ---: |
| `clippy::blocks_in_conditions` | 2 |
| `clippy::bool_assert_comparison` | 1 |
| `clippy::cloned_ref_to_slice_refs` | 2 |
| `clippy::field_reassign_with_default` | 23 |
| `clippy::items_after_test_module` | 4 |
| `clippy::let_and_return` | 8 |
| `clippy::manual_contains` | 2 |
| `clippy::manual_is_multiple_of` | 2 |
| `clippy::manual_range_contains` | 2 |
| `clippy::manual_try_fold` | 2 |
| `clippy::needless_borrow` | 12 |
| `clippy::needless_return` | 4 |
| `clippy::nonminimal_bool` | 6 |
| `clippy::redundant_closure` | 2 |
| `clippy::too_many_arguments` | 6 |
| `clippy::type_complexity` | 10 |
| `clippy::unnecessary_cast` | 6 |
| `clippy::unnecessary_map_or` | 2 |
| `clippy::unnecessary_to_owned` | 2 |
| `clippy::wrong_self_convention` | 2 |
| `dead_code` (rustc) | 6 |
| **Total** | **106** |

## Versioned distribution and updates

Releases are deliberate. Development pushes do not publish downloads. The source
version in `app/SiloUI/package.json`, package lock, Cargo manifest and Cargo lock
must agree. Stable versions use `MAJOR.MINOR.PATCH`; `0.0.0` and prereleases cannot
be published through the stable pipeline.

Supported packages:

| Platform | Installer | In-app updates |
| --- | --- | --- |
| Apple Silicon macOS | DMG | Signed Tauri app archive |

Linux host apps and packages are discontinued. Linux VMs continue to run on a
Mac host. Intel macOS and Windows are unsupported.
The native runtime, host Git/LFS tools and notices are packaged with
the application. The guest VM image is not: Silo downloads it once from the pinned
guest-image release on first use ([guest images](SiloUI-GUEST-IMAGES.md#download-on-first-use)),
which keeps installers and updates near 35 MiB. Existing computer disks are not release assets.
A change to `guest-image/image-lock.json` is embedded in the next app build; publish the guest
release assets before releasing an app that pins them, because the app downloads them directly.

### Signing setup

`tauri.conf.json` contains the permanent public updater key. This is safe to
commit. The private key is stored in protected GitHub environment
`release-signing` as `TAURI_SIGNING_PRIVATE_KEY`; its optional password is
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep an independent secure backup. Losing
the private key prevents updates to already installed applications. Never upload
private keys, complete build directories, or local GitHub configuration artifacts.

The macOS packager passes the signing password through Tauri's
[password environment input](https://v2.tauri.app/reference/cli/#signer-sign),
which the installed CLI also lists in `tauri signer sign --help`. Keep passwords
out of command arguments: a failed subprocess reports its arguments in Python's
exception text. Signing failures report only the exit code.

The `release-signing` and `release-publish` environments require maintainer review
and restrict execution to version tags. Artifact-only verification uses a fresh
ephemeral signing key in `release-verification`; these packages are for tests and
cannot update production installations. The public key override only occurs in
that isolated workflow checkout. These are not public releases.

Final publication uses one shared concurrency group with `queue: max`, preserving
up to 100 pending requests instead of replacing the pending request when a third
arrives. Publications remain serialized, and their version checks still reject
an obsolete or already published version. GitHub [released the larger queue on
May 7, 2026](https://github.blog/changelog/2026-05-07-github-actions-concurrency-groups-now-allow-larger-queues/).
Actionlint 1.7.12's unsupported-key diagnostic is a known
[upstream validation gap](https://github.com/rhysd/actionlint/issues/680), as
described in the [workflow fix audit](research/micro-reviews/workflows-fixes.md).

Drafts are Developer ID signed and notarized once the owner steps in
[Apple signing and notarization](#apple-signing-and-notarization) are done;
until then they fail, or are signed ad hoc when the owner opts in
([Releases without Developer ID signing](#releases-without-developer-id-signing)).
Artifact-only builds, local builds and a dry run stay ad-hoc signed and are not
notarized; such an installation can require System Settings → Privacy & Security
→ Open Anyway. Do not instruct users to disable Gatekeeper globally. Update
signatures are separate and always checked.

### Apple signing and notarization

`SILO_RELEASE_SIGNING` selects the mode of `scripts/package-macos-release.py`
(`scripts/macos_notarization.py` holds the steps):

| Mode | Used by | Signing | Notarization |
| --- | --- | --- | --- |
| `adhoc` | local builds, artifact-only runs | ad hoc | none |
| `dry-run` | release workflow dispatched with `dry_run` | ad hoc | every notarization, stapling and Gatekeeper command is printed with a `[dry-run]` prefix, not run |
| `developer-id` | every draft (version tag push or `draft` dispatch) with the Apple credentials configured | Developer ID Application, hardened runtime, timestamp | `notarytool submit --wait`, `stapler staple`, `spctl --assess` |

The `validate` job derives the requested mode: a draft requests `developer-id`,
and `dry_run` together with `draft` is refused. The macOS build job then runs
`macos_notarization.py resolve-mode`: with all seven Apple secrets present the
draft stays `developer-id`; with none of them and the repository variable
`SILO_RELEASE_ALLOW_UNNOTARIZED` set to `true` it becomes `adhoc` and the log
shows a warning (see
[Releases without Developer ID signing](#releases-without-developer-id-signing));
otherwise the run fails and names the missing ones. Credentials are
read only from the environment. A `developer-id` run stops before signing if the
identity is not a `Developer ID Application:` certificate.

#### Releases without Developer ID signing

Without an Apple Developer ID, leave the seven Apple secrets unset in
`release-signing` and opt in with the repository variable
`SILO_RELEASE_ALLOW_UNNOTARIZED`:

```sh
gh variable set SILO_RELEASE_ALLOW_UNNOTARIZED --body true
```

Tag pushes and `draft` dispatches then build the same assets signed ad hoc and
not notarized, with a warning in the log. Without the variable, missing Apple
secrets fail the run as before; a partial set always fails. The updater key
(`TAURI_SIGNING_PRIVATE_KEY`) is still required, and update signatures are still
checked. The macOS job uploads the resolved mode as the `signing-mode-<asset>`
artifact; `publish-release.py` prints it, states it on the draft page (with a
`<!-- silo-signing: <mode> -->` marker) and, on publication, refuses an ad hoc
draft when the previous published release was notarized unless **Publish
verified Silo draft** is dispatched with `allow_unnotarized`. Adding all seven
secrets later switches drafts back to Developer ID signing with no workflow
change; delete the variable then (`gh variable delete SILO_RELEASE_ALLOW_UNNOTARIZED`). Locally, `npm run desktop:build` produces the same kind of app and
`SILO_RELEASE_SIGNING=adhoc python3 scripts/package-macos-release.py <Silo.app> <output> --engine <silod> --cli <silo>`
the DMG and update archive.

What users see: macOS quarantines the downloaded DMG, and the first open of
`Silo.app` shows a dialog saying Apple could not verify it. On macOS 15, after
that first attempt they open System Settings → Privacy & Security and click
**Open Anyway** next to the message about Silo. Alternatively, after copying
Silo to Applications, they run
`xattr -dr com.apple.quarantine /Applications/Silo.app`.

Do not tell users to disable Gatekeeper globally. The Tauri updater does not
set the quarantine attribute, so in-app updates are expected to open without
the dialog; an ad-hoc signature changes with every build, so macOS can ask
again for Keychain access after an update. Both belong to the acceptance
evidence below and were not checked on a real update. State the first-open
steps in the release notes of every ad-hoc release.

Order of one `developer-id` run:

1. Every other Mach-O file in the app (the Git tools and the VM library) is
   re-signed with the identity, hardened runtime and a timestamp, because the
   Tauri bundle step signs them ad hoc. Then the tools `msb`,
   `silo-macos-gateway` and `silo-network-bootstrap` are signed with their
   entitlements and the library constraint, which names the library's final
   code hash.
2. `Silo Engine.app` is assembled from the signed tools and signed with the
   identity and the engine entitlements. Its `SiloEngineCodeRequirement` names
   the Developer ID team.
3. The app is sealed with the identity and `src-tauri/Entitlements.plist`
   (nested code first, outer bundle last, never `--deep`), then verified.
4. The app is zipped and submitted with `xcrun notarytool submit --wait`; the run
   fails unless the status is `Accepted`. The engine is nested in the app and is
   covered by that submission and ticket. It is not stapled on its own, because
   adding a file inside the sealed app would break its signature.
5. `xcrun stapler staple` and `validate` on the app, then
   `codesign --verify --deep --strict` and `spctl --assess --type execute`.
6. Only now are the updater archive (`.app.tar.gz`) and its Tauri signature
   created, so the archive carries the stapled app.
7. The DMG is built from the stapled app, signed, submitted, stapled and
   assessed with `spctl --assess --type open --context context:primary-signature`.

The standalone engine release tarball (`silo-engine-<version>-macos-arm64.tar.gz`)
holds an unsigned `silod` binary and the files an app needs to assemble its own
engine bundle, and that app signs it. Notarytool does not take a tarball and a
tarball cannot be stapled, so the tarball is not notarized; the engine that
ships is the one inside the notarized app.

#### Dry run

Dispatch `Build Silo release` with `dry_run` checked (no `draft`). The macOS job
signs ad hoc, and the log shows each command the real run would execute, with
credential values replaced by placeholders such as `<APPLE_API_KEY_ID>`. No
Apple credentials are needed. Locally:

```sh
SILO_RELEASE_SIGNING=dry-run TAURI_SIGNING_PRIVATE_KEY=<test key> \
  python3 scripts/package-macos-release.py <Silo.app> <output> --engine <silod>
```

`python3 -m unittest discover -s scripts -p 'test_release_notarization.py'`
checks the order of the steps, the dry-run stubs, that nothing runs in a dry run
and that no credential value is printed.

#### Repository secrets

All names are Actions secrets. The workflow passes them only to the steps that
need them, and only for `developer-id` runs. Add them to the `release-signing`
environment, which already requires maintainer review:

| Secret | Content |
| --- | --- |
| `APPLE_CERTIFICATE_P12_BASE64` | the Developer ID Application certificate with its private key, exported as `.p12` and base64 encoded |
| `APPLE_CERTIFICATE_PASSWORD` | the password chosen when exporting the `.p12` |
| `APPLE_DEVELOPER_ID_IDENTITY` | the exact certificate name, `Developer ID Application: Name (TEAMID)` |
| `APPLE_TEAM_ID` | the ten-character Apple team ID |
| `APPLE_API_KEY_P8` | contents of the App Store Connect API key `.p8` file |
| `APPLE_API_KEY_ID` | that key's ID |
| `APPLE_API_ISSUER_ID` | the issuer ID shown on the App Store Connect Keys page |

The certificate is imported into a temporary keychain for the packaging step and
deleted afterwards; the `.p8` is written to a temporary file with mode 0600 and
removed after notarization.

#### Owner checklist

- [ ] Enrol the team in the Apple Developer Program, if it is not already.
- [ ] In the Apple Developer account, create a **Developer ID Application**
  certificate (Certificates, Identifiers & Profiles) from a certificate signing
  request made in Keychain Access, and download it. Only the Account Holder can
  create Developer ID certificates.
- [ ] In Keychain Access, export the certificate with its private key as a
  `.p12` with a password; base64 encode it (`base64 -i file.p12`).
- [ ] Read the exact identity name with
  `security find-identity -v -p codesigning` and the team ID from the Apple
  Developer account.
- [ ] In App Store Connect, Users and Access, Integrations, create an API key
  with access that allows notarization, download the `.p8` once, and note the key
  ID and issuer ID.
- [ ] Add the seven secrets above to the `release-signing` environment.
- [ ] Run `Build Silo release` with `dry_run` and read the printed steps.
- [ ] Tag a release and check the draft: `spctl --assess --type execute -vv Silo.app`
  and `xcrun stapler validate Silo-macos-arm64.dmg` on a downloaded copy.
- [ ] Keep an independent backup of the `.p12`, its password and the `.p8`.

The macOS release packager signs the bundled VM engine first, then constrains
`msb` to that exact library's code hash. Apple system libraries remain permitted
by macOS. The helper's library-validation exception is paired with this enforced
constraint; an unconstrained helper fails release verification. The app and Git
helpers retain their ordinary library validation. This blocks engine substitution,
not malicious code already present in the approved build or replacement of the
entire ad-hoc-signed app. Each update gets a constraint for its own engine.
The packager regenerates both the DMG and signed updater archive from the same
finished app, with no AppleDouble archive entries.

The minimum macOS 15 constraint tests are required before draft creation; the
build runner also exercises the constraint tests. GitHub's macOS 14.8.9 and
15.7.9 runners were verified to have System Integrity Protection disabled on
2026-09-10. Their explicit `--constraints-only` mode checks library fingerprint
restrictions and records the two signature-enforcement controls as skipped.
A passing hosted result does not establish signature enforcement. Public release
also requires the full suite on a Mac with SIP enabled.
GitHub [retired its macOS 14 runners on November 2, 2026](https://github.com/actions/runner-images/issues/13518),
so the supported minimum is now macOS 15 and the required job runs on `macos-15`.
The bundle keeps `minimumSystemVersion` 14.0 on purpose: the update feed has
one `darwin-aarch64` package and existing clients do not check the OS version,
so raising it would update macOS 14 installations to an app that cannot open.
This CI test checks library enforcement, not nested VM execution.

### Build and publish

Follow [Release a new version](#release-a-new-version) above. Both version-tag
pushes and manual draft builds use the same signing and validation pipeline;
only the separate publication workflow can make the draft public.

The app reads
`https://github.com/amontlabs/silo/releases/latest/download/latest.json`.
That file references immutable version-specific download URLs and the macOS
updater signature. Partial build/upload failures leave the prior public release
and update feed unchanged. A partial draft must be inspected and explicitly
removed before retrying; the scripts never silently clobber it. A bad published
release is fixed with a newer version, not an automatic data downgrade.

GitHub release immutability was enabled for this repository on 2026-09-10.
The workflow also refuses existing release versions and older stable versions.
The `publish-release.py` tests cover missing/empty/unexpected macOS assets,
symlinks, invalid signature encoding, version bounds, complete checksums and
platform URLs. `verify-release-metadata.py` inspects the macOS Info.plist and
Mach-O headers without executing the app.

### Legacy Linux packages

Previously published Debian packages and AppImages remain historical downloads;
new releases contain only the macOS host app. Linux host packages receive no
engine updates and are not covered by current build or release checks. Linux
guest computers on Mac hosts remain supported.

### Required release acceptance evidence

- Clean install from the actual downloaded DMG.
- Real signed version-to-version update and app relaunch; preserved settings,
  account, secrets, computer disks and previous running state.
- Invalid signature, interrupted/offline download, low disk space, read-only
  installation directory and interrupted installation.
- macOS quarantine first launch, signature verification and Keychain behavior
  after upgrading an ad-hoc signed app.
- Built-in computer use qualified on an Apple Silicon Mac using the supported
  Mac host app and Linux guest image. Run the opt-in live tests
  `backup_controller::tests::live_built_in_computer_use_sets_up_and_survives_export_and_import`
  and `live_built_in_desktop_boots_repeatedly` with a guest image and the
  packaged `msb`; see [Rust test support](SiloUI-RUST-TEST-SUPPORT.md) for the inputs
  and `SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures`. Then confirm on the Mac
  host with throwaway `e2e-*` computers: a fresh boot, a stop and start, a
  checkpoint restore, an export and import, and Cua Driver readiness (`computerUse` is
  `ready` and `cua-driver doctor` passes) after each. Record the device, driver version,
  commands and results with the release evidence.
- No public-release claim until these checks have real evidence. Unit/build
  success does not substitute for clean installation or computer execution.

## Engine releases

The engine (`silod`) has its own releases, independent of the Silo app's `v*`
tags. An app that embeds the engine, such as Fjord, downloads a pinned release
instead of compiling Silo. The same release carries the Linux host engines,
which run Linux computers on a Linux machine
([`silod` on a Linux host](SiloUI-ENGINE.md#silod-on-a-linux-host)).

### Cut an engine release

The macOS engine is Apple Silicon only, like every Silo Mac host: its macOS VM
code does not build for Intel, so there is no `x86_64` or universal macOS
archive. The Linux engines are built for x86-64 and arm64.

1. Raise `version` in `app/SiloUI/src-tauri/silod/Cargo.toml` and
   `engineVersion` in
   `app/SiloUI/src-tauri/silod/macos/resources/engine-manifest.production.json`
   (and the development manifest) to the same value, and update `Cargo.lock`.
   Merge it.
2. Run the "Publish Silo engine" workflow from `main` (Actions, Run workflow).
   It takes the version from Cargo.toml, fails unless it runs on `main`, fails
   if the release or tag `engine-v<version>` already exists, and creates the tag
   and release at the commit it built. Alternatively, tag the merged commit
   `engine-v<version>` and push the tag; the workflow then fails unless the tag
   equals `engine-v<version>` of the Cargo version.
3. `.github/workflows/engine-release.yml` builds `silod` with Rust 1.94.0 for
   `aarch64-apple-darwin` on `macos-15`, packages it with
   `app/SiloUI/scripts/package-engine.py`, and checks that the binary is `arm64`
   and the checksum. A `tag` job checks the tag before either build starts. In
   parallel with the macOS build, its `build-linux` job builds on `ubuntu-24.04`
   (`linux-x64`) and `ubuntu-24.04-arm` (`linux-arm64`): it prepares the pinned
   runtime with the shared `prepare-release-runtime` action, builds `silod` and
   the production `silo` command for the runner's architecture
   (`package-engine.py --platform linux-x64|linux-arm64`), and checks the
   checksum, the ELF architecture of `silod`, `silo`, `msb` and `git`, that
   `silo --print-channel-identifier` prints `org.silo.preview`, and
   `release.json` and the manifest. Only when all three builds pass, both Linux
   architectures included, does it create the GitHub release `engine-v<version>`
   with the three archives; a failed Linux build stops the whole release, and
   none is published with fewer archives. The release is not
   marked latest, so `releases/latest/download/latest.json` keeps pointing at
   the app. Each build compiles in the GitHub App configuration, like the app,
   from the same three repository secrets as the release workflow. The Linux
   binaries therefore carry the GitHub App client secret, as the macOS app and
   engine do. That is accepted: it is a public client's secret, extractable
   from any distributed binary, and GitHub treats the client secret of a native
   app as public (see [GitHub App configuration](#local-setup) above). It never
   authenticates a Silo installation to a backend.

Assets, at `https://github.com/amontlabs/silo/releases/download/engine-v<version>/`:

- `silo-engine-<version>-macos-arm64.tar.gz`
- `silo-engine-<version>-macos-arm64.tar.gz.sha256`, one line:
  `<sha256>  silo-engine-<version>-macos-arm64.tar.gz`
- `silo-engine-<version>-linux-x64.tar.gz` and
  `silo-engine-<version>-linux-arm64.tar.gz`, each with its `.sha256` in the
  same form

The macOS tarball has one top-level directory, `silo-engine-<version>/`:

```text
release.json                  engineVersion, sourceCommit, targets (aarch64-apple-darwin),
                              rustToolchain, schemaSha256, protocolMin, protocolMax
bin/silod                     arm64, not code signed
macos/Info.production.plist   without SiloEngineCodeRequirement
macos/Entitlements.plist
macos/LaunchAgents/org.silo.preview.engine.plist
macos/resources/engine-manifest.production.json   engineBuild = silo-<source commit>
api/silo-api.schema.json
```

Each Linux tarball is a complete engine directory, `silo-engine/`, that runs
where it is unpacked (`silod --service register` installs the systemd user
unit):

```text
release.json                  as above, targets x86_64- or aarch64-unknown-linux-gnu
silod                         not signed
silo                          the production silo command
silo-runner                   the CI runner helper (Go, static), version = the engine's
silo-ctr, silo-init           container computer helpers: the rootless namespace holder, and the
                              container's first process (silo-container-shim)
msb, git, git-lfs,            bundled tools, beside silod where the engine looks
git-remote-http, git-remote-https
resources/engine-manifest.json                      production, engineBuild = silo-<source commit>
resources/microsandbox/manifest.json
resources/microsandbox/licenses/
resources/microsandbox/<target>/lib/libkrunfw.so.5.6.1
resources/git-support/        Git manifest, licenses, share/git-core/templates,
                              ssl/ (cacert.pem, the certificate bundle Linux Git
                              verifies HTTPS with), lfs-transfer/ (the guest's
                              git-lfs-transfer)
resources/silo-runner-licenses/   licenses of the Go modules silo-runner links
api/silo-api.schema.json
THIRD-PARTY-NOTICES.md
LICENSE
```

`silo-runner` is built by `package-engine.py` itself (`go build` with `CGO_ENABLED=0`, `GOOS=linux`
and `GOARCH` of the platform, with the Go version in `app/SiloUI/runner/go.mod`), so every job that
packages a Linux archive installs Go first (`actions/setup-go`), and building one locally needs Go
1.27.3 (a newer Go downloads it when `GOTOOLCHAIN` allows). The release verification runs
`silo-runner version` and expects the engine version. The macOS engine archive does not carry it
(a Mac builds the helper with `go build`). Setup:
[CI runner, Linux hosts](SiloUI-CI-RUNNER.md#linux-hosts).

The runtime files are the ones `npm run runtime:prepare` stages for that
architecture; the script checks them against the MicroSandbox, Git and LFS
transfer manifests and checks every executable's ELF machine before it writes
the archive.

### Embedding the engine: the signer requirement

`silod` reads the string `SiloEngineCodeRequirement` from the Info.plist of its
own bundle at run time and accepts the bundle only if its signature satisfies
`(<requirement>) and identifier "<bundle id>"`. A production engine
(`org.silo.preview.engine`) refuses to start without a non-empty value. A
development engine (`org.silo.dev.engine`) falls back to
`identifier "org.silo.dev.engine"`. The bundle signature seals the Info.plist,
so a packager sets the key before signing:

```sh
plutil -replace SiloEngineCodeRequirement -string '<requirement>' "Silo Engine.app/Contents/Info.plist"
```

The requirement must match the signature the packager then applies. Silo's own
packaging (`scripts/embed_engine.py`) resolves `SILO_ENGINE_SIGNING_IDENTITY` to
exactly one valid identity (`security find-identity`), signs with its SHA-1, and
writes the requirement from that same certificate: for a Developer ID
certificate, `anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists
and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = "<TEAM>"`;
for any other certificate, `certificate leaf = H"<sha1>"`; and for an ad hoc
signature, `identifier "<engine bundle id>"`. `verify_engine` rejects a
production engine without it. There are no build-time
requirement variables.

### Build an archive locally

To develop an embedding app against unreleased Silo, build the same archive on
an Apple Silicon Mac and hand it to the app (Fjord:
`SILOD_ARCHIVE=/path/to/archive`):

```sh
python3 app/SiloUI/scripts/package-engine.py --out-dir /tmp/silo-engine
```

This writes `silo-engine-<version>-macos-arm64.tar.gz` and its `.sha256`.
`--source-commit` overrides `git rev-parse HEAD`. Use the Rust 1.94.0 toolchain
with the `aarch64-apple-darwin` target installed, and the local GitHub build
configuration from [Local setup](#local-setup). The script refuses a Cargo
version that differs from the production manifest's `engineVersion`.

A Linux engine builds on a Linux machine of the same architecture, after
`npm --prefix app/SiloUI run runtime:prepare` (see [Local setup](#local-setup);
the MicroSandbox build also needs `libclang-dev` and `libcap-ng-dev`):

```sh
python3 app/SiloUI/scripts/package-engine.py --platform linux-x64 --out-dir /tmp/silo-engine
```

`--platform linux-arm64` on arm64. `--channel development` packages a Silo Dev
engine (the development manifest and a development `silo`) for tests, as
`.github/workflows/linux-host.yml` does; never publish one.

## Primary references

- [Tauri updater and signed static feeds](https://v2.tauri.app/plugin/updater/)
- [Tauri AppImage packaging](https://v2.tauri.app/distribute/appimage/)
- [AppImage filesystem/runtime layout](https://docs.appimage.org/introduction/software-overview.html)
- [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/)
- [GitHub macOS 14 runner retirement (November 2, 2026)](https://github.com/actions/runner-images/issues/13518)
- [Apple library constraints](https://developer.apple.com/documentation/security/defining-launch-environment-and-library-constraints)
- [GitHub release immutability](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
- [GitHub deployment environments](https://docs.github.com/en/actions/deployment/targeting-different-environments/managing-environments-for-deployment)

Reviewed 2026-09-10. Distribution/update acceptance evidence is tracked in
`docs/SiloUI-DISTRIBUTION-PLAN.md`.

## Release CI caches

The release workflow runs frontend tests, type checking, lint, and Node release
checks once in a shared job. Draft creation requires that job to pass. Each
platform still runs Python release checks, native tests, updater checks, and
its packaging and signing verification.

`warm-release-caches.yml` populates caches on `main` when runtime inputs, dependency
manifests, vendor sources, compiler configuration or cache tooling change; it also
supports manual dispatch on `main`. Let its first
cold run finish before tagging a release to benefit from the cache.
[GitHub cache scope](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
allows tags to restore default-branch caches, but not caches from other tags.
Release jobs restore caches without saving them.

The shared `prepare-release-runtime` action caches pinned public downloads,
patched MicroSandbox executables and their checksums, the pinned Git LFS transfer
source and compiled guest server, and the guest archive. Cold LFS server builds
use Go 1.25 or newer; CI installs Go 1.25.x. The source archive is SHA-256 checked
before extraction and Go verifies module downloads against the pinned go.sum.
The server is cross-compiled with CGO disabled for the Linux guest architecture,
including on macOS hosts. Before accepting a staged or shared executable, preparation
resolves the effective compiler in the pinned module's Go 1.25.0 context. The
[upstream go.mod](https://github.com/charmbracelet/git-lfs-transfer/blob/971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b/go.mod)
contains no `toolchain` directive; cold builds verify that these selection lines
still match. This respects [Go toolchain selection](https://go.dev/doc/toolchain),
including `GOTOOLCHAIN` and module requirements. The executable manifest records
the selected Go version, requested toolchain, staging-script recipe digest, and
effective build flags, experiments, architecture tuning, and FIPS setting. A
change to any of these rejects both executable caches. Builds and compiler notices
use the selected compiler's GOROOT with further toolchain switching disabled.
The LFS manifest also records every bundled license file's SHA256; both caches
reject missing, changed or unexpected notices instead of packaging an incomplete
notice tree alongside a valid executable.
Keys include the runner, target, Rust and selected Go versions, staging scripts and runtime patch,
so app version changes alone do not invalidate the runtime. The guest image is not part of the
runtime cache.
Preparation always verifies and stages restored inputs and regenerates package
metadata. Cache misses follow the normal build path.

The patched MicroSandbox executable key also includes the staging-script recipe
digest and the requested `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`,
`CARGO_BUILD_RUSTFLAGS`, target-specific `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`, and
`CARGO_PROFILE_RELEASE_*` overrides, including build-script overrides. Profile
settings such as debug information and optimization change compiled bytes without
changing the source pin or the runtime's capability probes.
[Cargo documents these compiler inputs](https://doc.rust-lang.org/cargo/reference/environment-variables.html).
Changing them rebuilds the executable even when a fallback public archive restores
otherwise valid source, patch, compiler, and capability checks.

Release validation checks exact runtime-cache availability for each target using
`lookup-only` on the existing validation runner. Go setup runs before this lookup
so a newer 1.25.x patch release cannot count as an exact hit for older executables;
CI sets `GOTOOLCHAIN=local` at job scope in every job that prepares resources, so lookup,
preparation and the later Tauri build hook record the same toolchain policy. Warm platforms start their native
and package jobs directly, without an extra producer runner or artifact transfer.
A missing exact cache starts one credential-free runtime producer inside that
platform's `release-platform.yml` invocation. Its native and package jobs wait for
that producer; another platform's runtime does not block them. Sequential benchmark
mode still runs native tests in the package job before release compilation.

The cold producer archives only the existing public runtime-cache allowlist. The
archive preserves executable modes and excludes application build products, local
configuration, source-build work directories, and staged `release-info.json`.
Consumers require the producing job's archive SHA256, reject unsafe paths and
links, and then run normal preparation to validate and stage inputs for the current
release. No release job saves a shared cache; only the main-branch warmer does.
If an exact cache is evicted or damaged after lookup, ordinary preparation retains
its safe local rebuild fallback. Cache reuse is an optimization, not a prerequisite
for correctness.

The lookup and restore use the same key and literal path list. GitHub's
[cache version implementation](https://github.com/actions/toolkit/blob/main/packages/cache/src/internal/cacheUtils.ts)
combines those paths, compression method and format salt; its platform discriminator
applies only to Windows. Our Linux and macOS runners use zstd. The archive transfer
adds an explicit digest failure check because GitHub's
[artifact download validation](https://docs.github.com/en/actions/tutorials/store-and-share-data#validating-artifacts)
reports a digest mismatch as a warning.

Cargo download caches contain registry indexes, downloaded crates, and Git databases,
following the [Cargo home guidance](https://doc.rust-lang.org/cargo/guide/cargo-home.html).
A separate reviewed dependency cache contains selected compiled crates.io dependencies
for the exact release profile and target. It never contains the full Cargo target
tree, application executables or fingerprints, workspace/path/git package outputs,
or local configuration. Registry build-script products are included only when the
exporter attributes them to an approved locked crates.io package.

Only the credential-free `main` warmer compiles and exports that dependency cache,
using explicit synthetic GitHub configuration and no signing environment. An exact
cache lookup skips compilation when the cache already exists. Before saving, the
exporter audits package ownership, artifact hashes, paths and modes, verifies registry
source bytes against locked crate archives, and rejects the synthetic secret marker.
Release jobs only restore; they never export or save their credentialed build products.

Dependency keys include compiler and SDK identity, native toolchain versions, target,
release profile, dependency graph and features, lockfile pins and checksums, vendor
sources, Cargo configuration, and compiler environment overrides. Only the excluded
root application's version is normalized across Cargo/Tauri manifests, the lockfile
and root graph references. Dependency versions and checksums remain exact. Consumers
compute their own context before artifact-only verification changes the updater key.
The importer independently validates the current graph and lockfile, then checks all
cached artifacts and installed registry source bytes before restoring source timestamps
and approved dependency products. A missing cache, cache-service failure or rejected
import follows ordinary compilation with an empty dedicated release target.

CI release compilation and bundling use
`app/SiloUI/src-tauri/target/release-compile/<target>/release/`; bundles are under its
`bundle/` directory. Local build paths documented above are unchanged. Every release
still compiles the application, and the build rejects Cargo output reporting the
application executable as fresh. Native and updater tests retain their ordinary test
targets and run on every platform; dependency reuse removes no release gates.

The 0.3.1 macOS release spent about 14 minutes preparing its runtime. Reusing the
patched runtime targets that cost; actual savings must be measured on a release
with a warm cache. The first cache-warming run still pays the cold build cost.

## Fast feedback and phase measurements

Run `npm --prefix app/SiloUI run preflight` before a native build. This reads the
approved `runtime-inputs.json`, checks its schema, supported targets, source pins,
features and the actual patch digest without network access or Rust compilation.
Runtime preparation and draft creation run it automatically. JavaScript staging
and Rust dependency validation consume this same file; changing runtime pins
requires reviewing it and verifying downloaded bytes during staging.

Ordinary GitHub build and permission checks live in the binary's `cfg(test)`
modules. This preserves their assertions while avoiding Cargo's additional
normal debug executable build for integration tests. Use a Cargo test filter
for focused feedback, then run the full native suite once for combined changes.

Vitest runs reviewed browser-independent suites in Node and retains jsdom for
all other suites. The global worker limit remains overridable with
`npm --prefix app/SiloUI test -- --maxWorkers=2`; do not inherit that limit into
individual projects because project limits override the CLI root limit.

The release workflow runs native and updater checks on macOS in parallel with
the app build. The draft job requires the native checks, frontend checks,
package checks and minimum-macOS checks to pass. Native
test jobs receive no signing credentials. Only the reviewed public release dependencies
described above are cached; application and native-test products are excluded.

Workflow checkouts set `persist-credentials: false`. The [pinned checkout action](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/action.yml)
defaults to retaining its token for later authenticated Git commands. These jobs
need Git authentication only during checkout; publication and package downloads
receive explicit step-scoped tokens. Disabling persistence keeps that token out
of subsequent build and test Git commands. Job permissions remain read-only by
default, with existing write permissions confined to publication jobs.

Reusable workflow and runtime-action string inputs enter shell commands through
quoted environment variables. GitHub expands expressions before parsing inline
scripts, so quoting a `${{ inputs.target }}` expression alone does not prevent
script injection. See [GitHub's script-injection guidance](https://docs.github.com/en/actions/reference/security/secure-use#use-an-intermediate-environment-variable).
The workflow regression runs extracted commands against disposable executables
with ordinary targets, quote-breaking input and command-substitution input.
Current release callers supply fixed matrix values; this protects the reusable
input boundary without changing release gates or published asset names.

CI, including its release-tooling checks, cancels obsolete runs for the same
pull request and workflow. Push runs use their run IDs, so they stay
independent. [GitHub concurrency groups](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency)
are repository-wide; including the workflow name prevents cross-workflow
cancellation. Publication and release-build concurrency policies are unchanged.

Artifact-only runs have independent concurrency groups, so they do not queue
behind or displace a pending publication. Tagged and draft publications of the
same release tag share one concurrency group per tag, so builds of different
releases never cancel each other while waiting. Optional `benchmark_ref` pins every
checkout to a full 40-character source commit while using the dispatched
workflow definition. It is rejected for publication; validation logs both
workflow and source commits before checkout. Omit it for normal releases.

For a controlled CI comparison, dispatch the same source commit twice with
`draft=false`, once with `benchmark_schedule=sequential` and once with
`benchmark_schedule=parallel`. Sequential mode runs native checks before package
compilation and is rejected for draft creation. Compare job/step timestamps and
the `native-timings-*` / `package-timings-*` JSON artifacts. The measurement
wrapper preserves command failures and records elapsed time, CPU use and peak
child-process RSS without command arguments, environment variables or logs.
Runner allocation and caches vary; a single comparison does not establish a
guaranteed speedup.

Runtime cache fallback restores public input candidates only. Preparation still
validates downloaded digests and the patched executable's build key. That key
includes embedded agentd bytes as well as source, patch, target, compiler and
features. The expanded key requires one initial rebuild of the patched runtime.
Only the credential-free default-branch warmer may populate shared caches.

Bundling retries the observed AppImage type2-runtime and Tauri vendor-tool
download failure chains for HTTP 500/502/503/504, at most three attempts with 2s/4s backoff. Every attempt
streams output and remains in the bundle log. Compilation, runtime preparation
and package validation are outside this retry boundary; other errors fail
immediately. This handles transient upstream download failures without
repeating the expensive build phases.

Each bundle attempt runs in its own process group. If forwarding stdout/stderr
or writing the local log fails, the wrapper stops that group and reaps its
bundle command before propagating the error. Python's
[`Popen` context manager](https://docs.python.org/3/library/subprocess.html#subprocess.Popen)
waits for the child on exit; it does not stop it on an exception. The synthetic
stream-failure regression checks all three output destinations and verifies
that the child is reaped. A descendant fixture inherits a separate pipe; EOF
verifies that it also exits after forwarding fails. This does not test a real
package build.

The release dependency-cache build also owns its command's process group.
Reading compiler output, forwarding diagnostics, and recording artifact JSON
must complete before the command can be released. An exception stops the group
and reaps its leader before the metadata file closes. Its synthetic regression
injects a forwarding failure after a fixture command starts, verifies a signal
exit, and checks that `waitpid` reports no unreaped child.

### Disk-space diagnostic units

Low-space messages for update installation, computer image preparation, and
(formerly) ChatGPT app downloads express their existing binary byte calculations as MiB.
[NIST's binary-prefix definitions](https://physics.nist.gov/cuu/Units/binary.html)
distinguish one MiB (1,048,576 bytes) from one MB (1,000,000 bytes). The previous MB
label understated the represented byte amount. Update preflight regressions cover
an exact 2 MiB shortfall and one extra byte, which must round up to 3 MiB, while
preserving the installed fixture and cleaning up the staging probe.

Verification: three Rust tests against the extracted production update preflight
functions passed after the rounding-boundary regression failed with the old MB
label. Rust formatting, TypeScript typecheck, lint, and whitespace checks passed.
The full native test compile could not use the cached Tauri dependency because
new integration tests require its test feature; no packaged app was built or run.
