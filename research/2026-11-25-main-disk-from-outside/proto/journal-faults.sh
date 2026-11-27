#!/bin/sh
# #381's test "each journal fault planted: the replay stops where the kernel stops, or a gap names
# the rule; never a crash", run against this folder's ext4read.py (#381's trial with the journal's
# checksums verified) and, with --unchecked, against #381's trial as it is (pass its path).
#
# Makes a copy of a mounted ext4 image whose journal holds committed transactions not yet in
# place (as a checkpoint of a running computer does), plants one fault per copy with jplant.py,
# then asks the kernel (a read-write loop mount of a scratch copy: it replays the journal) and
# e2fsck what they do with it, and the reader what it gives. Needs root, a loop device,
# e2fsprogs, Python 3. No KVM.
#
#   sudo sh journal-faults.sh [READER] [WORKDIR]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
reader=${1:-$here/ext4read.py}
w=${2:-$(mktemp -d)}
cd "$w"
rm -f d.img live.img
truncate -s 512M d.img
mkfs.ext4 -q -F -O metadata_csum,64bit d.img
mkdir -p m k
mount -o loop d.img m
trap 'umount m 2>/dev/null || true; umount k 2>/dev/null || true' EXIT
mkdir -p m/a
for i in $(seq 1 400); do echo "file $i" > m/a/f$i; done
sync; sleep 6
# Two bursts in separate transactions, not yet written in place.
mkdir m/b; for i in $(seq 1 200); do echo n > m/b/n$i; done
sleep 6
rm -rf m/a; mkdir m/c; for i in $(seq 1 100); do echo c > m/c/c$i; done
sleep 6
cp --sparse=always d.img live.img
umount m; trap - EXIT

kernel() {  # what the kernel makes of the image: the paths it lists after its own replay
  cp --sparse=always "$1" k.img
  if mount -o loop k.img k 2>/dev/null; then
    (cd k && find . -mindepth 1 -path ./lost+found -prune -o -printf '%i\t%y\t%s\t%m\t/%P\n' | grep -v '/lost+found' | sed 's/\tf\t/\tfile\t/;s/\td\t/\tdir\t/;s/\tl\t/\tlink\t/' | sort -t "$(printf '\t')" -k5)
    umount k
  else
    echo "REFUSED"
  fi
  rm -f k.img
}
e2() {  # e2fsck's answer on a copy: its exit code and what it says of the journal
  cp --sparse=always "$1" e.img
  c=0; e2fsck -fy e.img >e.log 2>&1 || c=$?
  r=$(grep -i -E 'journal|checksum|corrupt|transaction' e.log | grep -v 'recovering journal' | head -1 | tr '\t' ' ' | cut -c1-60)
  echo "exit $c: $r"; rm -f e.img e.log
}
mine() {
  out=$(python3 -I "$reader" walk "$1" 2>/dev/null) || true
  case "$out" in gap*) echo "$out" | tr '\t' ' ';; *) echo "$out" | grep -v '/lost+found' | awk -F'\t' '{printf "%s\t%s\t%s\t%s\t%s\n",$1,$3,$4,$5,$6}' | sort -t "$(printf '\t')" -k5;; esac
}

printf '%-11s | %-28s | %-62s | %s\n' fault kernel e2fsck reader
for kind in none data descriptor commit superblock sequence; do
  cp --sparse=always live.img f.img
  [ "$kind" = none ] || python3 -I "$here/jplant.py" f.img "$kind" >/dev/null
  kernel f.img > k.out; mine f.img > r.out
  if grep -q '^REFUSED' k.out; then kv="refuses to mount"; else kv="mounts, $(wc -l < k.out) paths"; fi
  if grep -q '^gap' r.out; then rv=$(cat r.out); elif cmp -s k.out r.out; then rv="same $(wc -l < r.out) paths as the kernel"; else rv="DIFFERS from the kernel: $(diff k.out r.out | grep -c '^[<>]') lines"; fi
  printf '%-11s | %-28s | %-62s | %s\n' "$kind" "$kv" "$(e2 f.img)" "$rv"
done
rm -f f.img k.out r.out
