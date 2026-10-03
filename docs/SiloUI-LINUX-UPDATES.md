# Linux system updates

Silo's Debian installer registers a signed APT source, so Ubuntu/Xubuntu can
install subsequent releases through Software Updater. AppImage remains an
optional portable download with its existing in-app updater. macOS is unchanged.

## Installation and migration

Install the new Debian package once and accept **Receive Silo updates through
Software Updater?**. A noninteractive installation uses the default, enabled.
The package installs a repository-specific public key and source definition;
it never adds global APT trust or changes Ubuntu's own sources.

An older Silo installation needs this installer once. An in-app banner alone
cannot register the source, and the existing released installers do not contain
these hooks. After enrollment, the normal system refresh discovers Silo updates.
No repeated package downloads or terminal commands are needed for normal updates.
The in-app update card shows the available version. Collapsed **How to install**
help explains the Software Updater route, refreshing APT when no update appears,
and installing a downloaded Debian package with APT when the graphical installer
does not work. **View installers on GitHub** opens the release page, where
**Assets** lists the installers. The button does not download or install a package.

The **Update** action in Silo refreshes package information and requests system
authentication before upgrading and restarting. For updates started outside Silo,
quit Silo before applying the system update. Closing its window is not Quit.
Quitting stops local computers; computers on other devices keep running. The installer refuses to
replace a running packaged Silo or its runtime, names the blocking process, and
never kills either. Short-lived Connections relays (`silo-ui
--remote-bridge` started by SSH from another device, and `--remote-guest`
editor connections) hold no computer or app state, so they do not block an update.
New package versions also refuse startup while installation is in progress.
The first migration from an older version cannot enforce that startup guard in
old code, so keep Silo closed during this first installation.

The installed source is `/etc/apt/sources.list.d/silo.sources`, with trust limited
to `/usr/share/keyrings/silo-archive-keyring.gpg`. Software & Updates can disable
it. Upgrades preserve a deleted source and administrator edits such as
`Enabled: no`. `sudo dpkg-reconfigure silo` offers enrollment again. Removal
cleans up an unchanged source; administrator-edited files are retained.
If an edited source remains enabled after removal, disable it in Software &
Updates or remove it: the package-owned signing key is no longer installed.
A system crash during installation can leave a startup guard; finish the
interrupted package transaction with `sudo apt --fix-broken install`.

## Publishing

The source URL is `https://apt.silo.amontlabs.com/apt`, suite `stable`,
component `main`, architectures `amd64` and `arm64`.

`.github/workflows/apt-repository.yml` runs after **Publish verified Silo draft**
succeeds, manually, and weekly to refresh expiring metadata. It downloads only
public stable releases, verifies both package checksums against `SHA256SUMS`,
validates their internal package/version/architecture, and signs the indexes.
Because `SHA256SUMS` is part of the same editable release, each release must also
be immutable, and every package must match GitHub's release attestation
(`gh release verify-asset`), which GitHub signs at publication and later edits
cannot change. A mutable or unattested release stops the job before signing.
Drafts never enter the repository. The latest release must be the numerically
highest stable release and must not change during publication.

GitHub Pages hosts the latest two complete releases, signed `InRelease` and
`Release.gpg`, and deterministic package indexes. Signed metadata expires in
14 days. Previously signed indexes are retained by hash for clients refreshing
cached metadata. The job fails above a 900 MiB site budget; move the package pool
to larger hosting before increasing retention. Failed builds leave the deployed
site intact. Monitor workflow failures: expired metadata intentionally stops
clients accepting stale updates. GitHub may disable scheduled workflows in a
public repository after 60 days without activity; keep the schedule enabled.
See [GitHub scheduled workflow behavior](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule).

Infrastructure setup:

1. Enable GitHub Pages with GitHub Actions as its build source, and keep release
   immutability enabled in the repository settings.
2. Create an `apt-publish` environment restricted to the `main` branch.
3. Store the dedicated ASCII-armored private archive key in that environment's
   `SILO_APT_SIGNING_KEY` secret. Do not use the Tauri updater key. Never commit
   private key material or place it in build artifacts.
