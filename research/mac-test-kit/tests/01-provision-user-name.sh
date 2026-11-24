#!/bin/bash
# id: 01
# title: Provisioning with the Mac's own login name
# tier: vm
# time: about 30 minutes (two or three first starts)
# needs: the base computer of test 00, macOS 27, no sudo; a person only for the optional look at Setup Assistant
# decides: moving-in-on-macos, step 5 of file 10 (how a new macOS computer gets its user and SSH)
# question: Does Apple's guest provisioning accept the Mac's own login name as the computer's user, and if not, at which stage and with which error is it refused?
# outcome own-name-works: The Mac's own login name was provisioned and SSH answered. moving-in-on-macos
#   holds as written: main's user is your user name, set by provisioning, with no setup user.
# outcome own-name-refused: Provisioning refused the Mac's own login name (the stage and error are in the
#   sentence) and another name worked. moving-in-on-macos needs file 10's workaround: provision
#   silo-setup, then make your user over SSH (step 7). Report FB23241619 as still open.
# outcome own-name-silent: No error, but SSH never answered for the Mac's own login name, while another
#   name worked: the failure shows only as a timeout (and Setup Assistant, if a person looked).
#   Same workaround as own-name-refused, and Silo must treat "SSH does not answer" as a failure.
# outcome both-fail: Neither name gave SSH: provisioning itself does not work on this Mac or this
#   build. Look at the attached run logs before ruling anything.
# outcome inconclusive: Something else happened; the sentence and the attached logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/first-start.sh"

if [ "${KIT_FAKE:-0}" = 1 ]; then
  kit_note "Linux run against linux/fake-silovm"
else
  kit_need_mac 27
fi
kit_need_silovm
kit_need_base_vm
fs_need_provisioning

own="${KIT_OWN_NAME:-$(id -un)}"
other="$VM_USER"
[ "$own" != "$other" ] || other="silo-setup"
kit_fact own_name "$own"
kit_fact other_name "$other"

kit_confirm "Starts two new macOS computers, one after the other, cloned from the base (up to $((FS_SSH_LIMIT / 60 + 1)) minutes each), and maybe a third with a window to look at."

b=""
trap '[ -n "$b" ] && vm_destroy "$b"' EXIT

# try NAME LABEL: one fresh computer provisioned for NAME. Records facts under LABEL.
try_name() {
  local name="$1" label="$2"
  b="$(vm_fresh "$label")" || kit_fail "could not clone the base"
  fs_first_start "$b" "$name"
  kit_fact "${label}_result" "$FS_RESULT"
  kit_fact "${label}_refused_stage" "$FS_STAGE"
  kit_fact "${label}_error_name" "$FS_NAME"
  kit_fact "${label}_error_code" "$FS_CODE"
  kit_fact "${label}_stopped_how" "$FS_HOW"
  if [ "$FS_RESULT" = ssh ]; then
    kit_measure "${label}_seconds_to_ssh" "$FS_SECONDS" s "first start to SSH with the password, user $name"
    local who
    who="$(vm_ssh_pw "$b" "$name" 'id -un; sw_vers -productVersion; sw_vers -buildVersion' 2>/dev/null | tr '\n' ' ')"
    kit_fact "${label}_guest_says" "$who"
  fi
  eval "${label}_result=\$FS_RESULT ${label}_stage=\$FS_STAGE ${label}_err=\$FS_NAME ${label}_seconds=\$FS_SECONDS"
  kit_fact "${label}_stop" "$(vm_stop "$b" 10)"
  fs_keep_log "$b" "$label"
  vm_destroy "$b"; b=""
}

own_result="" own_stage="" own_err="" own_seconds=""
other_result="" other_stage="" other_err="" other_seconds=""
try_name "$own" own
try_name "$other" other

# When the own name failed: look at the screen of a computer started anyway.
seen="not looked"
if [ "$own_result" != ssh ]; then
  if [ -t 0 ]; then
    b="$(vm_fresh own-gui)" || kit_fail "could not clone the base"
    fs_forget_host_keys
    vm_start "$b" --provision-user "$own" --provision-password "$(vm_password)" --provision-full-name "Silo Kit" \
      --continue-on-provisioning-error --gui --max-seconds 900 > /dev/null
    fs_wait_event "$b" started 120 || true
    kit_log "A window shows the computer started for '$own'. Wait until the screen settles (about 2 minutes)."
    sleep "${KIT_LOOK_DELAY:-90}"
    "$SILOVM" ctl --bundle "$b" screenshot "$KIT_WORK/own-gui.png" > "$KIT_WORK/screenshot.json" 2>&1 \
      && kit_attach "$KIT_WORK/own-gui.png"
    kit_attach "$KIT_WORK/screenshot.json"
    seen="$(kit_ask_yn "Does the silovm window show Setup Assistant (a language, country or account screen)?")"
    vm_stop "$b" 30 > /dev/null
    fs_keep_log "$b" own-gui
    vm_destroy "$b"; b=""
  else
    kit_note "no terminal: nobody looked at the screen of a computer started with the refused name"
  fi
fi
kit_fact own_setup_assistant_shown "$seen"

refusal="refused at stage $own_stage with $own_err"
case "$own_result/$other_result" in
  ssh/ssh)
    kit_outcome own-name-works "'$own' (the Mac's login name) was provisioned, SSH answered after $own_seconds s; '$other' after $other_seconds s." ;;
  refused/ssh)
    kit_outcome own-name-refused "'$own' was $refusal; '$other' worked, SSH after $other_seconds s. Setup Assistant shown when started anyway: $seen." ;;
  timeout/ssh)
    kit_outcome own-name-silent "'$own' gave no error, but SSH did not answer within $FS_SSH_LIMIT s; '$other' worked after $other_seconds s. Setup Assistant shown: $seen." ;;
  ssh/*)
    kit_outcome inconclusive "'$own' worked (SSH after $own_seconds s) but '$other' did not ($other_result, stage $other_stage, $other_err): look at logs/other.*" ;;
  */refused|*/timeout|*/stopped)
    kit_outcome both-fail "'$own': $own_result ($own_stage, $own_err); '$other': $other_result ($other_stage, $other_err). Look at the attached run logs." ;;
  *)
    kit_outcome inconclusive "'$own': $own_result ($own_stage, $own_err); '$other': $other_result ($other_stage, $other_err)." ;;
esac
