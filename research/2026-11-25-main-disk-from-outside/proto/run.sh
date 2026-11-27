#!/bin/sh
# Every measure of this folder, from nothing, on Linux. Needs gcc, git, curl, Python 3, pip;
# the ext4 part needs root, a loop device and e2fsprogs (skipped without). No KVM, no Mac.
#
#   sh run.sh [WORKDIR] [SECONDS_PER_FUZZ_RUN]
#
# Writes its results to WORKDIR/results.txt. The probe of #390 is taken from that PR's branch
# (claude/project-thread-pj4x2d, mac-test-kit/py/apfs/apfscontent.py); dissect.apfs's test disks
# and Apple's lzfse (github.com/lzfse/lzfse) are fetched.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
w=${1:-$(mktemp -d)}
secs=${2:-1200}
mkdir -p "$w" && cd "$w"
out="$w/results.txt"; : > "$out"
say() { echo "$*" | tee -a "$out"; }

# Apple's lzfse: a plain build, and one with sanitizers and coverage for the fuzzer.
[ -d lzfse ] || git clone -q https://github.com/lzfse/lzfse.git
L=lzfse/src
mkdir -p lib obj
gcc -O2 -fPIC -shared $L/lzfse_decode.c $L/lzfse_decode_base.c $L/lzfse_fse.c $L/lzvn_decode_base.c \
    $L/lzfse_encode.c $L/lzfse_encode_base.c $L/lzvn_encode_base.c -o lib/liblzfse.so.1
for f in lzfse_decode lzfse_decode_base lzfse_fse lzvn_decode_base lzfse_encode lzfse_encode_base lzvn_encode_base; do
  gcc -O2 -c $L/$f.c -o obj/$f.o
  gcc -g -O1 -fsanitize=address,undefined -fno-sanitize-recover=all -fsanitize-coverage=trace-pc -c $L/$f.c -o obj/$f.fz.o
done
gcc -O2 -I$L "$here/corrupt.c" obj/lzfse_*.o obj/lzvn_*.o -o corrupt
gcc -O2 -I$L "$here/seeds.c" obj/lzfse_*.o obj/lzvn_*.o -o seeds
gcc -g -O1 -fsanitize=address,undefined -I$L "$here/fuzz_lzfse.c" obj/*.fz.o -o fuzz_lzfse

# A corpus like a work folder: source, prose, JSON, binaries, images.
mkdir -p corpus
py=$(python3 -c 'import sysconfig; print(sysconfig.get_paths()["stdlib"])')
cat "$py"/*.py > corpus/python-source.txt
cat "$here"/../../../plan/*/decisions/*.md > corpus/markdown.txt 2>/dev/null || cat "$py"/*.txt > corpus/markdown.txt || true
find "$py" -name '*.json' -size +1k | head -200 | xargs cat > corpus/json.txt 2>/dev/null || true
cp "$(python3 -c 'import sys; print(sys.executable)')" corpus/elf-binary
find /usr/lib -name 'libc.so.6' | head -1 | xargs -I{} cp {} corpus/elf-binary2
find / -xdev -name '*.png' -size +100k 2>/dev/null | head -3 | xargs cat > corpus/png.bin || true

say "== 1. One flipped bit in a decmpfs block (corrupt.c, corrupt_zlib.py)"
./corrupt corpus/* | tee -a "$out"
python3 -I "$here/corrupt_zlib.py" corpus/* | tee -a "$out"

say "== 2. Apple's lzfse decoder fuzzed with hostile blocks (fuzz_lzfse.c), PAD=0 then PAD=8"
rm -rf seedsdir && mkdir seedsdir && ./seeds seedsdir corpus/python-source.txt corpus/json.txt corpus/elf-binary corpus/markdown.txt >/dev/null
for pad in 0 8; do
  mkdir -p "fz$pad"
  r=$(cd "fz$pad" && PAD=$pad ASAN_OPTIONS=detect_leaks=0 ../fuzz_lzfse ../seedsdir "$secs" 2>&1 | grep -m1 -E 'ERROR|inputs run' || true)
  say "PAD=$pad: $r"
done

say "== 3. The APFS content reader fuzzed with a hostile disk (fuzz_reader.py)"
python3 -c 'import atheris' 2>/dev/null || pip install -q atheris
mkdir -p data
for n in case_sensitive; do
  [ -s data/$n.bin ] || { curl -fsSL -o data/$n.bin.gz https://media.githubusercontent.com/media/fox-it/dissect.apfs/main/tests/_data/$n.bin.gz && gunzip -f data/$n.bin.gz; }
done
mkdir -p probe
for f in apfsread.py apfscontent.py; do
  git -C "$here" show "origin/claude/project-thread-pj4x2d:research/mac-test-kit/py/apfs/$f" > probe/$f 2>/dev/null ||
    curl -fsSL -o probe/$f "https://raw.githubusercontent.com/amontlabs/silo/claude/project-thread-pj4x2d/research/mac-test-kit/py/apfs/$f"
done
for who in probe reader; do
  dir=$w/probe; [ $who = reader ] && dir=$here
  mkdir -p "rf-$who" && rm -f "rf-$who/faults.tsv"
  (cd "rf-$who" && COLLECT=faults.tsv LD_LIBRARY_PATH="$w/lib" python3 "$here/fuzz_reader.py" "$dir" "$w/data/case_sensitive.bin" \
     -max_total_time="$secs" -timeout=20 -rss_limit_mb=1500 -seed=1 > log.txt 2>&1 || true)
  runs=$(grep -o '^#[0-9]*' "rf-$who/log.txt" | tail -1)
  say "$who: $runs inputs, $(wc -l < "rf-$who/faults.tsv" 2>/dev/null || echo 0) distinct faults"
  cut -f1-3 "rf-$who/faults.tsv" 2>/dev/null | sed 's/^/  /' | tee -a "$out" || true
  grep -m1 -E 'ERROR: libFuzzer' "rf-$who/log.txt" | sed 's/^/  stopped: /' | tee -a "$out" || true
done

say "== 4. ext4: journal faults, the kernel against the reader (journal-faults.sh)"
if [ "$(id -u)" = 0 ] && command -v mkfs.ext4 >/dev/null; then
  git -C "$here" show "origin/claude/project-thread-2e76z4:research/2026-11-25-linux-main/proto/ext4read.py" > ext4read-381.py 2>/dev/null ||
    curl -fsSL -o ext4read-381.py "https://raw.githubusercontent.com/amontlabs/silo/claude/project-thread-2e76z4/research/2026-11-25-linux-main/proto/ext4read.py"
  say "-- this folder's ext4read.py (checksums verified)"
  mkdir -p jf1 jf2
  sh "$here/journal-faults.sh" "$here/ext4read.py" "$w/jf1" 2>/dev/null | tail -7 | tee -a "$out"
  say "-- #381's trial as it is"
  sh "$here/journal-faults.sh" "$w/ext4read-381.py" "$w/jf2" 2>/dev/null | tail -7 | tee -a "$out"
else
  say "skip: needs root, a loop device and e2fsprogs"
fi
