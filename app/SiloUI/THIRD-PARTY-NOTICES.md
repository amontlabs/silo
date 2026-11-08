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

# silo-runner

The Linux engine archives include `silo-runner`, Silo's own Go program (`app/SiloUI/runner`), built
with Go 1.27.3 and statically linked. It links these Go modules; their licenses are in
`resources/silo-runner-licenses/`, with the Go license:

- `github.com/actions/scaleset` v0.5.0 (GitHub's runner scale set client), MIT.
- `github.com/golang-jwt/jwt/v5` v5.3.1, MIT.
- `github.com/google/uuid` v1.6.0, BSD-3-Clause.
- `gopkg.in/yaml.v3` v3.0.1, MIT and Apache-2.0 (its `NOTICE` is included).
- The Go standard library and runtime, BSD-3-Clause.

`silo-runner` does not bundle GitHub's runner program: the `silo-linux` recipe downloads it into each
computer, checked against a pinned SHA-256.

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
archive and Launchpad (`apt source <package>=<version>`, https://launchpad.net/ubuntu/+source/). The same three-year
written offer applies to these packages' source. Images before `ubuntu-24.04-v5` also contained Ubuntu's
`ffmpeg`, `libx264-164` and `libx265-199`; v5 and later do not.

# Silo guest image: Cua Driver

Silo's guest images from `ubuntu-24.04-v5` on contain Cua Driver, installed at
`/usr/local/lib/silo/cua-driver/<version>/cua-driver` and linked as `/usr/local/bin/cua-driver`. Computers
created from an earlier image download the same pinned release once when computer use is set up
(`app/SiloUI/src-tauri/guest/cua-driver-lock.json`). Only the `cua-driver` binary of the release archive is
used. Cua Driver is MIT-licensed; its licence text follows. The optional `cua-perception` and `cua-som`
extensions (AGPL-3.0) are not part of the image.

- Project: https://github.com/trycua/cua
- Release: https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.38.0
- Linux ARM64 archive SHA-256: `80c9ae66bc7322052dd426c6133c96086b33894bf06c972bca8215384400b33d`
- Linux x86-64 archive SHA-256: `31ab9709dd91ce73677e49b9f3e75c6f0d3e4d9625c286c9e58262bfbea698dd`

```
MIT License

Copyright (c) 2025 Cua AI, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

# Cua Driver on macOS computers

Silo's macOS computers install the same Cua Driver release's universal macOS archive (`CuaDriver.app`,
signed by Cua AI, Inc. with team identifier YCK386LBJ7) from the device's download cache into the
computer (`app/SiloUI/src-tauri/guest/macos/cua-driver-lock.json`). It is covered by the MIT licence
above. No ChatGPT application and no LCU are installed on any guest.

- Release: https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.38.0
- macOS universal archive SHA-256: `121cf20cafd2cfa69dd720e8967350c6406b22e558be6889de7a1ae3343b6d4b`

# noVNC

Silo's window for a macOS computer on another device bundles noVNC 1.7.0
(`@novnc/novnc`, https://github.com/novnc/noVNC/tree/v1.7.0), the browser RFB
(VNC) client, unmodified inside the application's web assets. noVNC is licensed under
MPL-2.0; its bundled `pako` is MIT and zlib licensed. The license texts are in the package's
`LICENSE.txt`, and the MPL-2.0 source for noVNC is the upstream repository at the tag
above. Silo does not change noVNC files, so the Source Code Form it distributes is the
upstream release.

# tungstenite

Silo's native code uses `tungstenite` 0.30 (https://github.com/snapview/tungstenite-rs),
licensed under MIT or Apache-2.0, for the loopback WebSocket endpoint of that window.

# russh

Silo's engine uses `russh` 0.65.1 (https://github.com/Eugeny/russh), licensed under
Apache-2.0, with the RustCrypto crates it builds on (MIT or Apache-2.0), for the SSH server of
container computers on Linux hosts. It is the library MicroSandbox's own SSH server uses.
