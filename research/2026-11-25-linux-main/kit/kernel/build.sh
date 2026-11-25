#!/bin/sh
# Builds Silo's kernel for the kit: the long-term release named below, the architecture's
# defconfig, then silo.config merged over it. Needs a compiler, flex, bison, bc, libelf, pahole.
# Prints the path of the image. Run: sh kernel/build.sh [workdir]
set -eu
VERSION=${KIT_KERNEL_VERSION:-6.12.60}
here=$(cd "$(dirname "$0")" && pwd)
w=${1:-$HOME/silo-kit/kernel}; mkdir -p "$w"; cd "$w"
[ -d "linux-$VERSION" ] || { curl -fsSLO "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-$VERSION.tar.xz"; tar xf "linux-$VERSION.tar.xz"; }
cd "linux-$VERSION"
case $(uname -m) in aarch64) arch=arm64 img=arch/arm64/boot/Image ;; x86_64) arch=x86 img=vmlinux ;; *) echo "unsupported $(uname -m)"; exit 1 ;; esac
make -s ARCH=$arch defconfig
scripts/kconfig/merge_config.sh -m .config "$here/silo.config" >/dev/null
make -s ARCH=$arch olddefconfig
# Every line of the fragment must have survived: a dependency kconfig dropped is a failure.
missing=$(grep -E '^(CONFIG_|# CONFIG_)' "$here/silo.config" | while read -r line; do grep -qxF "$line" .config || echo "$line"; done)
[ -z "$missing" ] || { echo "dropped by kconfig:"; echo "$missing"; exit 1; }
KBUILD_BUILD_TIMESTAMP='2026-11-25' KBUILD_BUILD_USER=silo KBUILD_BUILD_HOST=kit make -s -j"$(nproc)" ARCH=$arch
echo "$PWD/$img"
