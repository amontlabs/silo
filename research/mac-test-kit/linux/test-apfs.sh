#!/bin/bash
# The APFS probe of tests 17 to 19 (py/apfs/apfsread.py) on disks Apple's APFS wrote: the test
# images of dissect.apfs (Fox-IT, AGPL-3.0, stored with Git LFS; not copied into the kit). They
# are fetched once into KIT_APFS_DATA (default: a folder under the system's temporary folder).
# Without a network the check is skipped. Also, when mkapfs and sgdisk are installed, a GPT disk
# with two containers, as a macOS computer's disk has, must list both.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
. "$KIT_ROOT/linux/disk-common.sh"
disk_apfs_data || { echo "skip: no network for the test disks"; exit 0; }
D="$KIT_APFS_DATA"
python3 "$KIT_ROOT/linux/apfs-validate.py" "$D" || exit 1

if command -v mkapfs >/dev/null 2>&1 && command -v sgdisk >/dev/null 2>&1 && [ "$(id -u)" = 0 ]; then
  t="$(mktemp -d)"; trap 'rm -rf "$t" "$T"' EXIT
  truncate -s 600M "$t/gpt.img"
  sgdisk -n 1:2048:+256M -t 1:7C3457EF-0000-11AA-AA11-00306543ECAC -c 1:Main \
         -n 2:0:0 -t 2:52637672-7900-11AA-AA11-00306543ECAC "$t/gpt.img" >/dev/null 2>&1
  for i in 1 2; do
    st="$(sgdisk -i "$i" "$t/gpt.img" | awk '/First sector/{print $3}')"
    en="$(sgdisk -i "$i" "$t/gpt.img" | awk '/Last sector/{print $3}')"
    l="$(losetup -f --show -o $((st * 512)) --sizelimit $(((en - st + 1) * 512)) "$t/gpt.img")" || { echo "skip: no loop device"; exit 0; }
    mkapfs -L "Vol$i" "$l" >/dev/null; losetup -d "$l"
  done
  out="$(python3 "$KIT_ROOT/py/apfs/apfsread.py" info "$t/gpt.img")"
  if echo "$out" | grep -q '"name": "Vol1"' && echo "$out" | grep -q '"name": "Vol2"'; then
    echo "ok   GPT with two containers: both volumes listed"
  else
    echo "FAIL GPT with two containers: $out"; exit 1
  fi
fi