4. Merge the workflow, run **Publish Silo system updates**, and verify its Pages
   deployment before publishing an installer that enrolls users.
5. After every application release, confirm the APT workflow succeeds and an
   enrolled test device sees the new candidate through `apt-cache policy silo`.

The pinned public key fingerprint is
`D870CA15D275FDB538DDC5516F36347AE839F67A`; it expires 12 September 2029.
The public key and fingerprint live in `app/SiloUI/scripts/debian/`.
Keep the private key in a secure backup. Rotate well before expiry: distribute
an installer keyring containing both keys while the old key remains trusted,
then switch signing after clients have that keyring. Never replace the trust
anchor and signing key simultaneously without a migration.

## Verification

Run Python release-tool tests normally. The repository tests require Linux
`apt-get`, `dpkg-deb`, `gpg`, and `gpgv`:

```sh
python3 -m unittest discover -s app/SiloUI/scripts -p 'test_apt_repository.py'
```

They generate disposable signing keys and Debian fixtures, verify that APT
selects the latest candidate, reject tampered metadata and package bytes, and
check retained indexes and complete architecture pairs.

The package lifecycle test installs and removes a dummy `silo` package. Run it
**only in a disposable Linux container or CI runner**, as root:

```sh
SILO_APT_LIFECYCLE_TEST=1 python3 -m unittest discover -s app/SiloUI/scripts -p 'test_debian_installation.py'
```

It covers enrollment, opt-out, a running process refusing replacement without
being killed, successful APT installation, administrator edits, source deletion,
and cleanup. These tests exercise real APT/dpkg with fixtures. They do not prove
Xubuntu's graphical updater, native computer health, or a production release upgrade.

## Local evidence, 13 September 2026

- Debian Bookworm disposable container, real APT/GPG/dpkg: 8 repository tests
  and the installer lifecycle test passed, including an APT upgrade between
  dummy versions. Production credentials were not used by tests.
- Python release tooling: 42 tests passed. Node release tooling: 16 passed.
- Frontend update and onboarding tests: 19 passed; typecheck and lint passed.
- The actual v0.2.2 Debian control archive contains no maintainer hooks that
  conflict with the new packaging step. Both workflow YAML files parse.
- Repository-wide `cargo fmt --check` reports existing formatting differences;
  unrelated Rust code was not reformatted. No new native application bundle,
  live Xubuntu GUI session, or production system update was exercised here.

## Primary references

