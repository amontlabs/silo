#!/bin/sh
# Builds the guest: Ubuntu's generic kernel (downloaded, not installed) and an
# initramfs of busybox and clockagent. Needs qemu-system-x86, busybox-static, gcc.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
out=${OUT:-$here/.build}
mkdir -p "$out/root/bin" "$out/root/proc" "$out/root/sys" "$out/root/dev"
if [ ! -f "$out/vmlinuz" ]; then
  pkg=$(apt-cache depends linux-image-virtual | awk '/Depends/{print $2}' | grep '^linux-image-' | head -1)
  (cd "$out" && apt-get download "$pkg" >/dev/null 2>&1 && dpkg-deb -x "$pkg"_*.deb deb && cp deb/boot/vmlinuz-* vmlinuz && rm -rf deb "$pkg"_*.deb)
fi
gcc -static -O2 -o "$out/root/bin/clockagent" "$here/guest/clockagent.c"
cp /bin/busybox "$out/root/bin/busybox"
cp "$here/guest/init" "$out/root/init"; chmod +x "$out/root/init"
(cd "$out/root" && find . | cpio -o -H newc 2>/dev/null | gzip) > "$out/initrd.gz"
echo "$out"
