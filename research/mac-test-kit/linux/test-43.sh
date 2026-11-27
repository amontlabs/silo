#!/bin/bash
# Test 43 on Linux: its starts need a Mac, so it must skip after reading the provisioning options
# from a header. Run against linux/fixtures/43-provisioning.h (silovm's five options): it must find
# no option that runs a program; with one planted (a script URL), it must find it.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit43.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="$tmp/root" KIT_ID=43
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK"
run() { : > "$KIT_OUT"; KIT_43_HEADERS="$1" bash "$KIT_ROOT/tests/43-way-in-before-the-relay.sh" < /dev/null > "$tmp/run.log" 2>&1; }
run "$KIT_ROOT/linux/fixtures/43-provisioning.h"
grep -q '"kind":"skip"' "$KIT_OUT" || { echo "FAIL: test 43 did not skip on Linux"; cat "$tmp/run.log"; exit 1; }
grep -q '"name":"provisioning_can_run_program","value":"no"' "$KIT_OUT" || { echo "FAIL: expected no option that runs a program"; cat "$KIT_OUT"; exit 1; }
grep -q 'enablesRemoteLogin' "$KIT_OUT" || { echo "FAIL: the five options were not read"; exit 1; }
echo "ok   43: silovm's five provisioning options read, none runs a program; the starts skip on Linux"
sed 's|@property BOOL enablesRemoteLogin;|@property BOOL enablesRemoteLogin;\n@property (nullable, copy) NSURL *firstStartScriptURL API_AVAILABLE(macos(27.1));|' \
  "$KIT_ROOT/linux/fixtures/43-provisioning.h" > "$tmp/planted.h"
run "$tmp/planted.h"
grep -q '"name":"provisioning_candidates","value":"VZMacGuestProvisioningOptions.firstStartScriptURL"' "$KIT_OUT" ||
  { echo "FAIL: the planted script option was not found"; cat "$KIT_OUT"; exit 1; }
echo "ok   43: a planted firstStartScriptURL is found"
