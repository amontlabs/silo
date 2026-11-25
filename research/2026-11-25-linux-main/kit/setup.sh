#!/bin/sh
# Prepares the kit once: krun-boot built against the installed libkrun, Silo's kernel built, and
# Ubuntu 26.04's cloud image fetched, checked against Ubuntu's SHA256SUMS and converted to raw.
# Run as the user who runs the tests (in the kvm group). sh setup.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
KIT=${KIT:-$HOME/silo-kit}; mkdir -p "$KIT/bin" "$KIT/image" "$KIT/kernel"
a=$(case $(uname -m) in aarch64) echo arm64 ;; x86_64) echo amd64 ;; esac)
cc -O2 -Wall -o "$KIT/bin/krun-boot" "$here/krun-boot.c" $(pkg-config --cflags --libs libkrun 2>/dev/null || echo -lkrun)
img=$(sh "$here/kernel/build.sh" "$KIT/kernel/src" | tail -1); cp "$img" "$KIT/kernel/image"
cd "$KIT/image"
f=ubuntu-26.04-server-cloudimg-$a.img
curl -fsSLO "${KIT_IMAGE_URL:-https://cloud-images.ubuntu.com/releases/26.04/release}/SHA256SUMS"
[ -f "$f" ] || curl -fSLO "${KIT_IMAGE_URL:-https://cloud-images.ubuntu.com/releases/26.04/release}/$f"
grep " \*$f\$" SHA256SUMS | sed 's/ \*/  /' | sha256sum -c -
python3 -I "$here/../proto/qcow2raw.py" "$f" disk.raw
echo "kit ready in $KIT"
