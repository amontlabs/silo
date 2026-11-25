#!/bin/sh
# Checks ext4read.py against the kernel, on copies of a mounted ext4 image taken while it is
# mounted, as a checkpoint of a running computer is. Needs root, a loop device, e2fsprogs and
# Python 3. Needs no KVM. Run: sudo sh check.sh [workdir]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
w=${1:-$(mktemp -d)}
r="python3 -I $here/ext4read.py"
cd "$w"
rm -f d.img a.img b.img
truncate -s 2G d.img
mkfs.ext4 -q -F -O metadata_csum,64bit d.img
mkdir -p m
mount -o loop d.img m
trap 'umount m 2>/dev/null || true' EXIT

# Before: a repository-like tree, a big directory (htree), links, a fragmented file.
mkdir -p m/home/p/work/app/src m/home/p/work/fixtures m/home/p/big
for i in $(seq 1 300); do echo "fixture $i" > m/home/p/work/fixtures/f$i.json; done
for i in $(seq 1 3000); do : > m/home/p/big/e$i; done
for i in $(seq 1 40); do head -c 4096 /dev/urandom > m/home/p/work/app/src/m$i.ts; done
ln -s ../app m/home/p/work/link-to-app
ln m/home/p/work/app/src/m1.ts m/home/p/work/hard-m1
i=0; while [ $i -lt 200 ]; do dd if=/dev/urandom of=m/home/p/frag bs=4k count=1 seek=$((i*2)) conv=notrunc status=none; i=$((i+1)); done
sync
sleep 6
cp --sparse=always d.img a.img

# Then: what a night does, with no sync, copied 7 s later (the journal has committed, the
# metadata may not be in its place yet).
rm -rf m/home/p/work/fixtures
mv m/home/p/work/app/src/m2.ts m/home/p/work/app/src/renamed.ts
echo edit >> m/home/p/work/app/src/m3.ts
chmod 600 m/home/p/work/app/src/m4.ts
rm m/home/p/work/app/src/m5.ts && head -c 100 /dev/urandom > m/home/p/work/app/src/m5.ts
mkdir m/home/p/work/new && for i in $(seq 1 150); do echo n > m/home/p/work/new/n$i; done
for i in $(seq 1 1500); do rm m/home/p/big/e$i; done
sleep 7
cp --sparse=always d.img b.img

# The reference: each copy as the kernel reads it, after e2fsck has replayed its journal.
ref() {
  cp --sparse=always "$1" ref.img
  e2fsck -fy ref.img >/dev/null 2>&1 || true
  mkdir -p rm; mount -o loop,ro ref.img rm
  (cd rm && find . -mindepth 1 -path ./lost+found -prune -o -printf '%i\t%y\t%s\t%m\t/%P\n' | grep -v '/lost+found' | sed 's/\tf\t/\tfile\t/;s/\td\t/\tdir\t/;s/\tl\t/\tlink\t/' | sort -t "$(printf '\t')" -k5)
  umount rm; rm -f ref.img
}
mine() { $r walk "$1" $2 2>/dev/null | grep -v '/lost+found' | awk -F'\t' '{printf "%s\t%s\t%s\t%s\t%s\n",$1,$3,$4,$5,$6}' | sort -t "$(printf '\t')" -k5; }

for img in a b; do
  ref $img.img > $img.ref
  mine $img.img "" > $img.mine
  mine $img.img --no-replay > $img.norep
  echo "$img: kernel $(wc -l < $img.ref) paths; reader with replay: $(diff $img.ref $img.mine | grep -c '^[<>]' || true) differing lines; without replay: $(diff $img.ref $img.norep | grep -c '^[<>]' || true)"
done
echo "journal blocks replayed in b: $($r journal b.img | cut -d' ' -f1)"
echo "-- diff a b, with replay:"
$r diff a.img b.img | awk -F'\t' '$1=="F"{c[$2]++} $1=="end"{print} END{for(k in c) print k, c[k]}'
echo "-- diff a b, without replay:"
$r diff a.img b.img --no-replay | awk -F'\t' '$1=="F"{c[$2]++} $1=="end"{print} END{for(k in c) print k, c[k]}'
