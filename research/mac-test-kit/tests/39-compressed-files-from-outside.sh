#!/bin/bash
# id: 39
# title: The contents of compressed files, read from outside the computer
# tier: vm
# time: about 25 minutes (one first start, a few hundred MB copied inside, the disk read from the Mac)
# needs: the installed computer of test 00 (a raw disk, FileVault off inside, as test 14 checks), no sudo on the Mac
# step: 5
# decides: putting-back-a-path and file-changes-from-outside (#370 finding 6: look and put-back need the contents of a path, APFS compression included, from a reader no trial has run)
# question: Inside a macOS computer, files compressed by APFS (decmpfs: zlib, LZVN, LZFSE, in the xattr or in a resource fork) are common: the system's own apps keep them, and ditto writes them. Read from outside, from the computer's disk image, does a reader built on Apple's open-source decoders give back every file byte for byte, and which compressions does main actually hold?
# outcome reads-all: Every file of the corpus read from outside equals what the guest read, compressed ones included (the sentence counts each
#   compression): the second mode of #370's reader works as specified, with zlib and Apple's lzfse library; look and put-back can read a checkpoint
#   without mounting it. The census of main's own data volume (facts) says how much of it is compressed.
# outcome type-unread: Every file the reader could decode was right, but main holds a compression it has no decoder for (named, LZBITMAP most
#   likely): Silo's reader needs that decoder, or shows those files as a gap in look and put-back.
# outcome reads-wrong: A file read from outside differs from the guest's, a file the guest marks compressed was read as plain, or a plain
#   file gave a gap: the reader's handling of that case is wrong (examples in the facts); nothing reads contents from outside until it is fixed.
# outcome inconclusive: The computer did not start, its disk is not raw, its data volume is encrypted, or no compressed file was made; the sentence says which.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

W="$KIT_WORK"
READER="$KIT_ROOT/py/apfs/apfscontent.py"
kit_fact decoders "$(python3 -I "$READER" decoders)"

if guest_local; then
  # Linux analogue: no computer. The disk is an APFS image Apple's APFS wrote with every
  # compression (dissect.apfs's test disks, as linux/test-apfs.sh fetches them), and the guest's
  # own checksums are the contents those disks are known to hold.
  img="${KIT_APFS_DATA:-/tmp/silo-kit-apfs-data}/case_sensitive.bin"
  [ -s "$img" ] || kit_skip "Linux analogue: no test disk at $img (linux/test-apfs.sh fetches it)"
  fork=5f46d97f947137dcf974fc19914c547acd18fcdb25124c846c1100f8b3fbca5f
  attr="$(printf 'Compressed data in xattr %s\n' "$(printf 'a%.0s' $(seq 1 90))" | shasum -a 256 | cut -c1-64)"
  : > "$W/guest.sha"; : > "$W/flagged.txt"
  for a in zlib lzvn lzfse; do
    printf '%s  %s\n' "$fork" "/dir/compressed-$a-fork" >> "$W/guest.sha"
    printf '%s  %s\n' "$attr" "/dir/compressed-$a-xattr" >> "$W/guest.sha"
    printf '%s\n%s\n' "/dir/compressed-$a-fork" "/dir/compressed-$a-xattr" >> "$W/flagged.txt"
  done
  printf '%s  %s\n' "$(printf '\357\243\277 File System\n' | shasum -a 256 | cut -c1-64)" /dir/file >> "$W/guest.sha"
  python3 -I "$READER" ls "$img" --under /dir > "$W/reader.jsonl" 2> "$W/reader.err"
  res="$(python3 -I "$KIT_ROOT/py/39-compare.py" "$W/guest.sha" "$W/reader.jsonl" "$W/flagged.txt")"
  kit_fact compare "$(echo "$res" | sed -n 2p)"
  line="$(echo "$res" | sed -n 1p)"
  kit_outcome "${line%%|*}" "Linux analogue, dissect.apfs's case_sensitive disk: ${line#*|}"
  exit 0
fi

kit_need_mac 27
kit_need_free_gb 20
fmt="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("disk_format", "raw"))' "$KIT_BASE_VM/config.json" 2>/dev/null)"
[ "$fmt" = raw ] || kit_skip "the base computer's disk is $fmt, not raw: the reader reads raw images only"
kit_confirm "Test 39 starts a macOS computer, copies a few hundred MB inside it with their compression, stops it and reads its disk from the Mac: about 25 minutes."

trap guest_cleanup EXIT
guest_up c39
kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"

