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
