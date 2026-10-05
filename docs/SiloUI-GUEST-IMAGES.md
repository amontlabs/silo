# Silo guest images

Silo uses one recommended Ubuntu 24.04 image for the app's CPU architecture. Installers do not
contain it: Silo downloads it once, on first use ([below](#download-on-first-use)). Sections that
describe verification of an image "bundled" in the app record releases up to 0.11.x.
curl, Git, Git LFS, gh, CA certificates and Silo's credential helper are installed while
building that image. The v3 image also bundles sudo, Python 3 and
OpenSSH's SFTP server for offline working-account provisioning. Silo creates
the working account when it creates a computer; no Silo working account, token,
identity or user data enters the image.
Additional supported Ubuntu releases can be provided as prepared downloads later;
there is no version picker or arbitrary-image compatibility promise in this change.

## Publication and app builds

The public standard container package is
`ghcr.io/0xpolarzero/silo-guest:ubuntu-24.04-v4`, with `-arm64` and `-amd64` tags (v3 remains published).
The matching [versioned release](https://github.com/amontlabs/silo/releases/tag/guest-ubuntu-24.04-v4)
contains compressed Docker-save archives, package inventories in JSON manifests,
SHA256SUMS, the recipe, setup script and source commit. The image itself retains
Ubuntu's package copyright files under `/usr/share/doc`.

`.github/workflows/guest-image.yml` publishes using GitHub's short-lived job token
with package and release write permissions, only after a reviewer approves the
`guest-image-publish` environment. Create that environment once with required
reviewers and a deployment branch rule for `publish-guest-*`; the workflow refuses
to publish while the environment has no required reviewers. The recipe's OCI source and revision
labels link the image to its code. Check package visibility after first publication and change it to Public if needed;
anonymous pulls must be verified. This publication was already public.
Published version tags are never intentionally reused. For an image update,
increment `GUEST_IMAGE_VERSION` in `app/SiloUI/scripts/build-guest-image.mjs`
(and the recipe as needed); the workflow derives the release tag, title and
container image names from it and from the publishing repository. The workflow refuses
publication once its companion release, an architecture tag, or the multi-architecture tag exists. If publication
fails halfway, recover the exact already-built artifacts; do not rebuild over the
version. Otherwise increment the version.

Guest publication keeps one shared concurrency group with GitHub's supported
`queue: max` setting, so up to 100 pending runs wait instead of replacing one
another. This preserves distinct version requests while serializing their
existence checks and pushes. The default queue holds only one pending run even
with `cancel-in-progress: false`; a third request cancels the second. See
[GitHub's concurrency queue contract](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency).
GitHub [released this setting on May 7, 2026](https://github.blog/changelog/2026-05-07-github-actions-concurrency-groups-now-allow-larger-queues/).
Actionlint 1.7.12 still rejects that supported key, tracked in
[upstream issue 680](https://github.com/rhysd/actionlint/issues/680). When using
that version locally, suppress only its `unexpected key "queue" for "concurrency"
section` diagnostic; keep the policy test that requires `queue: max` and
`cancel-in-progress: false`, and keep all other validation enabled.

Publication preflight reads GitHub's release-by-tag endpoint and GHCR's manifest
endpoint with the job token. Only a confirmed release HTTP 404 and registry
HTTP 404 with `MANIFEST_UNKNOWN` or `NAME_UNKNOWN` permit a build. Authentication,
transport, rate-limit, and server errors stop the job. Redirects are rejected so
credentials stay on the fixed GitHub and GHCR hosts; diagnostics omit response
bodies and credentials. The job token remains step-scoped and registry access
uses a pull-scoped bearer token. This check does not reserve tags against writers
outside the workflow's concurrency group.

The supported Docker inspector cannot distinguish these failures reliably:
its [registry client](https://github.com/docker/cli/blob/master/internal/registryclient/fetcher.go)
can return the same missing-manifest error after unauthorized or unexpected HTTP
responses. The preflight therefore uses Python's standard HTTP client for
read-only API requests, without a new registry tool or credential service.
The [OCI Distribution 1.1.1 manifest and error contract](https://github.com/opencontainers/distribution-spec/blob/v1.1.1/spec.md)
provides typed registry failures; [GitHub's container registry authentication](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)
supports the existing Actions job token. Keep builds and pushes in the maintained
Docker tools; this script only enforces Silo's refusal to reuse a version.

The QEMU setup action uses its supported `image` input to pin the privileged
installer to `tonistiigi/binfmt@sha256:400a4873b838d1b89194d982c45e5fb3cda4593fbfd7e08a02e76b03b21166f0`.
Docker Hub's manifest index resolved on 2026-10-02 identifies version
`qemu-v10.2.3-68`, source revision `e29e7d72c9672c8c8bf846655ab149b50e1a62bd`,
and MIT licensing; it includes Linux AMD64 and ARM64 hosts. The Ubuntu AMD64
publishing runner installs only ARM64 emulation. Keep this maintained upstream
installer: no custom emulator installation is needed. Review and update the
digest explicitly when updating QEMU. Pinning prevents tag drift; it does not
remove the installer's host privileges or establish absence of vulnerabilities.
The pinned action's [input contract](https://github.com/docker/setup-qemu-action/blob/99012661954931238ded8c8b007157a8430204e1/action.yml)
and [implementation](https://github.com/docker/setup-qemu-action/blob/99012661954931238ded8c8b007157a8430204e1/src/main.ts)
confirm the default is mutable and installation uses privileged containers.

`app/SiloUI/guest-image/image-lock.json` pins the release URL (the repository's
`guest-ubuntu-24.04-v4` release in `amontlabs/silo`), and for each architecture the exact release
archive SHA-256, length, uncompressed archive length, image reference and Docker config digest.
The lock is the single source of truth: the app embeds it at build time
(`include_str!` in `src-tauri/src/guest_image.rs`) and downloads
`<releaseUrl>/image-<arch>.tar.gz`. `npm run runtime:prepare` and the Tauri bundle no longer
stage or contain the image, so it does not enter installers, updater archives or the APT
repository. Changing the lock changes the next build and nothing else: published installers keep
the image they were built with pinned.

`runtime:prepare` streams the MicroSandbox, Git and Git LFS inputs through incremental SHA-256
verification into an exclusive temporary file beside its destination, then renames it only after
verification succeeds. Downloads of inputs without a pinned length have a 1 GiB cap, and license
downloads have a 4 MiB cap.
See [streaming build-input measurements](research/stream-build-inputs-2026-10-02.md)
for peak memory, preparation timings and regression coverage.

To produce a candidate image locally, with Docker available:

```sh
node app/SiloUI/scripts/build-guest-image.mjs arm64
node app/SiloUI/scripts/build-guest-image.mjs amd64
```

The CLI also accepts a symlink to the script. Entry-point detection uses Node's
[canonical path resolution](https://nodejs.org/download/release/v24.11.1/docs/api/fs.html#fsrealpathsyncpath-options)
so a symlink runs the requested command instead of silently exiting; importing
the module for tests still performs no build.

Build outputs are ignored under `src-tauri/guest-image-artifacts/<architecture>`.
Docker export is compressed into a temporary archive. The builder waits for both
the export process and compression to succeed before replacing `image.tar.gz`.
A failed export removes temporary output and preserves the previous archive and
manifest; it never marks a partial gzip stream as the new candidate.
The Dockerfile-specific ignore file limits the Docker context to the recipe and
setup script. Build credentials and unrelated app files are not sent to Docker.
The base Ubuntu index is pinned. Apt packages are resolved at image publication
and their complete versions recorded; this is a tested, immutable distributed
artifact, not a promise that rebuilding the recipe later yields identical bytes.
App builds reuse the publication, not a fresh apt installation.

After publication, review both attached manifests and copy them into the lock's
`images.arm64` and `images.amd64` entries. Verify both archives against those
manifests before committing the lock. Updating a lock does not update existing
computers; restored backups also retain their guest systems.

## Guest image v3 publication

This records the earlier v3 publication and account verification. The
[current lock](../app/SiloUI/guest-image/image-lock.json) pins v4, described below.
The v3 recipe added `sudo`, `python3` and `openssh-sftp-server`. Account setup
uses these tools locally and refuses an image missing them. New computer creation
must not download or repair packages to establish the working account. At v3
publication, the optional desktop used separate package and KasmVNC downloads;
the [current desktop recipe](SiloUI-DESKTOP.md) uses Selkies.

The public [v3 release](https://github.com/amontlabs/silo/releases/tag/guest-ubuntu-24.04-v3)
was produced by [publication run 35546417121](https://github.com/amontlabs/silo/actions/runs/35546417121)
from source `a9827c263df3daee28959b2c2073d85c6f980e9d`. The v3 lock at publication
recorded these archives:

| Architecture | Compressed bytes | SHA-256 |
| --- | ---: | --- |
| ARM64 | 85,767,229 | `03f592e602afb0fff724a1f82a5866571356d398b4a3edae1bc15d3a7360efc0` |
| AMD64 | 87,768,822 | `8a3bf159c1d038626ef98a6e7c505ba1834a259f41886f4d14eba6a99d4f9566` |

Earlier local candidates passed seven Docker tests per architecture with
networking disabled. The ARM64 candidate passed account provisioning,
exec/SSH identity, SFTP/SCP permissions, Git/LFS roundtrips and restart
persistence with `--net none`; all eight missing-tool cases failed preflight
without package-manager calls. The isolated macOS debug bundle also passed
this suite using its bundled candidate. Evidence is under
`target/verification/working-account/offline-v3/` and `offline-v3-packaged/`.

Those candidates differ from the published archive hashes. The actual published
ARM64 archive subsequently passed all 13 offline live test groups using the
signed packaged runtime, with its public archive hash verified. Evidence:
`target/verification/working-account/offline-v3-published/live.log`.
The GUI was not launched, and Linux/KVM execution remains untested.

## Guest image v4 recipe (published)

`GUEST_IMAGE_VERSION` is `ubuntu-24.04-v4`. v4 is published as the
[`guest-ubuntu-24.04-v4` release](https://github.com/amontlabs/silo/releases/tag/guest-ubuntu-24.04-v4),
built from source commit `aae2ed939339cb559d81915b4e8df8a150cb8a90` (its
`source-commit.txt`), and `guest-image/image-lock.json` pins it: both manifests are
copied into the lock, and new computers use the image with the built-in desktop.
Compressed archives are 414,843,188 bytes (ARM64) and 423,476,714 bytes (x86-64).
That release predates attaching `src-tauri/guest/lcu-lock.json`, which the image
build consumes; the publication workflow now attaches it, so later releases include
it. The published v4 release itself is unchanged. Publication needs the owner's
reviewer approval in the `guest-image-publish` environment.

v4 builds on the unchanged v3 layer (`setup-github.sh`, sudo, Python 3, OpenSSH
SFTP server) and adds, in one further layer:

- **Desktop.** The package list of `src-tauri/guest/setup-desktop.sh` with the same
  `--no-install-recommends` (Xfce session, panel, settings, xfwm4, Thunar, terminal,
  Greybird, Xvfb, PulseAudio, AT-SPI core and the X utilities), and the pinned
  Selkies 2.0.0 `.deb` for the build architecture from `desktop-streamer-lock.json`.
  The `.deb` is downloaded, SHA-256 verified, installed and deleted inside one
  `RUN`; the lock and the poller are `RUN --mount=type=bind` inputs. Never `COPY` a
  package file and delete it later: that keeps it (about 60 MB) in a layer.
  The runtime pieces of `setup-desktop.sh` (Selkies web-client patch, connection
  credentials, receipts, `silo-desktop`) are not part of the image.
- **ChatGPT and LCU system libraries.** The LCU `SYSTEM_PACKAGES` (checked against
  v0.8.1, unchanged through v0.8.4; v0.8.5 adds libxres1, which the published v4 image already contains,
  [source](https://github.com/0xpolarzero/lcu/blob/v0.8.1/scripts/install.py)),
  a superset of the ChatGPT Linux `.deb` dependencies on Ubuntu 24.04, so LCU
  installs with `--skip-system --offline`. No OpenAI file and no installed LCU
  are in the image.
- **LCU archive.** `src-tauri/guest/lcu-lock.json` is the one place that pins LCU
  (version, and URL and SHA-256 per architecture; the ChatGPT app lock's
  `lcuVersion` must agree, which a test checks). The same `RUN` downloads the
  archive for the build architecture, verifies it with `sha256sum --check` and
  keeps it unextracted as `/usr/local/share/silo/lcu/lcu-<version>-linux-<arch>.tar.gz`
  (5.6 MB); the lock is a bind mount, never a `COPY`. The marker lists the
  `lcu-archive` capability. `verifyGuestImage` re-checks the archive hash against
  the lock and that `/opt/lcu`, `/usr/lib/chatgpt` and `/opt/silo` do not exist. A computer
  installs the archive against the mounted app at boot
  ([built-in computer use](SiloUI-DESKTOP.md#built-in-computer-use)); if the lock is
  bumped without a new image the computer downloads and verifies the new archive instead
  (needs network once). The published `ubuntu-24.04-v4` image contains LCU 0.8.1 while
  Silo pins 0.9.2, installed in the computer at setup; images built from the current lock
  stage the lock's version.
- **Accessibility defaults.** `gsettings-desktop-schemas`, the dconf stack,
  `/etc/dconf/profile/user` (`user-db:user`, `system-db:local`) and
  `/etc/dconf/db/local.d/00-silo-accessibility` with
  `toolkit-accessibility=true`, compiled by `dconf update`. This sets
  `org.a11y.Status.IsEnabled` in each session (Firefox exposes web content; GTK and Qt
  already do).
- **Chromium/Electron poller.** `src-tauri/guest/silo-accessibility.py` is installed
  as `/usr/local/libexec/silo-accessibility` and autostarted by
  `/etc/xdg/autostart/silo-accessibility.desktop` in any Xfce session. It calls
  `getAttributes()` and `getRelationSet()` on each application root and its first
  five children, which makes Chromium expose full web trees (Chrome 154: 4 to 242
  nodes; ChatGPT Electron: 2 to 28). It skips handled applications and backs off
  from 2 s to 10 s when nothing changes. It needs `python3-pyatspi`.
  Applications are identified by process id (never by name, which is a call into the
  application); AT-SPI calls time out after 1 s, a sweep is bounded to 6 s, and an
  application that responds slowly is skipped for 60 s. Autostart never restarts
  anything and libatspi aborts the process (exit 133) when the accessibility bus
  cannot be activated, so the same file also acts as its own supervisor: by default it
  waits (backoff up to 30 s, 10 minutes at most) until `org.a11y.Bus.GetAddress`
  succeeds, then runs `--worker` as a child and restarts it after abnormal exits
  (backoff 1 s to 30 s, giving up after 10 consecutive runs shorter than a minute).
- **Text editor.** GNOME Text Editor (GTK4) replaces Mousepad and is the system
  default for `text/plain` and a few common text types in `/etc/xdg/mimeapps.list`
  (`xdg-mime query default text/plain` gives `org.gnome.TextEditor.desktop`). GTK
  3.24's AT-SPI `PasteText` has a use-after-free that crashes every GTK3 text view.
  When launching it for automation, use `gnome-text-editor --standalone` so each
  launch is its own process rather than a request to an existing instance.

The apt lists and caches are removed in the same layer, and
`/usr/local/share/silo-packages.txt` is regenerated after the desktop packages.
`verifyGuestImage` also checks the desktop, poller, dconf and editor defaults, and
that Mousepad is absent. Measurements are in [guest image size](SiloUI-GUEST-IMAGE-SIZE.md).

**Licensing of the desktop streamer.** v4 redistributes the upstream Selkies 2.0.0 package
unchanged. Its pixelflux extension is upstream's default GPL build and carries private copies of
x264, x265, FFmpeg n8.1, kvazaar, libvpx, SVT-AV1 and dav1d, and pcmflux carries AlmaLinux 8
audio libraries, none of which are part of the Ubuntu package set. Silo therefore treats those
parts as redistributed GPL/LGPL software: `app/SiloUI/THIRD-PARTY-NOTICES.md` lists each component
with its version and license, the exact upstream sources and the build recipe are mirrored in the
[`guest-ubuntu-24.04-v4-source` release](https://github.com/amontlabs/silo/releases/tag/guest-ubuntu-24.04-v4-source)
with their SHA-256 values (`SOURCES.md`, `sources.json`, `SHA256SUMS`), and the v4 release notes and
the notices include a written offer for the corresponding source. The x264 commit is established by
upstream's build log and branch history rather than a pinned ref (see `SOURCES.md`). Ubuntu packages
keep their `/usr/share/doc/*/copyright` files and their source is in the Ubuntu archive for the
versions in `/usr/local/share/silo-packages.txt`. The published v4 image does not contain these
notices; the recipe now installs `src-tauri/guest/guest-third-party-notices.md` (the text
`THIRD-PARTY-NOTICES.md` repeats, which a test enforces) as
`/usr/share/doc/silo-guest-third-party/NOTICES.md`, so the next image version carries them. H.264
and H.265 are covered by patents these licenses do not grant; Silo provides no patent license.
Alternatives considered: installing Selkies at first boot (no redistribution, but it needs the
network at first start and does not remove the Ubuntu GPL packages from the image), and rebuilding
pixelflux without its GPL codecs (`PIXELFLUX_ENABLE_GPL=0`; needs Silo-built wheels and loses x265
4:4:4). When bumping `desktop-streamer-lock.json`, update the notice table and the source mirror
in the same change.

## Runtime behavior

The app downloads and verifies the pinned archive (below), then imports it into its private
MicroSandbox cache. It decompresses a bounded temporary Docker archive because the bundled
runtime's `image load` does not accept an outer gzip stream. Creation uses the verified cached
image with pulling disabled. A missing, corrupt or wrong-CPU image fails visibly; there is no
package-install or online-image fallback. GitHub access, Git identity and secrets remain
separate live configuration.

## Download on first use

From the release after 0.11.x the image is prepared on the device by the background preparation described in
[background preparation](SiloUI-PREPARATION.md). The decision and its numbers: the v4 archive is
about 400 MiB compressed, which made installers and updates about 440 MiB instead of 35 MiB and
pushed the APT repository past its [900 MiB budget](SiloUI-LINUX-UPDATES.md).

- **When.** At launch, after the image cache repair, without the operation gate. A creation that
  needs the image waits for the same work before it takes the gate, so "Creating a sandbox now
  finishes everything" still holds; the creation toast shows "Waiting for the VM image" and the
  preparation toast shows the download percentage.
- **Already imported.** The check is `guest_image::is_imported_as`: the runtime cache holds the
  pinned reference with the pinned config digest and materialized layers. A device that imported
  the same image from an earlier Silo (bundled or downloaded) is ready at once, with no download
  and no archive on disk. The image reference is unchanged by the repository move, so the cache
  key matches.
- **Where.** `<app data>/guest-image/<version>/image.tar.gz` (the app data directory is per
  [channel](SiloUI-BUILD-CHANNELS.md), so Silo and Silo Dev never share it), mode 0444 in a 0555
  folder, written by `preparation::download_and_publish` (the same code that publishes the LCU
  archive). `RuntimePaths.guest_image` is that root. The archive stays after the import so a
  device that loses its cache or selects another storage location imports it again without the
  network.
- **How.** `chatgpt_app::HttpDownloader`: unauthenticated HTTPS only, at most five redirects
  (release assets redirect to `objects.githubusercontent.com`), no GitHub token, resume of a
  partial file in `.download/<version>-image.tar.gz.part`, five attempts with backoff, a hard
  stop at the pinned length. Free space for the missing bytes is checked first. The size and the
  SHA-256 must both match the lock before the file is published atomically (staging directory,
  rename); a mismatch deletes the partial file and fails with "did not match its checksum and was
  removed. Retry." Older versions and stale partial files are removed after a publish.
- **Messages.** Retryable: "Silo could not download the VM image. Check your network connection,
  then retry." (with Retry in the toast). A 404 or 410: "The VM image is no longer available at
  its pinned location. Update Silo." Low space: "Free at least N MiB to prepare Silo's VM image,
  then retry."
- **Remote computers.** The other device runs the same app, so its own preparation downloads the
  image for its architecture and a creation requested over the connection waits for it there.
  Remote creation keeps its 35-minute request window.
- **Live tests** that need a real image place the verified archive at
  `src-tauri/runtime/guest-image/<version>/image.tar.gz` (download
  `<releaseUrl>/image-<arch>.tar.gz` and check it against the lock) or set
  `SILO_TEST_GUEST_ARCHIVE` for the ignored import test.

## Sources

- [GitHub Container registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry): OCI/Docker support, job-token publication, source labels, default private visibility and anonymous public pulls.
- [Docker build context](https://docs.docker.com/build/concepts/context/): Dockerfile-specific ignore files restrict uploaded build inputs.
- [Docker save](https://docs.docker.com/reference/cli/docker/image/save/): portable image archive export.

See [the initial size measurement](SiloUI-GUEST-IMAGE-SIZE.md) for the earlier
experiment. Final published archive sizes are authoritative in the image lock.

## Verification on 2026-09-10 (guest image v1)

The image publication run succeeded:
https://github.com/amontlabs/silo/actions/runs/34452627515
Source recipe commit: `e9d90f58acb45689931e371d008fd0c81015571a`.
Anonymous GHCR requests returned HTTP 200 for the multi-architecture image and
both platform manifests. Each platform's config digest matches the corresponding
published archive manifest and the checked-in lock. Compressed archive sizes are
67,674,633 bytes (ARM64) and 69,442,016 bytes (x86-64).

Local verification used Apple Silicon and disposable MicroSandbox homes/computers:

- Published archives passed checksum/size verification, and the packaged app
  resource matched the ARM64 lock.
- Offline Docker tool checks passed on both architectures in the publication job.
- MicroSandbox imported the archive, created with `--pull never`, booted to run
  Git/LFS/gh and integration checks, and returned to Stopped. Registry proxy
  access was blocked during the isolated import/creation check.
- Two concurrent calls to the production import helper performed exactly one
  cold-cache import. A later call used a runner that rejects any attempted
  reimport. Registry access was blocked and temporary archives were removed.
- Existing live GitHub bootstrap/identity and secrets rotation/removal tests
  passed using the new image. These tests require HTTPS test endpoints; they are
  separate from the offline image test. No real account credentials were used.
- The real app created `image-check`, displayed “Preparing the bundled VM image…”
  in the existing progress row, and settled at Stopped. The disposable computer was
  removed and the pre-existing `dev` computer stayed Stopped. No onboarding reset or
  user secret change was performed. Native accessibility observations were used;
  the screenshot provider was unavailable.

Backup verification exposed two existing compatibility gaps relevant to freshly
created Silo computers. Backup now accepts and restores only the exact credential-free
GitHub bootstrap network preset; custom policies, host secret references and
nonempty secret values remain rejected. The pinned MicroSandbox patch compares
cache metadata JSON structurally when serialized map ordering differs, while
all image blobs/filesystem artifacts retain byte-for-byte comparison. Existing
cache contents are never overwritten on equivalence or conflict.

Final checks passed: all 584 frontend tests, 249 native tests plus five build
configuration tests, and four opt-in live tests (concurrent image import,
GitHub/identity, secrets, and backup/restore). Backup restored root and workspace
files, Git identity and the default GitHub profile into both cold and warm caches.
The debug app was rebuilt with `npm --prefix app/SiloUI run desktop:build:debug`;
its bundled image matched the lock and `codesign --verify --deep --strict` passed.
The rebuilt production-mode app was reopened at
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app` and showed only the
preserved `dev` computer, Stopped. It remains open for testing. This is a local debug
build, not a notarized release.

Linux hardware/KVM and a Linux desktop bundle have not been exercised locally.
The two architecture image builds do not substitute for those checks. No optional
Ubuntu downloads or selection UI is implemented in this slice.

## Guest image v2 verification on 2026-09-14

Version 2 adds curl to new computers. Existing computer disks and restored backups retain
their packages; install curl inside those computers with
`apt-get update && apt-get install -y curl` (as root).

[Image publication](https://github.com/amontlabs/silo/actions/runs/34837327776)
built ARM64 and AMD64 and ran curl, Git, Git LFS, gh and credential-helper checks
with container networking disabled before publishing. Curl also read a local
file through its file protocol. Both manifests record curl 8.5.0-2ubuntu10.13.

Both downloaded archives passed compressed SHA-256, compressed size, uncompressed
size and Docker configuration digest checks before updating the application lock.
The six guest-image staging tests, 16 release-tooling tests, type checking and
lint passed locally. These checks establish image contents and packaging inputs;
they do not establish live computer networking or installation/upgrade acceptance.

A disposable ARM64 MicroSandbox computer also booted from the verified downloaded v2
archive with `--pull never`. `curl --version` and
`curl -fsS --max-time 20 https://example.com -o /dev/null` both exited successfully.
The test used a separate `/private/tmp/silo-curl-vm-check` home and the existing
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app/Contents/MacOS/msb` helper
with its bundled `Contents/Frameworks/libkrunfw.5.dylib`. The disposable computer was
stopped afterward. This checks the new image on the existing VM engine, not a
rebuilt or installed 0.4.2 app. No user computer was modified.
