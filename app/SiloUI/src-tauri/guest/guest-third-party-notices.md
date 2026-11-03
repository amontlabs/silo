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