- [VS Code Linux installation](https://code.visualstudio.com/docs/setup/linux): existing Debian installer enrollment pattern.
- [Ubuntu third-party repositories](https://ubuntu.com/server/docs/explanation/software/third-party-repository-usage/): repository-specific trust.
- [APT source definitions](https://manpages.debian.org/testing/apt/sources.list.5.en.html): deb822 and `Signed-By`.
- [Debian repository format](https://wiki.debian.org/DebianRepository/Format): hashes, relative package paths, and signed metadata.
- [Debconf developer guide](https://manpages.debian.org/unstable/debconf-doc/debconf-devel.7.en.html): installer choices.
- [GitHub Pages custom workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages): deployment permissions and artifacts.

## Reported empty graphical updater, 14 September 2026

The public [amd64 package index](https://apt.silo.amontlabs.com/apt/dists/stable/main/binary-amd64/Packages)
advertised Silo 0.4.4 during investigation. The corresponding
[APT publication run](https://github.com/amontlabs/silo/actions/runs/34852064374)
succeeded. Missing publication was therefore not reproduced. This does not
verify the user's installed version, repository enrollment, local APT cache,
or graphical package installer. Obtain the installed Silo version and
`apt-cache policy silo` from the affected computer before attributing a cause.
Automatic retry scheduling already shipped in 0.3.3; no additional scheduler
change was made for this report. The UI change was checked with frontend
fixtures, without launching a packaged app or touching live Linux state.

## Automatic discovery regression, 14 September 2026

The previous native loop waited 30 seconds at launch and then slept 24 hours
regardless of success, failure, or a skipped operation. Enabling checks did not
wake it. This explains a reproducible failure mode: start offline, reconnect,
and no automatic retry occurs that day. It does not establish which failure
occurred on the reported Linux installation.

The native schedule now checks after five seconds, retries failed requests after
one minute with exponential backoff capped at 15 minutes, and checks daily after
success. A five-second poll uses wall-clock deadlines so an overdue check runs
after Linux resumes. Enabling automatic checks makes the next poll eligible.
Admission and manual operations share the state lock; background checks preserve
discovered updates, failed downloads, and verified installers.

The updater remains host-owned. Tauri's [updater documentation](https://v2.tauri.app/plugin/updater/)
describes explicit check, download, and install operations and mandatory signature
verification. The application implements scheduling around the pinned plugin;
no automatic download or installation was added.

The packaged AppImage test (`app/SiloUI/scripts/test-linux-update.py`) now waits
for automatic discovery without clicking Check for updates before exercising
interrupted downloads, signature rejection, replacement, and restart. Run it on
Linux with the isolated signed AppImages described in its header. Unit tests
exercise the schedule with supplied times, including failure recovery, disabled
checks, busy operations, and resume. Those tests do not prove a live Linux GUI
upgrade or APT publication.

Verification on the macOS development host used deterministic fixtures and
synthetic GitHub build configuration for native tests:

- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml updates::`: 13 passed.
- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml runtime::update_recovery::`: 6 passed.
- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked -p tauri-plugin-updater --lib -- --test-threads=1`: 8 passed; one subprocess-only helper ignored. Loopback server tests required local network permission.
- `npm --prefix app/SiloUI run test:release`: 16 passed.
- `python3 -m unittest discover -s app/SiloUI/scripts -p 'test_release_metadata.py'`: 4 passed.
- Frontend typecheck, lint, and Python syntax validation of the packaged upgrade test passed.

No native bundle was built or inspected, and no live VM or installed Linux
application was touched. The packaged Linux acceptance test remains required
before claiming release readiness.

The full frontend run passed 734 of 735 tests; its sole failure expected the
installation status in the old component. After updating that assertion for
the shared installation guard, the affected application/update suites passed
all 111 tests across four files. The other 76 test files passed in the full run.
The guard now spans settings-save preparation and onboarding as well as native
installation; the main application's existing installation guard remains.

## In-app Debian updates, 14 September 2026

The reported machine had Silo 0.4.4 installed and an APT candidate of 0.4.4,
while the published repository already contained 0.5.0. Silo's GitHub release
check does not refresh APT's local indexes. The App Center's disabled Install
button was not reproduced and is not attributed to the stale index alone.

Debian installations now offer **Update** inside Silo. The operation flushes
settings, acquires the existing installation and VM mutation guards, records
running local VMs for recovery, and stops them after confirmation. It invokes
only `/usr/bin/pkexec --disable-internal-agent /usr/lib/silo/silo-system-update`
with the host PID and validated release version. The root-owned Python helper
runs in isolated mode, accepts no package/source/command override, and uses a
fixed environment and absolute executable paths. Its polkit action requires
administrator authentication for every operation.

The helper refreshes the configured Silo source, treats failed refreshes as
errors, downloads authenticated packages, and upgrades the exact selected Silo
version without allowing removals, downgrades, or unauthenticated packages.
Missing or disabled source configuration remains disabled and produces an
actionable error. APT lock failures and diagnostics are shown inside Silo.
Settings and VM disks are not package payloads. The native recovery journal
restores the prior local running set after restart or a failed operation.

Ordinary Debian installations still refuse replacement while Silo or its VM
runtime is running. During an in-app update, a root-private permit identifies
only the guarded app PID/start time and live helper PID/start time. The
maintainer script permits that inert app alone; other Silo instances and all
running local runtimes still block installation. The helper removes its permit
on completion; a dead helper or reused PID cannot authorize a later upgrade.
After successful version verification, Silo executes `/usr/bin/silo-ui` directly
to restart, avoiding the deleted old executable and unrelated AppImage paths.

Returning to the main window makes a background check eligible after a one-minute
cooldown. The native scheduler still honors disabled automatic checks, busy
operations, pending updates, and offline retry backoff. Progress is visible
outside the UI installation guard.

This cannot retrofit the Update button into an already running 0.4.4/0.5.0
binary. Install the first release containing this helper through the existing
system package route once; later updates use the new in-app action.

Primary references: [APT update/install semantics](https://manpages.debian.org/bookworm/apt/apt-get.8.en.html)
require refreshing indexes before package selection and document exact versions,
download-only, and removal/authentication safeguards. [pkexec](https://polkit.pages.freedesktop.org/polkit/pkexec.1.html)
documents session authentication, policy executable paths, argument validation,
and cancellation status. The implementation validates its own arguments; polkit
is not treated as an argument validator.

Verification for the in-app change: 16 native updater tests passed with synthetic
GitHub configuration; 130 frontend tests passed across the application/update
suites, followed by the additional production progress-boundary test. The real
Ubuntu 24.04 disposable-container lifecycle tests passed, including a signed HTTP
repository initially cached at 0.1.0, publication of 0.2.0, refresh/download/install,
refusal with a second Silo process, version verification, cleanup, and downgrade
refusal. The process fixtures use a copied sleep executable, not live VMs. The
GUI authentication prompt and a live desktop VM restart were not exercised.

## Stopping computers only for installation, 29 September 2026

Review finding F-04: the in-app update stopped every local VM before system
authentication, the source check, the APT refresh and the download, and waited
without limit while holding the installation and VM operation gates. Cancelling
the authentication prompt, a disabled source, another package manager holding the
lock, or a release not yet published to APT stopped and restarted every VM, and
Quit waited behind the prompt.

The helper now prints `ready` after the download and waits up to 30 minutes for
`install` on its standard input (pkexec passes standard input through). Silo
answers only after flushing settings, recording the running set and stopping it;
any other answer, end of input or the timeout installs nothing. An already
installed target version uses the same handshake before Silo restarts. Silo waits
up to 30 minutes for `ready` while computers keep running and new operations are
refused; the VM operation gate is taken only for the install stage, so Quit is
not queued behind authentication. On timeout Silo closes the helper's input, stops
the prompt if authentication has not completed, and reports that no computer was
stopped. A failure after `install` restores the recorded running set. APT's
existing 60-second lock timeout bounds the install stage. Unit tests use shell
fixtures for the handshake and Python tests for the helper; the packaged pkexec
prompt and a live Debian upgrade were not exercised.


## Failed process replacement, 2 October 2026

After a successful Debian installation, Silo keeps its installation and VM
operation guards, closes helpers, releases its single-instance claim and executes
the updated binary. If replacement fails, it exits with status 1. Reopen Silo
to acquire the claim and restore the saved running set from the update journal.
The old process never cancels shutdown or resumes computer recovery after releasing
its claim. Settings were flushed before installation, so this exit intentionally
skips the ordinary exit handler and its additional state writes.

The pinned [single-instance plugin 2.4.5 public API](https://github.com/tauri-apps/plugins-workspace/blob/single-instance-v2.4.5/plugins/single-instance/src/lib.rs)
provides initialization and destruction, but no verified reacquisition operation.
Its [Linux implementation](https://github.com/tauri-apps/plugins-workspace/blob/single-instance-v2.4.5/plugins/single-instance/src/platform_impl/linux.rs)
releases the D-Bus name on destruction; its connection state is private.
Re-registering the plugin cannot establish ownership through the existing managed
connection. Silo therefore terminates rather than adding a separate ownership
mechanism. Rust's [exec contract](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.exec)
returns only on failure and warns that process state may have changed.
[process::exit](https://doc.rust-lang.org/std/process/fn.exit.html) terminates all
threads without running Rust destructors, matching the successful replacement
path's cleanup requirements.

The regression test uses isolated child processes with temporary HOMEs. It
injects a real missing-executable failure and replaces a test process with
`/bin/sh` for the success case. Both cases close the synthetic claim first and
prevent a subsequent mutation marker; failure exits with status 1 and explains
that Silo must be reopened. These checks do not exercise session D-Bus, a second
Linux app, package installation, or live VM recovery. Native Linux acceptance
still requires a disposable session before claiming release readiness.
