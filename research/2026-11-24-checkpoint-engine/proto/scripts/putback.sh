#!/bin/bash
# putback.sh: an ext4 disk image stands for a Linux computer's disk. A run deletes a folder of
# 20,000 files, edits 10 files and adds 5; then another agent edits 2 of those 10. Checkpoints
# around the run are clones of the image; each is read through a clone of its own, since mounting
# replays the journal and a checkpoint never changes. Then `ckpt putback`.
set -e
B=${CKPT:-$(dirname "$0")/../zig-out/bin/ckpt}
W=${W:-/mnt/c/runs/putback-$$}
mkdir -p $W/live $W/a $W/b
img=$W/disk.img
truncate -s 4G $img
mkfs.ext4 -q -F $img
mount -o loop $img $W/live
L=$W/live
mkdir -p $L/proj/src $L/proj/deps
for i in $(seq 1 200); do mkdir -p $L/proj/deps/pkg$i; for j in $(seq 1 100); do echo "module $i $j" > $L/proj/deps/pkg$i/f$j.js; done; done
for i in $(seq 1 50); do seq 1 $((i*100)) > $L/proj/src/file$i.c; done
ln -s src/file1.c $L/proj/main.c
sync; cp --reflink=always $img $W/A.img                       # the checkpoint before the run
# The run
rm -rf $L/proj/deps
for i in $(seq 1 10); do echo "edited by the run" >> $L/proj/src/file$i.c; done
for i in $(seq 1 5); do echo new > $L/proj/src/new$i.c; done
mkdir -p $L/proj/build/out; echo obj > $L/proj/build/out/a.o
sync; cp --reflink=always $img $W/B.img                       # the checkpoint after the run
# Another agent, later
echo "edited later" >> $L/proj/src/file3.c
echo "edited later" >> $L/proj/src/file7.c
echo other > $L/proj/src/other.c
sync
# Reading a checkpoint: a scratch clone of it, mounted
for x in A B; do cp --reflink=always $W/$x.img $W/$x.read.img; done
t0=$(date +%s.%N)
mount -o loop $W/A.read.img $W/a; mount -o loop $W/B.read.img $W/b
t1=$(date +%s.%N)
echo "{\"mount_two_checkpoints_s\":$(echo "$t1 - $t0" | bc)}"
$B putback before=$W/a after=$W/b live=$L apply=1
# Check: every path the run changed and nobody touched since is as before the run.
diff -r $W/a/proj/deps $L/proj/deps >/dev/null && echo '{"deps_back":true}'
for i in 1 2 4 5 6 8 9 10; do cmp -s $W/a/proj/src/file$i.c $L/proj/src/file$i.c || echo "file$i not back"; done
grep -q "edited later" $L/proj/src/file3.c && grep -q "edited later" $L/proj/src/file7.c && echo '{"later_edits_kept":true}'
[ -e $L/proj/src/new1.c ] && echo "new1 still there" || echo '{"run_new_files_removed":true}'
[ -e $L/proj/build ] && echo "build still there" || echo '{"run_new_dirs_removed":true}'
[ -e $L/proj/src/other.c ] && echo '{"later_new_file_kept":true}'
umount $W/a $W/b $W/live
