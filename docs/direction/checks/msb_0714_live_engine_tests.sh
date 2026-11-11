#!/usr/bin/env bash
# Live engine tests against the rebuilt 0.7.14 runtime in a private home (~/.m14), like T8.
# Never touches Silo / Silo Dev homes, engines or Keychain. Usage: bash <this> <evidence-dir>
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
APP="$REPO/app/SiloUI"
OUT=${1:?evidence dir}; mkdir -p "$OUT"
WORK=$HOME/.m14
IMAGES="$HOME/Library/Application Support/org.silo.dev/guest-image"   # read only
say() { echo "[$(date +%H:%M:%S)] $*"; }
TMPD=/tmp/m14t   # short: runtime sockets need paths under 104 bytes
trap 'rm -rf "$WORK" "$TMPD"; say "removed $WORK $TMPD"' EXIT
rm -rf "$WORK" "$TMPD"; mkdir -p "$WORK/bin" "$TMPD"
cp "$APP/src-tauri/binaries/msb-aarch64-apple-darwin" "$WORK/bin/msb"
codesign -s - --force --entitlements "$APP/src-tauri/Entitlements.plist" "$WORK/bin/msb" 2>&1 | tail -1
say "runtime $("$WORK/bin/msb" --version)"
export SILO_TEST_MSB="$WORK/bin/msb"
export SILO_TEST_LIBKRUNFW="$APP/src-tauri/runtime/microsandbox/aarch64-apple-darwin/lib/libkrunfw.5.dylib"
export SILO_TEST_GUEST_IMAGE="$IMAGES"
export SILO_TEST_TMP="$TMPD"
export SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures
export SILO_T8_CONFIGS="$WORK/configs"; mkdir -p "$SILO_T8_CONFIGS"
cd "$APP" || exit 1
link="$APP/src-tauri/runtime/guest-image"   # symlink (ignored by git) to the Dev image dir, read only
run() { # name, test filter (only those in $ONLY when set)
  [ -n "${ONLY:-}" ] && [[ " $ONLY " != *" $1 "* ]] && return
  say "== $1"
  cargo test -p silo-engine --locked --lib "$2" -- --ignored --nocapture --test-threads=1 > "$OUT/live-$1.log" 2>&1
  echo "exit $?" >> "$OUT/live-$1.log"
  grep -E "^test result|^exit|panicked|FAILED" "$OUT/live-$1.log" | head -6
}
run checkpoints live_checkpoint_restore_and_fork_use_the_runtimes_names
run network live_network_policy_modes_restore_and_fork
run export-network live_export_accepts_each_network_policy
run account live_start_sets_up_the_silo_account_of_new_and_older_computers
run secrets live_secret_adapter_uses_refs_and_preserves_boot_for_live_updates
run export-import real_backup_restore_preserves_root_and_computer_without_original_cache
run checkpoint-export real_checkpoint_export_imports_and_cold_boots_checkpoint_time_disk
run lifecycle-recovery lifecycle_recovery_survives_real_worker_exit_without_repeating_restart
