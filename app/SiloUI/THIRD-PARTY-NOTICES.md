# Bundled MicroSandbox runtime

Silo builds the MicroSandbox v0.7.6 source with Silo patches for networking, storage, restore, and desktop lifecycle policy. macOS packaging adds the app's code signature:

- `msb`, licensed under Apache-2.0.
- `libkrunfw` 5.6.1. The library code is LGPL-2.1-only. Its embedded Linux kernel and kernel patches are GPL-2.0-only or compatible licenses.

Upstream provenance:

- Release: https://github.com/superradcompany/microsandbox/releases/tag/v0.7.6
- MicroSandbox source: https://github.com/superradcompany/microsandbox/tree/09df3d4b9d832adaede1fb9a198cfc660bfab8cd
- libkrunfw source: https://github.com/superradcompany/libkrunfw/tree/cf4c22b9f05c680928e6d96a9d198f5845573a87

The license texts are bundled under `microsandbox/licenses/`. Redistribution requirements and the pinned artifact hashes are recorded in `docs/SiloUI-RUNTIME-PACKAGING.md` in Silo's source tree.

# Bundled Git and Git LFS

Silo stages the required Git client runtime from the target archive in dugite-native v2.53.0-4, commit `4098283a7ecb8a227b9d43580336c78a06f90e5d`. Dugite-native is the portable Git distribution maintained for GitHub Desktop. Silo retains Git, Git LFS, HTTPS transport, templates, and Linux certificates. It excludes Scalar, Git Credential Manager, server programs, and unrelated helpers.

The archive contains:

- Git 2.53.0, commit `67ad42147a7acc2af6074753ebd03d904476118f`, licensed under GPL-2.0.
- Git LFS 3.7.1, commit `b84b33847fe6458f36ef521534dc0eac953cb379`, licensed under MIT plus component terms recorded in its license.
- Git HTTPS helpers and templates.
- Curl's converted Mozilla CA certificate bundle on Linux, licensed under MPL-2.0.

Upstream provenance:

- Dugite-native release: https://github.com/desktop/dugite-native/releases/tag/v2.53.0-4
- Dugite-native source: https://github.com/desktop/dugite-native/tree/4098283a7ecb8a227b9d43580336c78a06f90e5d
- Git source: https://github.com/git/git/tree/67ad42147a7acc2af6074753ebd03d904476118f
- Git LFS source: https://github.com/git-lfs/git-lfs/tree/b84b33847fe6458f36ef521534dc0eac953cb379
Exact license texts are bundled under `git-support/licenses/`. Git LFS's license includes the copied Go code terms and directs distributors to the licenses of its Go modules; external distribution still requires that dependency-license review. Linux Debian and RPM packages depend on the distribution's libcurl package. AppImage builds copy the build distribution's eligible libcurl dependency chain and must retain the licenses collected by linuxdeploy. Exact target archives, SHA-256 values, packaged path rules, platform limits, and corresponding-source review requirements are recorded in `docs/SiloUI-RUNTIME-PACKAGING.md` in Silo's source tree.

# Bundled Git LFS SSH transfer server

Silo builds charmbracelet/git-lfs-transfer from commit
`971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b`, licensed under MIT, for Linux
ARM64 or x86-64 guests. This server implements the upstream Git LFS pure SSH
protocol. Silo copies it into a temporary guest directory for each authorized
publish operation; the guest does not receive GitHub write credentials.

- Source: https://github.com/charmbracelet/git-lfs-transfer/tree/971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b
- Source archive SHA-256: `92d6720202aa5a059c6683df78f1fa47722c0c48ff1dc4ebfc0bc8137d988702`
- Upstream dependencies: https://github.com/charmbracelet/git-lfs-transfer/blob/971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b/go.mod

The MIT license, Go runtime license, and license/notice files for every linked
external Go module are bundled under `git-support/lfs-transfer/`. The module
list comes from `go list -deps` for the actual guest build target; upstream
`go.sum` and the Go checksum database verify module source. Corresponding
source and redistribution review remain part of release preparation.

# Bundled LCU release archive

Silo's guest image (v4 and later) stages an unextracted LCU release archive for the sandbox's built-in computer use. LCU is MIT-licensed; the license text is inside the archive. The image holds only the archive, in `/usr/local/share/silo/lcu/`; LCU itself is installed from it in the sandbox when computer use is set up.

- Project: https://github.com/amontlabs/lcu
- Release: https://github.com/amontlabs/lcu/releases/tag/v0.11.0
- Linux ARM64 archive SHA-256: `bfb91127e103065e47088545c4d71dcb00714eec05fcf72ff30913212c485dc8`
- Linux x86-64 archive SHA-256: `12919c3bd94f1d74874e8a4da4e5d7c713138079b613df05e5f81a1e03e6a62c`

The pinned URL and hashes are in `app/SiloUI/src-tauri/guest/lcu-lock.json`. The published v4 image itself contains the LCU 0.8.1 archive (release v0.8.1, SHA-256s `441649e7afe14dc948caaa5bd94034567e4404fc8c0bb6a450bd692b73161806` arm64, `8b0934f8c0c79d40a5073f180db33db8f1568af00177befa4b8da677694731bc` x86-64); images built from the current lock stage 0.11.0. The image contains no ChatGPT application.

# Silo guest image: Selkies desktop streamer and codecs

