#!/bin/bash
# id: 24
# title: A simulator booted and used over SSH, with and without a user on the screen
# tier: vm
# time: about 25 minutes
# needs: the Xcode computer of test 23, macOS 27 on the Mac
# step: later
# decides: computers-macos-screen, computer-image (whether main logs its user in on its screen), simulators-for-parallel-agents
# question: Over SSH, does a simulator in a computer boot, install and launch an app, and give a screenshot that is not blank, first as the computer starts, then after its user is logged in on its screen?
# outcome works-without-screen-login: Everything worked over SSH with nobody logged in on the
#   computer's screen. Main needs no login on its screen for simulators; the boot time and the
#   screenshot are in the measures.
# outcome needs-screen-login: It failed with nobody on the screen and worked once the user was
#   logged in on it (automatic login, then a restart). Main must log its user in on its own screen
#   at every start, and computer-image must set that up. Run 25 and 27 after this one: they log the
#   user in the same way first.
# outcome already-on-screen: The provisioned computer logged its user in on its screen by itself,
#   and everything worked. Main gets the session simulators need with nothing to set up.
# outcome blank-screenshot: The app ran but every screenshot was blank (one colour). The guest's
#   graphics do not render the simulator; agents cannot check their work by looking at it.
# outcome fails: It failed in both cases; the step and error are in the sentence.
# outcome inconclusive: Something else went wrong; the sentence and the attached logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

ios_need_mac_or_fake "the simulator only runs on macOS"
ios_need_xcode_vm
ios_fake || kit_confirm "This starts a clone of the Xcode computer for about 25 minutes, boots a simulator in it twice and restarts it once."
trap guest_cleanup EXIT
ios_up sim
ios_fake || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "start of a clone of the Xcode computer"

build="$(ios_build_app 2>&1)"
echo "$build" | grep -q built || { kit_outcome inconclusive "the test app did not build: $(echo "$build" | tail -2 | tr '\n' ' ' | cut -c1-200)"; exit 0; }

# try_round LABEL: boot a new simulator, install and launch the app, take a screenshot, shut it
# down. Prints "ok <boot seconds> <colours>" or "fail <step> <message>".
try_round() {
  local label="$1" udid boot out t0 t1 bmp colours
  udid="$(ios_new_device "kit-$label")"
  [ -n "$udid" ] || { echo "fail create $(guest_sh 'xcrun simctl list runtimes' 2>&1 | tail -1)"; return; }
  t0="$(kit_now)"
  boot="$(guest_sh "$GUEST_LIMIT limit 300 xcrun simctl bootstatus '$udid' -b 2>&1; echo rc=\$?")"
  t1="$(kit_now)"
  echo "$boot" | grep -q 'rc=0' || { echo "fail boot $(echo "$boot" | grep -v rc= | tail -1 | cut -c1-160)"; return; }
  out="$(guest_sh "$GUEST_LIMIT xcrun simctl install '$udid' \$HOME/silokit/KitApp.app 2>&1 && limit 60 xcrun simctl launch --terminate-running-process '$udid' dev.silo.kitapp 2>&1; echo rc=\$?")"
  echo "$out" | grep -q 'rc=0' || { echo "fail launch $(echo "$out" | grep -v rc= | tail -1 | cut -c1-160)"; return; }
  ios_wait 8
  guest_sh "xcrun simctl io '$udid' screenshot --type=bmp \$HOME/silokit/shot-$label.bmp >/dev/null 2>&1"
  bmp="$KIT_WORK/shot-$label.bmp"
  guest_get "silokit/shot-$label.bmp" "$bmp" 2>/dev/null
  [ -s "$bmp" ] && kit_attach "$bmp"
  colours="$(ios_bmp_colours "$bmp")"
  guest_sh "xcrun simctl shutdown '$udid'; xcrun simctl delete '$udid'" >/dev/null 2>&1
  echo "ok $(python3 -c "print(round($t1 - $t0, 1))") ${colours:-0}"
}

console="$(ios_console_user)"
kit_fact screen_user_at_start "$console"
on_screen_at_start=no; [ "$console" = none ] || on_screen_at_start=yes
first="$(try_round nologin)"
kit_fact round_as_started "$first"

# Second round: log the user in on the screen at start (automatic login), restart, try again.
if [ "$console" = none ]; then
  console="$(ios_screen_login)" || { kit_outcome inconclusive "the computer did not come back after the restart for automatic login"; exit 0; }
  kit_fact screen_user_after_autologin "$console"
  second="$(try_round login)"
  kit_fact round_with_screen_login "$second"
else
  second="$first"
fi

# shellcheck disable=SC2086
case "$first" in ok*) set -- $first; kit_measure boot_seconds "$2" s "simctl bootstatus -b, as the computer started" ;; esac
# shellcheck disable=SC2086
case "$second" in ok*) set -- $second; kit_measure boot_seconds_logged_in "$2" s "simctl bootstatus -b, user on the screen" ;; esac

colours_of() { echo "$1" | awk '{print $3}'; }
case "$first:$second" in
  ok*:ok*)
    if [ "$(colours_of "$first")" -le 2 ] && [ "$(colours_of "$second")" -le 2 ]; then
      kit_outcome blank-screenshot "the app launched but the screenshots have $(colours_of "$first") and $(colours_of "$second") colours"
    elif [ "$on_screen_at_start" = yes ]; then
      kit_outcome already-on-screen "the provisioned computer logs its user ($console) in on its screen by itself, and the simulator worked: $first"
    else
      kit_outcome works-without-screen-login "the simulator worked with nobody on the screen ($first) and with the user on it ($second)"
    fi ;;
  fail*:ok*) kit_outcome needs-screen-login "with nobody on the screen: $first; with the user on it: $second" ;;
  *) kit_outcome fails "with nobody on the screen: $first; with the user on it: $second" ;;
esac
