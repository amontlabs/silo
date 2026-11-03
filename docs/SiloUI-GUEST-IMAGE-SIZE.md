# Guest image size measurement

> Dated measurements. The ChatGPT app and LCU sections below describe 2026-10 experiments; the
> v5 recipe (2026-11-03) has neither, and no v5 size has been measured yet (v5 is unpublished).

Measured 2026-09-09 on Apple Silicon using Docker/OrbStack, linux/arm64.
This is a packaging experiment, not an implemented MicroSandbox image import.

Built Ubuntu 24.04 with the unchanged `app/SiloUI/src-tauri/guest/setup-github.sh`,
then removed apt download/index caches and the temporary setup script in the same
layer. Base index: `sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254`.
No credentials or user VM files were included.

| Measurement | Bytes |
| --- | ---: |
| Docker image layer size | 233951371 |
| Docker save archive | 238564864 |
| gzip-compressed Docker save archive | 66494292 |
| gzip-compressed current macOS debug Silo.app | 48890698 |
| Current app allocated disk space (du -sk × 1024) | 124321792 |

Adding this compressed guest archive means approximately 66.5 MB more download
and installed app storage. Sum of the separately compressed app and image is
115.4 MB; this is not a measured final DMG. Current app plus compressed archive
is approximately 190.8 MB installed, before runtime extraction/cache and VM data.
All displayed MB are decimal. The 234 MB image layer size is not a prediction of
MicroSandbox's exact cache or per-VM allocated disk consumption.

Offline `docker run --rm --network none` verified Git 2.43.0, Git LFS 3.4.1,
and gh 2.45.0. This checks included tools, not actual VM boot or GitHub integration.
The disposable Docker image was removed; builder cache was not globally pruned.
No x86-64 image or release-signed app was measured. No other development toolchains
are included. These totals include Ubuntu itself, not just added tools.

Commands: docker build --platform linux/arm64; docker image inspect; docker image
save; gzip; tar -czf for the current app; du -sk for installed app allocation.

