#!/usr/bin/env bash
# T8 live check: what a guest can reach in each network policy mode, a stopped computer changed
# between modes by the engine's own start, a fork and a restore of an allowlist computer, and the
# export code paths on each. Run:
#   bash docs/direction/checks/t8_network_policy_check.sh 2>&1 | tee docs/direction/evidence/T8.txt
#
# No Silo or Silo Dev engine or app is started, stopped or called. The check is two ignored tests of
# the engine (`network_live` in runtime/checkpoints/tests and `live_export_accepts_each_network_policy`
# in backup.rs), so every argument given to `msb` is the engine's own. They use a private runtime
# home under $HOME/.t8, a copy of the rebuilt `msb` signed for the hypervisor, the Dev guest image
# (read only) and sandboxes named e2e-t8-*; the home is deleted at the end.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
APP="$REPO/app/SiloUI"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/code/projects/silo-wt-targets/T8}
WORK=$HOME/.t8
IMAGES=${SILO_TEST_GUEST_IMAGE:-$HOME/Library/Application\ Support/org.silo.dev/guest-image}
IMAGES=${IMAGES//\\ / }
MSB_BUILT="$APP/src-tauri/binaries/msb-aarch64-apple-darwin"
say() { echo "[$(date +%H:%M:%S)] $*"; }

cleanup() {
  say "cleanup: private runtime homes under $WORK"
  if [ -d "$WORK/tmp" ]; then
    for home in "$WORK"/tmp/t8-*/home; do
      [ -d "$home" ] || continue
      say "sandboxes left in $home: $(MSB_HOME=$home MSB_PATH=$WORK/bin/msb "$WORK/bin/msb" list 2>/dev/null | grep -c 'e2e-t8-')"
    done
  fi
  if [ -n "${T8_KEEP:-}" ]; then say "kept $WORK (T8_KEEP is set)"; return; fi
  rm -rf "$WORK"
  say "removed $WORK"
}
trap cleanup EXIT

rm -rf "$WORK"; mkdir -p "$WORK/bin" "$WORK/tmp" "$WORK/configs"
cp "$MSB_BUILT" "$WORK/bin/msb"
codesign -s - --force --entitlements "$APP/src-tauri/Entitlements.plist" "$WORK/bin/msb" 2>&1 | tail -1
say "runtime: $("$WORK/bin/msb" --version), restore probe: $("$WORK/bin/msb" --silo-restore-strict-protocol)"
say "restore --net-strict: $("$WORK/bin/msb" restore --help | grep -c -- '--net-strict') line(s) in its help"
say "built binary sha256: $(shasum -a 256 "$MSB_BUILT" | cut -d' ' -f1)"

export SILO_TEST_MSB="$WORK/bin/msb"
export SILO_TEST_LIBKRUNFW="$APP/src-tauri/runtime/microsandbox/aarch64-apple-darwin/lib/libkrunfw.5.dylib"
export SILO_TEST_GUEST_IMAGE="$IMAGES"
export SILO_TEST_TMP="$WORK/tmp"
export SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures
export SILO_T8_CONFIGS="$WORK/configs"

cd "$APP" || exit 1
say "== modes, change on start, fork and restore (engine test live_network_policy_modes_restore_and_fork)"
cargo test -p silo-engine --locked --lib live_network_policy_modes_restore_and_fork -- --ignored --nocapture --test-threads=1 2>&1 \
  | grep -E "^(PROBE|allowed|other|direct|secret|name |another|Verified)|test result|panicked|FAILED|^error|assert|left:|right:|^\s+(Reach|example)" 
LIVE=${PIPESTATUS[0]}
say "== export code paths on the runtime's own configurations (engine test live_export_accepts_each_network_policy)"
cargo test -p silo-engine --locked --lib live_export_accepts_each_network_policy -- --ignored --nocapture --test-threads=1 2>&1 \
  | grep -E "^EXPORT|test result|panicked|FAILED|^error"
EXPORT=${PIPESTATUS[0]}
say "live test exit: $LIVE; export test exit: $EXPORT"
exit $((LIVE + EXPORT))