Silo's published guest images from `ubuntu-24.04-v4` on (GitHub release `guest-ubuntu-24.04-v4` and
`ghcr.io/0xpolarzero/silo-guest:ubuntu-24.04-v4-{arm64,amd64}`) contain the unmodified upstream
Selkies 2.0.0 Ubuntu 24.04 package, installed at `/opt/selkies`. The Silo application itself does not
contain these components; it downloads the image.

Selkies and its two native extensions are licensed under MPL-2.0:

- Selkies 2.0.0: https://github.com/selkies-project/selkies/releases/tag/2.0.0 (commit `3ec56fb1538cf077c27156f5ab75b6595a83c461`)
- pixelflux 2.1.0: https://github.com/selkies-project/pixelflux/tree/2.1.0 (commit `1ebcb0a10a96caeba20e1ad09be47cc018a55238`)
- pcmflux 2.1.0: https://github.com/selkies-project/pcmflux/tree/2.1.0 (commit `61ed92062dc559639dedd44e469d08a32ac1d70d`)

The pixelflux wheel in the image is upstream's default (GPL-enabled) build. It bundles private copies
of the following libraries, which are separate from the Ubuntu packages in the image:

| Library | Version | License |
| --- | --- | --- |
| x264 (libx264 core 165) | `stable` branch at build time, commit `b35605ace3ddf7c1a5d67a2eb553f034aef41d55` (r3222; established from upstream's build log and the branch history, see the source release) | GPL-2.0-or-later |
| x265 | 4.2 (`e444744c03978c1fb4e037168967020cf2648427`) | GPL-2.0-or-later |
| FFmpeg libavcodec, libavfilter, libavformat, libavutil, libswresample, libswscale | n8.1 (`9047fa1b084f76b1b4d065af2d743df1b40dfb56`), configured with `--enable-gpl --enable-libx265 --enable-libkvazaar --enable-libvpx --enable-libsvtav1 --enable-libdav1d` | GPL-2.0-or-later as built |
| kvazaar | v2.3.2 | BSD-3-Clause |
| libvpx | v1.15.2 | BSD-3-Clause |
| SVT-AV1 | v4.2.0 | BSD-3-Clause-Clear with the Alliance for Open Media Patent License 1.0 |
| dav1d | 1.5.1 | BSD-2-Clause |
| libjpeg-turbo (statically linked) | 3.1 | IJG, BSD-3-Clause, Zlib |
| libopus (statically linked in pcmflux) | vendored by `opusic-sys` | BSD-3-Clause |

pcmflux additionally bundles these AlmaLinux 8 libraries: libpulse and libpulsecommon (pulseaudio-libs
14.0-4.el8), libsndfile 1.0.28-17.el8_10, libgcrypt 1.8.5-8.el8_10, libgpg-error 1.31-1.el8, libasyncns
0.8-14.el8, libsystemd (systemd-libs 239-82.el8_10.19), libmount, libblkid and libuuid (util-linux
2.32.1-48.el8_10) under LGPL-2.1-or-later (libuuid BSD-3-Clause); and flac-libs, libogg, libvorbis, libcap,
libselinux, liblz4, libpcre2, liblzma, libdbus, libgsm, libX11-xcb, libXau, libXi, libXtst and libxcb under
BSD-style, MIT, public-domain or dual licenses (exact versions: `pcmflux-2.1.0.dist-info/sboms/auditwheel.cdx.json`
inside the image). The LGPL libraries are ordinary shared objects and can be replaced.
Selkies also depends on Python packages (aiohttp, cryptography, Pillow, uvloop and others), each under a
permissive license, whose license files are in their `dist-info` directories under
`/opt/selkies/lib/python3.12/site-packages/`. Pixelflux's own inventory of everything it contains is in
`pixelflux-2.1.0.dist-info/licenses/LICENSES.md` in the image, and Selkies' is at
https://docs.selkies.io/latest/licensing.

Because the pixelflux extension is linked with libx264, x265 and a GPL build of FFmpeg, upstream
distributes it under GPL-2.0-or-later terms as a whole. The H.264 and H.265 codecs are covered by patents
that these licenses do not grant; Silo does not provide a patent license.

Corresponding source. Source for every component above, at the exact versions, is mirrored at
https://github.com/amontlabs/silo/releases/tag/guest-ubuntu-24.04-v4-source with `SHA256SUMS`,
`sources.json` and `SOURCES.md` (Selkies, pixelflux, pcmflux, x264, x265, FFmpeg, kvazaar, libvpx,
SVT-AV1, dav1d and the AlmaLinux 8 source packages), including the build recipe (`pyproject.toml`,
`setup.py`) and the FFmpeg configure line.
Written offer: for at least three years from 2026-10-02, and for as long as the image is offered for
download, Silo will give anyone who received it the complete corresponding source code of these
components on request, at no charge beyond the cost of providing it, via
https://github.com/amontlabs/silo/issues.

# Silo guest image: Ubuntu packages

The image also contains Ubuntu 24.04 packages (the exact list with versions is `/usr/local/share/silo-packages.txt`
in the image), each under its own license; copyright files are in `/usr/share/doc/<package>/copyright` and the
common license texts in `/usr/share/common-licenses/`. Source for each package is available through the Ubuntu
archive and Launchpad (`apt source <package>=<version>`, https://launchpad.net/ubuntu/+source/). The image includes
Ubuntu's `ffmpeg`, `libx264-164` and `libx265-199` in addition to the Selkies copies above. The same three-year
written offer applies to these packages' source.