Background sources: [MicroSandbox OCI images](https://microsandbox.dev/platform/local)
and [Docker image build practices](https://docs.docker.com/build/building/best-practices/).

## Current v3 archives and Bluefin comparison, 2026-09-22

The September 9 experiment above predates the current published image. Reading
`app/SiloUI/guest-image/image-lock.json` gives these pinned v3 archive sizes
(decimal MB):

| Architecture | Compressed archive | Uncompressed Docker-save archive |
| --- | ---: | ---: |
| ARM64 | 85.77 MB | 295.77 MB |
| AMD64 | 87.77 MB | 269.18 MB |

These are archive lengths, not measured allocated disk use inside a VM. The
base includes Ubuntu and the CLI/account tools described in
[guest images](SiloUI-GUEST-IMAGES.md). Xfce, the Selkies streamer and desktop
tools are installed separately by `src-tauri/guest/setup-desktop.sh`. Local candidate
archives have different hashes; the table uses the published lock, not those
candidates.

The [desktop research](SiloUI-LINUX-DESKTOP-RESEARCH.md#costs-and-limits) budgets
an additional 0.5–1.5 GB download and 2–5 GB installed for the desktop plus light
browser use. Both ranges were unmeasured estimates; the
[October 1 measurement](#desktop-recipe-measurement-2026-10-01) below supersedes them.

[Bluefin's installation documentation](https://docs.projectbluefin.io/installation/#disk-usage),
checked September 22, reports approximately 12.4 GB installed for Bluefin and
13.2 GB for Bluefin LTS, including their default Flatpak applications. Developer
mode raises these to 17.4 GB and 14.5 GB respectively. These are upstream disk
usage figures, not compressed download sizes or measurements made in Silo.

## Desktop recipe measurement, 2026-10-01

Measured on Apple Silicon with Docker/OrbStack, linux/arm64. The base was built
from the unchanged `app/SiloUI/guest-image/Dockerfile`; its gzip archive was
86,067,079 bytes, within 0.4% of the published v3 lock. A candidate image added
the exact package list from `src-tauri/guest/setup-desktop.sh` and the pinned
Selkies 2.0.0 ARM64 package, then removed apt lists and downloaded packages.
Luda and LCU agent tools, the official desktop app LCU requires, and user applications
were not included. No x86-64 image was measured.

| Measurement | Bytes |
| --- | ---: |
| Desktop apt downloads (271 packages, onto plain Ubuntu plus base tools) | ~135,000,000 |
| Selkies 2.0.0 ARM64 package download | ~59,600,000 |
| Base image, `docker save` / gzip -9 | 301,947,392 / 86,067,079 |
| Base plus desktop, `docker save` / gzip -9 | 991,276,032 / 307,815,538 |
| Added by the desktop, uncompressed / gzip | 689,328,640 / 221,748,459 |
| Selkies `Installed-Size` alone | ~173,000,000 |

Running memory remains the [September 18 observation](SiloUI-DESKTOP.md#verification-2026-09-18)
(about 123–233 MiB, depending on the quantity).

### Shared image storage in MicroSandbox 0.7.4

The candidate image was loaded with `msb load` into a disposable `MSB_HOME`, and
two sandboxes (8 GiB root disk) were created and booted with the signed `msb`
from the local `Silo Dev.app`. No Silo app, settings or user VM was involved.

- The image occupied 1,019,104 KiB once, in the content-addressed `cache/`.
- Each sandbox added 4,280 KiB of host allocation, before and after boot; its
  8 GiB `upper.ext4` is sparse.
- The guest root was an overlay with the cached image as `lowerdir` and the
  sandbox's `upper.ext4` as `upperdir`. Both guests saw `/usr/bin/selkies` and
  `xfce4-session`, with 56 KiB of the root disk used.

An image-baked desktop therefore costs its size once per host and image version,
and does not consume the sandbox's root-disk allowance. The current per-VM recipe
instead writes about 690 MB into each sandbox's own upper disk, against its
quota, and repeats the downloads for every VM. During an image upgrade, hosts
keep both image versions cached while older VMs use the previous one.

### ChatGPT app and LCU in the image, 2026-10-01

Same method, layered on the desktop candidate above. The official
`chatgpt_arm64.deb` from OpenAI's `latest` URL was version 26.928.31416,
453,121,290 bytes, SHA-256
`b3c3f37dedfc57db72e810a1dc5be7b22586f0b99b7d4875868b06bdd855ba86`, with
`Installed-Size` 1,526,812 KiB. It was installed from a bind mount so the package
file did not remain in a layer. LCU 0.4.0 was then installed by calling the
unchanged `setup-lcu.py` `extract_release` path, which reported runtime
`0.0.27/20260927214556-b77d38801cca`. Agent registration, `doctor` and a VM boot
were not run.

| Image | Uncompressed (`docker image inspect`) | gzip -9 `docker save` |
| --- | ---: | ---: |
| Base plus desktop | 966,462,539 | 307,815,523 |
| Plus ChatGPT app | 2,696,523,215 | 965,586,482 |
| Plus LCU 0.4.0 | 4,432,691,266 | 1,661,058,238 |

The app brought 27 new packages: the app itself, `mesa-vulkan-drivers` (about
113 MB) and smaller systemd/NSS libraries. LCU's installer copies the whole app
into a private generation under `/opt/lcu/apps` (1,535,176 KiB), so the image
holds the app twice; its own release adds about 37 MB. The same final image was
1,302,708,209 bytes with zstd -19 instead of gzip.

The earlier [redistribution review](research/codex-linux-engine-probe-2026-09-22.md)
found no grant to redistribute the app's engine. Publishing an image that
contains it is a separate decision from these measurements.

## v4 recipe measurement, 2026-10-01

Built with `node app/SiloUI/scripts/build-guest-image.mjs arm64|amd64` (Docker/OrbStack;
arm64 native, amd64 emulated), from the unpublished v4 recipe described in
[guest images](SiloUI-GUEST-IMAGES.md#guest-image-v4-recipe-published). Decimal MB.

| Architecture | gzip -9 archive | Uncompressed Docker-save archive | Packages |
| --- | ---: | ---: | ---: |
| ARM64 | 409.26 | 1,294.17 | 519 |
| AMD64 | 417.90 | 1,365.23 | 522 |

For comparison the published v3 archives are 85.77 / 295.77 MB (ARM64) and
87.77 / 269.18 MB (AMD64). The v4 layer is 972 MB uncompressed on ARM64. It
includes ffmpeg and bubblewrap (needed by LCU) and the Selkies package (169 MB
installed on ARM64, 276 MB on AMD64). These totals are about 20 MB above the
earlier planning estimate of 389 / 400 MB; the cause was not investigated.
No `.deb`, apt list or temporary file remains in the image.

Checks on both architectures: Selkies 2.0.0 and Xfce 4.18.3 installed; Mousepad
absent; as a normal user, `dbus-launch gsettings get org.gnome.desktop.interface
toolkit-accessibility` is `true`; `xdg-mime query default text/plain` is
`org.gnome.TextEditor.desktop`. In an Xfce session on Xvfb the autostarted poller
was running, `org.a11y.Status.IsEnabled` was true and the AT-SPI desktop listed
the Xfce components and GNOME Text Editor. Poller CPU over a 60 s window with those
applications running: 0.04 CPU-seconds (about 0.07% of a core, 18 MB RSS) on native
ARM64; 0.11 CPU-seconds (about 0.18%, 26 MB RSS) on emulated AMD64. The poller was not
exercised against Chrome or Electron in this build. The ChatGPT app library check
was skipped: its extracted copy (`/private/tmp/cg`) no longer exists and nothing
was downloaded. These are container checks, not VM boots. Local images were removed
afterwards.
