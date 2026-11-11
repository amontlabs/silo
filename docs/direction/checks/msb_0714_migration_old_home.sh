#!/usr/bin/env bash
# Builds a private "old Silo" app-data folder with the 0.7.6 patched msb (read-only use of the
# silo-direction checkout's binary): usage: bash <this> <evidence-dir>. App data: /tmp/m14.
# Computers are created with the same msb arguments the 0.7.6 engine uses (engine/src/runtime/mod.rs
# create_computer): root disk, an owned /workspace disk, labels, --no-start.
set -uo pipefail
OUT=${1:?evidence dir}; mkdir -p "$OUT"
OLD=$HOME/code/projects/silo-direction/app/SiloUI/src-tauri
ENT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/app/SiloUI/src-tauri/Entitlements.plist
IMG="$HOME/Library/Application Support/org.silo.dev/guest-image/ubuntu-24.04-v5/image.tar.gz"
REF=ghcr.io/amontlabs/silo-guest:ubuntu-24.04-v5-arm64
APPD=${M14_APP:-/tmp/m14};  # short: sockets need < 104 bytes
 STORAGE=$APPD/runtime-checkpoints-converted; export MSB_HOME=$STORAGE/microsandbox
say() { echo "[$(date +%H:%M:%S)] $*"; }
[ -e "$APPD" ] && { echo "$APPD exists; remove it first"; exit 1; }
mkdir -p "$APPD/bin" "$STORAGE" "$APPD/tmp"
cp "$OLD/binaries/msb-aarch64-apple-darwin" "$APPD/bin/msb-old"
codesign -s - --force --entitlements "$ENT" "$APPD/bin/msb-old" 2>&1 | tail -1
M=$APPD/bin/msb-old
export MSB_PATH=$M MSB_LIBKRUNFW_PATH=$OLD/runtime/microsandbox/aarch64-apple-darwin/lib/libkrunfw.5.dylib
[ -f "$MSB_LIBKRUNFW_PATH" ] || { echo "old libkrunfw missing: $MSB_LIBKRUNFW_PATH"; exit 1; }
say "old runtime: $($M --version)"
gunzip -c "$IMG" > "$APPD/tmp/image.tar" && $M image load --input "$APPD/tmp/image.tar" --tag $REF --quiet; say "image load exit $?"
rm -f "$APPD/tmp/image.tar"
create() { $M create $REF --pull never --name "$1" --cpus 1 --max-cpus 1 --memory 1G --max-memory 1G --root-disk 2G \
  --mount-owned /workspace:kind=disk,size=1G --label silo.managed=true --no-start --quiet >/dev/null; }
put() { $M exec "$1" -- sh -c "printf '%s' '$3' > '$2'; sync" >/dev/null; }
create_started() { create "$1" && $M start "$1" >/dev/null; }
say "== e2e-mig-run (left running) and its fork"
create_started e2e-mig-run; put e2e-mig-run /workspace/r run-1
$M fork e2e-mig-run --name e2e-mig-fork --quiet; say "fork exit $?"
put e2e-mig-fork /workspace/f fork-1
$M stop e2e-mig-fork >/dev/null
say "== e2e-mig-main (disk and full checkpoints, then a fork of the same running computer)"
create_started e2e-mig-main; put e2e-mig-main /workspace/a main-1; put e2e-mig-main /root/a main-root-1
$M stop e2e-mig-main >/dev/null
$M snapshot create diskck --sandbox e2e-mig-main --quiet; say "disk checkpoint exit $?"
$M start e2e-mig-main >/dev/null; put e2e-mig-main /workspace/b main-2
$M snapshot create fullck --sandbox e2e-mig-main --full --quiet; say "full checkpoint exit $?"
$M fork e2e-mig-main --name e2e-mig-mfork --quiet; say "fork of main exit $?"
put e2e-mig-mfork /workspace/m mfork-1; $M stop e2e-mig-mfork >/dev/null
put e2e-mig-main /workspace/c main-3
$M stop e2e-mig-main >/dev/null
say "== e2e-mig-disk (disk-only restore, then later writes)"
$M restore e2e-mig-main:diskck --name e2e-mig-disk; say "disk restore exit $?"
$M start e2e-mig-disk >/dev/null 2>&1; put e2e-mig-disk /workspace/d disk-1; $M stop e2e-mig-disk >/dev/null
for n in e2e-mig-late e2e-mig-bad; do say "== $n"; create_started $n; put $n /workspace/l "$n-1"; $M stop $n >/dev/null; done
# a legacy journal that points at another computer's disk: the runtime cannot establish its state
sed -i.bak 's#e2e-mig-bad/upper.ext4#e2e-mig-late/upper.ext4#' "$MSB_HOME/sandboxes/e2e-mig-bad/runtime/root-disk.json" && rm "$MSB_HOME/sandboxes/e2e-mig-bad/runtime/root-disk.json.bak"
$M list 2>&1 | tee "$OUT/old-home-list.txt"
$M snapshot list 2>&1 | tee "$OUT/old-home-snapshots.txt"
say "done; app data $APPD"
