#!/bin/bash
# The Linux runs of the iOS tests (23 to 30): each test runs with KIT_FAKE=1 in a temporary folder,
# with the fake silovm of linux/fake-silovm-guest and the fake xcrun, vm_stat, sysctl, sysadminctl,
# security and stat of linux/fake-ios, and must end on the outcome its Linux run is built to
# reach. Called by linux/selftest.sh. Changes nothing outside the temporary folder (test 27 listens
# on 127.0.0.1:18444, test 28's boundary on ports the system picks, for a moment).
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
if [ "$(uname -s)" = Darwin ]; then echo "on a Mac: the Linux runs of the iOS tests are skipped"; exit 0; fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export KIT_WORK_ROOT="$tmp/root" KIT_FAKE=1 KIT_YES=1 KIT_IOS_TLS_PORT=18444 FAKE_SILOVM_STATE="$tmp/silovm-state"
fails=0

# run ID EXPECTED_OUTCOME [MEASURE=VALUE] [ENV=VALUE...]: EXPECTED_OUTCOME is an outcome key, or
# "skip" for a test that must skip itself on Linux.
run() {
  local id="$1" want="$2" measure="${3:-}" f out got
  shift 2; [ $# -gt 0 ] && shift
  f="$(ls "$KIT_ROOT"/tests/"$id"-*.sh)"
  out="$tmp/$id.jsonl"; : > "$out"
  rm -rf "$tmp/work/$id"; mkdir -p "$tmp/work/$id"
  env "$@" KIT_ID="$id" KIT_WORK="$tmp/work/$id" KIT_OUT="$out" bash "$f" > "$tmp/$id.log" 2>&1 < /dev/null
  got="$(python3 -c 'import json,sys
o=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
k=[e["key"] for e in o if e["kind"]=="outcome"]
s=[e for e in o if e["kind"]=="skip"]
err=[e["reason"] for e in o if e["kind"]=="error"]
print(k[-1] if k else ("skip" if s and not err else "none:" + ";".join(err)))' "$out")"
  if [ "$got" != "$want" ]; then
    echo "FAIL $id: outcome $got, expected $want"; tail -15 "$tmp/$id.log"; fails=$((fails + 1)); return
  fi
  if [ -n "$measure" ]; then
    if ! python3 -c 'import json,sys
n,v=sys.argv[2].split("=")
m=[e for e in map(json.loads,open(sys.argv[1])) if e["kind"]=="measure" and e["name"]==n]
sys.exit(0 if m and str(m[-1]["value"])==v else 1)' "$out" "$measure"; then
      echo "FAIL $id: measure $measure not found"; tail -15 "$tmp/$id.log"; fails=$((fails + 1)); return
    fi
  fi
  echo "ok   $id: $got $measure"
}

# The helpers of lib/ios.sh, against the fakes.
(
  export KIT_ID=xx KIT_WORK="$tmp/work/helpers" KIT_OUT="$tmp/helpers.jsonl"
  mkdir -p "$KIT_WORK"; : > "$KIT_OUT"
  # shellcheck source=/dev/null
  . "$KIT_ROOT/lib/common.sh"; . "$KIT_ROOT/lib/vm.sh"; . "$KIT_ROOT/lib/guest.sh"; . "$KIT_ROOT/lib/ios.sh"
  bad=0
  rt="$(ios_runtime_and_type)"
  [ "$rt" = "com.apple.CoreSimulator.SimRuntime.iOS-27-3 com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro" ] || { echo "FAIL ios_runtime_and_type: $rt"; bad=1; }
  u="$(ios_new_device kit-name)"
  [ "$(cat "$GUEST_HOME/.fake-sim/$u/name" 2>/dev/null)" = kit-name ] || { echo "FAIL ios_new_device: the device is not named kit-name"; bad=1; }
  m="$(ios_memory_used_mb)"
  [ "$m" = "3000 0.00" ] || { echo "FAIL ios_memory_used_mb: '$m', expected '3000 0.00'"; bad=1; }
  [ "$(ios_console_user)" = none ] || { echo "FAIL ios_console_user: someone on the screen"; bad=1; }
  HOME="$GUEST_HOME" FAKE_SIM_BLANK=1 xcrun simctl io "$u" screenshot --type=bmp "$tmp/blank.bmp"
  HOME="$GUEST_HOME" xcrun simctl io "$u" screenshot --type=bmp "$tmp/shot.bmp"
  [ "$(ios_bmp_colours "$tmp/blank.bmp")" = 1 ] || { echo "FAIL ios_bmp_colours: blank is not 1"; bad=1; }
  [ "$(ios_bmp_colours "$tmp/shot.bmp")" -gt 100 ] || { echo "FAIL ios_bmp_colours: a picture has few colours"; bad=1; }
  [ "$(ios_bmp_colours "$tmp/none.bmp")" = 0 ] || { echo "FAIL ios_bmp_colours: a missing file is not 0"; bad=1; }
  echo "$(ios_build_app)" | grep -q built || { echo "FAIL ios_build_app"; bad=1; }
  exit "$bad"
) && echo "ok   lib/ios.sh helpers" || fails=$((fails + 1))

run 23 skip
run 24 needs-screen-login "" FAKE_SIM_NEEDS_CONSOLE=1
run 24 works-without-screen-login
run 24 blank-screenshot "" FAKE_SIM_BLANK=1
run 25 measured simulators_booted=3
run 26 skip
run 27 separate-store
run 28 inconclusive linux_boundary_ok=1
run 29 inconclusive linux_parsers_ok=1
run 30 inconclusive linux_parsers_ok=1
exit $((fails > 0))
