#!/bin/bash
# id: 24
# title: A simulator booted and used over SSH, with and without a user on the screen
# tier: vm
# time: about 25 minutes
# needs: the Xcode computer of test 23, macOS 27, no sudo on the Mac
# decides: computers-macos-screen, computer-image (whether main logs its user in on its screen), simulators-for-parallel-agents
# question: Over SSH, does a simulator in a computer boot, install and launch an app, and give a screenshot that is not blank, first as the computer starts, then after its user is logged in on its screen?
# outcome works-without-screen-login: Everything worked over SSH with nobody logged in on the
#   computer's screen. Main needs no login on its screen for simulators; the boot time and the
#   screenshot are in the measures.
# outcome needs-screen-login: It failed with nobody on the screen and worked once the user was
#   logged in on it (automatic login, then a restart). Main must log its user in on its own screen
#   at every start, and computer-image must set that up.
# outcome already-on-screen: The provisioned computer logged its user in on its screen by itself,
#   and everything worked. Main gets the session simulators need with nothing to set up.
# outcome blank-screenshot: The app ran but every screenshot was blank (one colour). The guest's
#   graphics do not render the simulator; agents cannot check their work by looking at it.
# outcome fails: It failed in both cases; the step and error are in the sentence.
# outcome inconclusive: something else went wrong; the sentence and logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

guest_local && kit_skip "needs a Mac: the simulator only runs on macOS"
kit_need_mac 27
kit_need_silovm
ios_need_xcode_vm
kit_confirm "This starts a clone of the Xcode computer for about 25 minutes, boots a simulator in it twice and restarts it once."
trap guest_cleanup EXIT
ios_up sim
kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "start of a clone of the Xcode computer"

build="$(ios_build_app 2>&1)"
echo "$build" | grep -q built || { kit_outcome inconclusive "the test app did not build: $(echo "$build" | tail -2 | tr '\n' ' ' | cut -c1-200)"; exit 0; }

# try_round LABEL: boot a new simulator, install and launch the app, take a screenshot, shut it
# down. Prints "ok <boot seconds> <colours>" or "fail <step> <message>".
try_round() {
  local label="$1" udid boot t0 t1 png colours
  udid="$(ios_new_device "kit-$label")"
  [ -n "$udid" ] || { echo "fail create $(guest_sh 'xcrun simctl list runtimes' 2>&1 | tail -1)"; return; }
  t0="$(kit_now)"
  boot="$(guest_sh "$GUEST_LIMIT limit 300 xcrun simctl bootstatus '$udid' -b 2>&1; echo rc=\$?")"
  t1="$(kit_now)"
  echo "$boot" | grep -q 'rc=0' || { echo "fail boot $(echo "$boot" | grep -v rc= | tail -1 | cut -c1-160)"; return; }
  out="$(guest_sh "$GUEST_LIMIT xcrun simctl install '$udid' \$HOME/silokit/KitApp.app 2>&1 && limit 60 xcrun simctl launch --terminate-running-process '$udid' dev.silo.kitapp 2>&1; echo rc=\$?")"
  echo "$out" | grep -q 'rc=0' || { echo "fail launch $(echo "$out" | grep -v rc= | tail -1 | cut -c1-160)"; return; }
  sleep 8
  guest_sh "xcrun simctl io '$udid' screenshot \$HOME/silokit/shot-$label.png >/dev/null 2>&1"
  png="$KIT_WORK/shot-$label.png"
  guest_get "silokit/shot-$label.png" "$png" 2>/dev/null
  [ -s "$png" ] && kit_attach "$png"
  colours="$(python3 - "$png" <<'PY' 2>/dev/null
import subprocess, sys, os, tempfile
src = sys.argv[1]
bmp = tempfile.mktemp(suffix=".bmp")
subprocess.run(["sips", "-s", "format", "bmp", src, "--out", bmp], capture_output=True)
data = open(bmp, "rb").read()
off = int.from_bytes(data[10:14], "little")
px = data[off:]
step = max(4, (len(px) // 20000) // 4 * 4)
print(len(set(px[i:i + 3] for i in range(0, len(px) - 3, step))))
os.remove(bmp)
PY
)"
  guest_sh "xcrun simctl shutdown '$udid'; xcrun simctl delete '$udid'" >/dev/null 2>&1
  echo "ok $(python3 -c "print(round($t1 - $t0, 1))") ${colours:-0}"
}

console="$(ios_console_user)"
kit_fact screen_user_at_start "$console"
first="$(try_round nologin)"
kit_fact round_as_started "$first"

# Second round: log the user in on the screen at start (automatic login), restart, try again.
if [ "$console" = none ]; then
  pw="$(vm_password)"
  guest_sudo "sysadminctl -autologin set -userName $VM_USER -password '$pw'" >/dev/null 2>&1
  guest_reboot >/dev/null || { kit_outcome inconclusive "the computer did not come back after the restart for automatic login"; exit 0; }
  sleep 20
  console="$(ios_console_user)"
  kit_fact screen_user_after_autologin "$console"
  second="$(try_round login)"
  kit_fact round_with_screen_login "$second"
else
  second="$first"
fi

case "$first" in ok*) set -- $first; kit_measure boot_seconds "$2" s "simctl bootstatus -b, nobody on the screen" ;; esac
case "$second" in ok*) set -- $second; kit_measure boot_seconds_logged_in "$2" s "simctl bootstatus -b, user on the screen" ;; esac

colours_of() { echo "$1" | awk '{print $3}'; }
case "$first:$second" in
  ok*:ok*)
    if [ "$(colours_of "$first")" -le 2 ] && [ "$(colours_of "$second")" -le 2 ]; then
      kit_outcome blank-screenshot "the app launched but the screenshots have $(colours_of "$first") and $(colours_of "$second") colours"
    elif [ "$first" = "$second" ]; then
      kit_outcome already-on-screen "the provisioned computer logs its user ($console) in on its screen by itself, and the simulator worked: $first"
    else
      kit_outcome works-without-screen-login "the simulator worked with nobody on the screen ($first) and with the user on it ($second)"
    fi ;;
  fail*:ok*) kit_outcome needs-screen-login "with nobody on the screen: $first; with the user on it: $second" ;;
  *) kit_outcome fails "with nobody on the screen: $first; with the user on it: $second" ;;
esac
