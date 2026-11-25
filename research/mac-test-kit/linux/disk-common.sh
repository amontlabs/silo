# Shared by linux/test-03.sh ... test-19.sh: builds the C programs into a temporary kit home and
# runs one test with KIT_FAKE=1. Sourced.
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
export KIT_WORK_ROOT="$T/home" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK_ROOT/bin"
for src in "$KIT_ROOT"/c/*.c; do
  cc -O2 -Wall -o "$KIT_WORK_ROOT/bin/$(basename "$src" .c)" "$src" -lpthread || exit 1
done
disk_run() { # disk_run ID FILE
  export KIT_ID="$1" KIT_WORK="$T/work/$1" KIT_OUT="$T/results.jsonl"
  mkdir -p "$KIT_WORK"; : > "$KIT_OUT"
  bash "$KIT_ROOT/$2" || { echo "the test exited $?"; exit 1; }
  if grep -q '"kind":"skip"' "$KIT_OUT"; then
    echo "skipped here: $(grep '"kind":"skip"' "$KIT_OUT")"; exit 0
  fi
}
disk_expect_measure() {
  grep -q "\"name\":\"$1\"" "$KIT_OUT" || { echo "FAIL: no measure $1"; cat "$KIT_OUT"; exit 1; }
}
disk_expect_outcome() {
  local n; n="$(grep -c '"kind":"outcome"' "$KIT_OUT")"
  [ "$n" = 1 ] || { echo "FAIL: $n outcomes"; cat "$KIT_OUT"; exit 1; }
  grep '"kind":"outcome"' "$KIT_OUT"
}

# disk_apfs_data: fetch dissect.apfs's test disks (Fox-IT, AGPL-3.0, Git LFS; not copied into the
# kit) once into KIT_APFS_DATA. Returns 1 without a network.
export KIT_APFS_DATA="${KIT_APFS_DATA:-${TMPDIR:-/tmp}/silo-kit-apfs-data}"
disk_apfs_data() {
  local n base=https://media.githubusercontent.com/media/fox-it/dissect.apfs/main/tests/_data
  mkdir -p "$KIT_APFS_DATA"
  for n in snapshot corrupt encrypted case_sensitive case_insensitive; do
    [ -s "$KIT_APFS_DATA/$n.bin" ] && continue
    curl -fsSL -o "$KIT_APFS_DATA/$n.bin.gz" "$base/$n.bin.gz" || { echo "cannot fetch dissect.apfs's $n.bin.gz"; return 1; }
    gunzip -f "$KIT_APFS_DATA/$n.bin.gz" || return 1
  done
}