# The corpus: text in sizes around one 64 KiB block, several blocks and several MB, compressed by
# ditto; random bytes, which a compressor leaves plain or stores raw in its blocks; and copies of
# apps of the system volume, which ditto keeps compressed as they are.
guest_sh 'set -e
D="$HOME/silokit39"; rm -rf "$D" "$D.src"; mkdir -p "$D.src" "$D/sys"
cd "$D.src"
perl -e '"'"'
my @w = qw(agent night main checkpoint record boundary model reply turn computer Silo file disk block chain);
srand(39);
for my $n (100, 4096, 65535, 65536, 65537, 200000, 1048576, 5000000) {
  open(my $f, ">", "text-$n.txt") or die; my $s = "";
  while (length($s) < $n) { $s .= $w[int(rand(@w))] . (rand() < 0.1 ? "\n" : " "); }
  print $f substr($s, 0, $n); close $f;
  open($f, ">", "random-$n.bin") or die; my $r = ""; $r .= chr(int(rand(256))) for 1..$n; print $f $r; close $f;
}
open(my $e, ">", "empty.txt") or die; close $e;
'"'"'
ditto --hfsCompression "$D.src" "$D/ditto"
cp -R "$D.src" "$D/plain"
for a in /System/Applications/Calculator.app /System/Applications/TextEdit.app /System/Applications/Chess.app /usr/share/man/man1; do
  [ -e "$a" ] && ditto "$a" "$D/sys/$(basename "$a")"
done
rm -rf "$D.src"
sync' > "$W/corpus.log" 2>&1 || { kit_attach "$W/corpus.log"; kit_fail "the corpus could not be made inside the computer"; }

guest_sh 'D="$HOME/silokit39"; find "$D" -type f -print0 | xargs -0 shasum -a 256' > "$W/guest.sha" 2> "$W/guest-sha.err"
guest_sh 'find "$HOME/silokit39" -type f -flags +compressed' > "$W/flagged.txt" 2>/dev/null
kit_fact guest_files "$(wc -l < "$W/guest.sha" | tr -d ' ') files, $(wc -l < "$W/flagged.txt" | tr -d ' ') flagged compressed"
# How common compression is in main's own data volume, as provisioned (the system volume is sealed
# and read-only, so what Silo reads of main is the data volume).
kit_fact data_volume_census "$(guest_sh 'n=0; c=0
for d in /Users /Applications /Library /private/var/db /usr/local; do
  [ -d "$d" ] || continue
  n=$((n + $(find -x "$d" -type f 2>/dev/null | wc -l)))
  c=$((c + $(find -x "$d" -type f -flags +compressed 2>/dev/null | wc -l)))
done; echo "$c of $n files compressed"')"
kit_fact filevault_inside "$(guest_sh 'fdesetup status' 2>&1 | head -1)"

kit_fact stop "$(vm_stop "$GB" 120)"
# The disk is read as Silo would read a checkpoint: from a clone of the image, never the image
# a computer may be writing.
cp -c "$GB/disk.img" "$W/read.img" || kit_fail "clonefile of the disk image failed"
t0="$(kit_now)"
python3 -I "$READER" ls "$W/read.img" --under "/Users/$VM_USER/silokit39" > "$W/reader.jsonl" 2> "$W/reader.err"
t1="$(kit_now)"
kit_measure reader_seconds "$(python3 -c "print('%.1f' % ($t1 - $t0))")" s "every file of the corpus read and hashed from outside"
rm -f "$W/read.img"
# The guest printed paths from its own root; the reader reads the data volume, where /Users is.
sed "s#  /System/Volumes/Data/#  /#" "$W/guest.sha" > "$W/guest.sha.norm" && mv "$W/guest.sha.norm" "$W/guest.sha"
for f in guest.sha flagged.txt reader.jsonl reader.err corpus.log; do [ -s "$W/$f" ] && kit_attach "$W/$f"; done
if grep -q '"gap": "volume' "$W/reader.jsonl" 2>/dev/null || [ ! -s "$W/reader.jsonl" ]; then
  kit_outcome inconclusive "the reader did not open the data volume: $(head -c 300 "$W/reader.jsonl" "$W/reader.err" 2>/dev/null | tr '\n' ' ')"
  exit 0
fi
res="$(python3 -I "$KIT_ROOT/py/39-compare.py" "$W/guest.sha" "$W/reader.jsonl" "$W/flagged.txt")"
kit_fact compare "$(echo "$res" | sed -n 2p)"
line="$(echo "$res" | sed -n 1p)"
kit_outcome "${line%%|*}" "${line#*|}"
